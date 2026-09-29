//! What `/v1` needs to know, held in memory.
//!
//! A snapshot is built from the database and then never changes. A change
//! to keys, providers or users builds a new one that replaces it, so a
//! model call never waits for the database.

use std::collections::HashMap;
use std::fmt;

use anyhow::Result;
use ultrafast_translate::provider::ProviderKind;

use crate::secrets::Cipher;
use crate::store::Store;

#[derive(Debug, Clone)]
pub struct SnapKey {
    pub id: i64,
    pub name: String,
    pub user_id: Option<i64>,
    pub team_id: Option<i64>,
    pub expires_at: Option<String>,
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
    keys: HashMap<String, SnapKey>,
    /// By name.
    providers: HashMap<String, SnapProvider>,
}

impl Snapshot {
    /// Reads the keys that can work and every usable provider. A provider
    /// that cannot be used is logged and left out; it does not fail the load.
    pub async fn load(store: &Store, cipher: &Cipher) -> Result<Snapshot> {
        let keys = store
            .live_keys()
            .await?
            .into_iter()
            .map(|k| {
                let key = SnapKey {
                    id: k.id,
                    name: k.name,
                    user_id: k.user_id,
                    team_id: k.team_id,
                    expires_at: k.expires_at,
                };
                (k.hash, key)
            })
            .collect();

        let mut providers = HashMap::new();
        for p in store.list_providers().await? {
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
        Ok(Snapshot { keys, providers })
    }

    /// The key for this hash, unless it has expired as of `now` (UTC,
    /// `YYYY-MM-DD HH:MM:SS`). A key stops working at `expires_at`.
    pub fn key(&self, hash: &str, now: &str) -> Option<&SnapKey> {
        self.keys
            .get(hash)
            .filter(|k| k.expires_at.as_deref().is_none_or(|at| at > now))
    }

    pub fn provider(&self, name: &str) -> Option<&SnapProvider> {
        self.providers.get(name)
    }

    /// How many keys are held, expired ones included.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}
