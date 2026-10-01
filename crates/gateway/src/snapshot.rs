//! What `/v1` needs to know, held in memory.
//!
//! A snapshot is built from the database and then never changes. A change
//! to keys, providers or users builds a new one that replaces it, so a
//! model call never waits for the database.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use ultrafast_translate::provider::ProviderKind;

use crate::identity::{Role, UserStatus};
use crate::routing::{BreakerSettings, TargetRef};
use crate::secrets::Cipher;
use crate::store::Store;

#[derive(Debug, Clone)]
pub struct SnapKey {
    pub id: i64,
    pub name: String,
    pub user_id: Option<i64>,
    pub team_id: Option<i64>,
    pub expires_at: Option<String>,
    /// The names (`provider/model` or a route) the key may call; `None` is
    /// no allowlist.
    pub allowed: Option<HashSet<String>>,
}

/// A catalog model of a provider that is in the snapshot.
#[derive(Debug, Clone)]
pub struct SnapModel {
    pub id: i64,
    pub provider: String,
    pub name: String,
    pub enabled: bool,
    pub everyone: bool,
    pub team_ids: HashSet<i64>,
    pub user_ids: HashSet<i64>,
}

#[derive(Debug, Clone)]
pub struct SnapRoute {
    pub id: i64,
    pub name: String,
    /// Every user may use the route. Otherwise its teams and admins.
    pub everyone: bool,
    pub team_ids: HashSet<i64>,
    /// The primaries in order, with their weights. Targets whose model is
    /// gone from the catalog or whose provider cannot be used are left out.
    pub primaries: Vec<(TargetRef, u32)>,
    /// The fallbacks in order, tried after every primary.
    pub fallbacks: Vec<TargetRef>,
    pub retries: u32,
    pub first_token_timeout: Duration,
    pub total_timeout: Duration,
    pub breaker: BreakerSettings,
}

impl SnapRoute {
    /// Primaries in order, then fallbacks in order.
    pub fn targets(&self) -> impl Iterator<Item = &TargetRef> {
        self.primaries
            .iter()
            .map(|(t, _)| t)
            .chain(self.fallbacks.iter())
    }

    pub fn has_targets(&self) -> bool {
        !self.primaries.is_empty() || !self.fallbacks.is_empty()
    }
}

/// An active user, as far as access goes.
#[derive(Debug, Clone)]
pub struct SnapUser {
    pub role: Role,
    pub team_ids: HashSet<i64>,
}

/// A provider with its credential in the clear. It is never serialized.
#[derive(Clone)]
pub struct SnapProvider {
    pub id: i64,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
}

/// Shows only whether there is a credential, never the credential.
impl fmt::Debug for SnapProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_some() {
            "<redacted>"
        } else {
            "<none>"
        };
        f.debug_struct("SnapProvider")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind.as_str())
            .field("base_url", &self.base_url)
            .field("api_key", &api_key)
            .finish()
    }
}

/// It holds credentials, so it has no `Debug` and no `Serialize`.
pub struct Snapshot {
    /// By the hash of the key.
    keys: HashMap<String, Arc<SnapKey>>,
    /// By name.
    providers: HashMap<String, SnapProvider>,
    /// By provider name and model name.
    models: HashMap<(String, String), SnapModel>,
    /// By name.
    routes: HashMap<String, SnapRoute>,
    /// Active users, by id.
    users: HashMap<i64, SnapUser>,
}

impl Snapshot {
    /// Reads the keys that can work and every usable provider. A provider
    /// that cannot be used is logged and left out; it does not fail the load.
    pub async fn load(store: &Store, cipher: &Cipher) -> Result<Snapshot> {
        // One read transaction: the tables are never read at different moments.
        let rows = store.snapshot_rows().await?;
        let keys = rows
            .keys
            .into_iter()
            .map(|k| {
                let key = SnapKey {
                    id: k.id,
                    name: k.name,
                    user_id: k.user_id,
                    team_id: k.team_id,
                    expires_at: k.expires_at,
                    allowed: k.allowed.map(|names| names.into_iter().collect()),
                };
                (k.hash, Arc::new(key))
            })
            .collect();

        let mut providers = HashMap::new();
        for p in rows.providers {
            let Some(kind) = ProviderKind::parse(&p.kind) else {
                tracing::error!(provider = %p.name, "provider left out: unknown kind");
                continue;
            };
            let api_key = match p.credential.as_deref().map(|c| cipher.decrypt(c)) {
                None => None,
                Some(Ok(bytes)) => match String::from_utf8(bytes) {
                    Ok(text) => Some(text),
                    Err(_) => {
                        tracing::error!(provider = %p.name, "provider left out: credential is not text");
                        continue;
                    }
                },
                Some(Err(_)) => {
                    tracing::error!(provider = %p.name, "provider left out: credential cannot be decrypted");
                    continue;
                }
            };
            let provider = SnapProvider {
                id: p.id,
                name: p.name.clone(),
                kind,
                base_url: p.base_url,
                api_key,
            };
            providers.insert(p.name, provider);
        }

        let mut models: HashMap<(String, String), SnapModel> = HashMap::new();
        let mut by_id: HashMap<i64, (String, String)> = HashMap::new();
        for m in rows.models {
            // A model of a provider that cannot be used is not there.
            if !providers.contains_key(&m.provider_name) {
                continue;
            }
            by_id.insert(m.id, (m.provider_name.clone(), m.name.clone()));
            let model = SnapModel {
                id: m.id,
                provider: m.provider_name.clone(),
                name: m.name.clone(),
                enabled: m.enabled,
                everyone: false,
                team_ids: HashSet::new(),
                user_ids: HashSet::new(),
            };
            models.insert((m.provider_name, m.name), model);
        }
        for g in rows.model_grants {
            let Some(name) = by_id.get(&g.model_id) else {
                continue;
            };
            let Some(model) = models.get_mut(name) else {
                continue;
            };
            match (g.team_id, g.user_id) {
                (Some(team), _) => {
                    model.team_ids.insert(team);
                }
                (None, Some(user)) => {
                    model.user_ids.insert(user);
                }
                (None, None) => model.everyone = true,
            }
        }

        let mut routes: HashMap<String, SnapRoute> = HashMap::new();
        let mut route_names: HashMap<i64, String> = HashMap::new();
        for r in rows.routes {
            route_names.insert(r.id, r.name.clone());
            let st = r.settings;
            // Stored values are validated; a stray negative one is read as 0.
            let secs = |n: i64| u64::try_from(n).unwrap_or(0);
            routes.insert(
                r.name.clone(),
                SnapRoute {
                    id: r.id,
                    name: r.name,
                    everyone: r.everyone,
                    team_ids: HashSet::new(),
                    primaries: Vec::new(),
                    fallbacks: Vec::new(),
                    retries: u32::try_from(st.retries).unwrap_or(0),
                    first_token_timeout: Duration::from_millis(secs(st.first_token_timeout_ms)),
                    total_timeout: Duration::from_millis(secs(st.total_timeout_ms)),
                    breaker: BreakerSettings {
                        failures: u32::try_from(st.breaker_failures).unwrap_or(0).max(1),
                        window: Duration::from_secs(secs(st.breaker_window_s)),
                        open: Duration::from_secs(secs(st.breaker_open_s)),
                    },
                },
            );
        }
        for (route_id, team_id) in rows.route_grants {
            if let Some(route) = route_names.get(&route_id).and_then(|n| routes.get_mut(n)) {
                route.team_ids.insert(team_id);
            }
        }
        for t in rows.route_targets {
            let key = (t.provider_name, t.model_name);
            let Some(model) = models.get(&key) else {
                continue;
            };
            let target = TargetRef {
                provider: key.0,
                model: key.1,
                model_id: model.id,
            };
            if let Some(route) = route_names.get(&t.route_id).and_then(|n| routes.get_mut(n)) {
                if t.primary {
                    let weight = u32::try_from(t.weight).unwrap_or(0);
                    route.primaries.push((target, weight));
                } else {
                    route.fallbacks.push(target);
                }
            }
        }

        let active: Vec<_> = rows
            .users
            .into_iter()
            .filter(|u| u.status == UserStatus::Active)
            .collect();
        let mut teams = rows.teams;
        let users = active
            .into_iter()
            .map(|u| {
                let team_ids = teams
                    .remove(&u.id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| t.team_id)
                    .collect();
                (
                    u.id,
                    SnapUser {
                        role: u.role,
                        team_ids,
                    },
                )
            })
            .collect();
        Ok(Snapshot {
            keys,
            providers,
            models,
            routes,
            users,
        })
    }

    /// The key for this hash, unless it has expired as of `now` (UTC,
    /// `YYYY-MM-DD HH:MM:SS`). A key stops working at `expires_at`.
    pub fn key(&self, hash: &str, now: &str) -> Option<&Arc<SnapKey>> {
        self.keys
            .get(hash)
            .filter(|k| k.expires_at.as_deref().is_none_or(|at| at > now))
    }

    pub fn provider(&self, name: &str) -> Option<&SnapProvider> {
        self.providers.get(name)
    }

    pub fn model(&self, provider: &str, name: &str) -> Option<&SnapModel> {
        self.models.get(&(provider.to_string(), name.to_string()))
    }

    pub fn models(&self) -> impl Iterator<Item = &SnapModel> {
        self.models.values()
    }

    pub fn route(&self, name: &str) -> Option<&SnapRoute> {
        self.routes.get(name)
    }

    pub fn routes(&self) -> impl Iterator<Item = &SnapRoute> {
        self.routes.values()
    }

    /// An active user.
    pub fn user(&self, id: i64) -> Option<&SnapUser> {
        self.users.get(&id)
    }

    /// How many keys are held, expired ones included.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}
