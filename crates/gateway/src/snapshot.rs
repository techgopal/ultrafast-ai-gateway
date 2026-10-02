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

use crate::budgets::Budget;
use crate::cache::RouteCache;
use crate::identity::{Role, UserStatus};
use crate::limits::{LimitScope, Subject, Subjects};
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
    pub team_ids: Vec<i64>,
    pub user_ids: Vec<i64>,
    /// Per million tokens, in millionths of a dollar; `None` is unknown.
    pub input_price_micros: Option<i64>,
    pub output_price_micros: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct SnapRoute {
    pub id: i64,
    pub name: String,
    /// Every user may use the route. Otherwise its teams and admins.
    pub everyone: bool,
    pub team_ids: Vec<i64>,
    /// The primaries in order, with their weights. Targets whose model is
    /// gone from the catalog or whose provider cannot be used are left out.
    pub primaries: Vec<(TargetRef, u32)>,
    /// The fallbacks in order, tried after every primary.
    pub fallbacks: Vec<TargetRef>,
    pub retries: u32,
    pub first_token_timeout: Duration,
    pub total_timeout: Duration,
    pub breaker: BreakerSettings,
    /// Whether and how the route keeps answers.
    pub cache: RouteCache,
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
    pub team_ids: Vec<i64>,
}

/// A provider with its credential in the clear. It is never serialized.
#[derive(Clone)]
pub struct SnapProvider {
    pub id: i64,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
    /// Azure OpenAI only.
    pub api_version: Option<String>,
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
            .field("api_version", &self.api_version)
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
    /// By provider name, then model name: a call looks it up by `&str`
    /// without building a key.
    models: HashMap<String, HashMap<String, SnapModel>>,
    /// By name.
    routes: HashMap<String, SnapRoute>,
    /// Active users, by id.
    users: HashMap<i64, SnapUser>,
    /// The rate limits, by what they are set on (id 0 for the gateway).
    limits: HashMap<(LimitScope, i64), Arc<Subject>>,
    /// The budgets, by what they are set on (id 0 for the gateway).
    budgets: HashMap<(LimitScope, i64), Vec<Arc<Budget>>>,
}

/// Grants are few; a list keeps each id once.
fn push_new(ids: &mut Vec<i64>, id: i64) {
    if !ids.contains(&id) {
        ids.push(id);
    }
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
                api_version: p.api_version,
            };
            providers.insert(p.name, provider);
        }

        let mut models: HashMap<String, HashMap<String, SnapModel>> = HashMap::new();
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
                team_ids: Vec::new(),
                user_ids: Vec::new(),
                input_price_micros: m.input_price_micros,
                output_price_micros: m.output_price_micros,
            };
            models
                .entry(m.provider_name)
                .or_default()
                .insert(m.name, model);
        }
        for g in rows.model_grants {
            let Some(name) = by_id.get(&g.model_id) else {
                continue;
            };
            let Some(model) = models.get_mut(&name.0).and_then(|m| m.get_mut(&name.1)) else {
                continue;
            };
            match (g.team_id, g.user_id) {
                (Some(team), _) => push_new(&mut model.team_ids, team),
                (None, Some(user)) => push_new(&mut model.user_ids, user),
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
                    team_ids: Vec::new(),
                    primaries: Vec::new(),
                    fallbacks: Vec::new(),
                    retries: u32::try_from(st.retries).unwrap_or(0),
                    first_token_timeout: Duration::from_millis(secs(st.first_token_timeout_ms)),
                    total_timeout: Duration::from_millis(secs(st.total_timeout_ms)),
                    cache: r.cache,
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
                push_new(&mut route.team_ids, team_id);
            }
        }
        for t in rows.route_targets {
            let Some(model) = models
                .get(&t.provider_name)
                .and_then(|m| m.get(&t.model_name))
            else {
                continue;
            };
            let target = TargetRef {
                provider: t.provider_name,
                model: t.model_name,
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
        // A limit whose team, user or key is gone has nothing to limit.
        let limits = rows
            .limits
            .into_iter()
            .filter(|l| l.has_subject() && !l.limit.is_none())
            .map(|l| {
                let id = l.scope_id.unwrap_or(0);
                let subject = Subject {
                    scope: l.scope,
                    id,
                    label: l.label(),
                    limit: l.limit,
                };
                ((l.scope, id), Arc::new(subject))
            })
            .collect();
        let mut budgets: HashMap<(LimitScope, i64), Vec<Arc<Budget>>> = HashMap::new();
        for b in rows.budgets.into_iter().filter(|b| b.has_subject()) {
            let id = b.scope_id.unwrap_or(0);
            budgets
                .entry((b.scope, id))
                .or_default()
                .push(Arc::new(Budget {
                    id: b.id,
                    scope: b.scope,
                    scope_id: id,
                    scope_label: b.label(),
                    amount_micros: b.amount_micros,
                    period: b.period,
                    action: b.action,
                }));
        }
        Ok(Snapshot {
            keys,
            providers,
            models,
            routes,
            users,
            limits,
            budgets,
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
        self.models.get(provider)?.get(name)
    }

    pub fn models(&self) -> impl Iterator<Item = &SnapModel> {
        self.models.values().flat_map(|m| m.values())
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

    /// The limits that apply to a call of this key: its own, its owner's, those
    /// of the owner's teams and of the key's team, and the gateway's. Only
    /// subjects that have a limit are in it.
    pub fn subjects(&self, key: &SnapKey) -> Subjects {
        if self.limits.is_empty() {
            return Subjects::default();
        }
        let get = |scope, id| self.limits.get(&(scope, id)).cloned();
        Subjects {
            key: get(LimitScope::Key, key.id),
            user: key.user_id.and_then(|u| get(LimitScope::User, u)),
            teams: self
                .team_ids_of(key.user_id, key.team_id)
                .into_iter()
                .filter_map(|t| get(LimitScope::Team, t))
                .collect(),
            gateway: get(LimitScope::Gateway, 0),
        }
    }

    /// The teams a call counts for: those of the key's owner and the key's
    /// own team.
    fn team_ids_of(&self, user_id: Option<i64>, team_id: Option<i64>) -> Vec<i64> {
        let mut team_ids: Vec<i64> = user_id
            .and_then(|u| self.users.get(&u))
            .map(|u| u.team_ids.clone())
            .unwrap_or_default();
        if let Some(team) = team_id {
            push_new(&mut team_ids, team);
        }
        team_ids
    }

    /// The budgets that apply to a call of this key, owner and team: those
    /// of the key, its owner, their teams (as for [`Snapshot::subjects`])
    /// and the gateway, in that order. The log writer calls it with the
    /// ids of a record, so a key that is gone still counts.
    pub fn budgets_of(
        &self,
        key_id: i64,
        user_id: Option<i64>,
        team_id: Option<i64>,
    ) -> Vec<Arc<Budget>> {
        if self.budgets.is_empty() {
            return Vec::new();
        }
        let get = |scope, id| {
            self.budgets
                .get(&(scope, id))
                .into_iter()
                .flatten()
                .cloned()
        };
        let mut out: Vec<Arc<Budget>> = get(LimitScope::Key, key_id).collect();
        if let Some(u) = user_id {
            out.extend(get(LimitScope::User, u));
        }
        for t in self.team_ids_of(user_id, team_id) {
            out.extend(get(LimitScope::Team, t));
        }
        out.extend(get(LimitScope::Gateway, 0));
        out
    }

    /// Every budget.
    pub fn all_budgets(&self) -> Vec<Arc<Budget>> {
        self.budgets.values().flatten().cloned().collect()
    }

    /// How many keys are held, expired ones included.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}
