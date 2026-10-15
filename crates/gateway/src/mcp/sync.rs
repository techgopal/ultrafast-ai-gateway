//! Tool discovery: ask a server what it offers and bring the stored tools
//! in line (new tools arrive switched off, see `Tx::upsert_tools`).

use std::fmt;

use super::decrypt_secret;
use super::upstream::{discover_tools, UpstreamError};
use crate::app::AppState;
use crate::identity::Principal;
use crate::store::{AuditEntry, SyncDelta};

/// The longest a background pass waits for one server's sync, so a stuck
/// server cannot stall the rest. A manual sync keeps only the upstream
/// client's own per-call limits. (`state.http` never follows redirects, so
/// the credentials of a server are sent to its own URL only.)
pub const BACKGROUND_SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Who asked for the sync, for the audit trail. A later automatic sync has
/// no user.
#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: Option<i64>,
    pub email: String,
}

impl Actor {
    pub fn of(p: &Principal) -> Self {
        Self {
            user_id: Some(p.user_id),
            email: p.email.clone(),
        }
    }
}

/// Why a sync did not happen. The text is ours: it never holds upstream text
/// and is what is stored as the server's last sync error.
#[derive(Debug)]
pub enum SyncError {
    /// There is no such server.
    NotFound,
    /// The secret cannot be read or is missing.
    Unusable,
    Upstream(UpstreamError),
    Store(anyhow::Error),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("no such server"),
            Self::Unusable => f.write_str("the server's secret is missing or cannot be read"),
            Self::Upstream(e) => e.fmt(f),
            Self::Store(_) => f.write_str("the result could not be stored"),
        }
    }
}

impl std::error::Error for SyncError {}

impl From<anyhow::Error> for SyncError {
    fn from(e: anyhow::Error) -> Self {
        Self::Store(e)
    }
}

/// What a sync did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncOutcome {
    pub delta: SyncDelta,
    /// Tools the upstream listed that were left out.
    pub skipped: usize,
}

/// Reads the tools of the server and stores them. On a failure nothing about
/// the tools changes; the server records our summary of what went wrong.
pub async fn sync_server(
    state: &AppState,
    server_id: i64,
    actor: &Actor,
) -> Result<SyncOutcome, SyncError> {
    let row = state
        .store
        .server_by_id(server_id)
        .await?
        .ok_or(SyncError::NotFound)?;
    if row.is_local() {
        return sync_local(state, &row, actor).await;
    }
    let secret = if row.auth_kind == "none" {
        None
    } else {
        match row
            .auth_secret_enc
            .as_deref()
            .and_then(|s| decrypt_secret(&state.cipher, s))
        {
            Some(s) => Some(s),
            None => {
                record_failure(state, &row, actor, &SyncError::Unusable).await;
                return Err(SyncError::Unusable);
            }
        }
    };
    let found =
        if row.is_oauth() {
            // A token for the server, as a bearer secret, in place of a stored one.
            match super::oauth::Config::of_row(&row, &state.cipher) {
                Some(cfg) => state
                    .oauth
                    .with_bearer(&cfg, |token| {
                        let mut as_bearer = row.clone();
                        as_bearer.auth_kind = "bearer".to_string();
                        async move { discover_tools(&state.http, &as_bearer, Some(&token)).await }
                    })
                    .await,
                None => {
                    record_failure(state, &row, actor, &SyncError::Unusable).await;
                    return Err(SyncError::Unusable);
                }
            }
        } else {
            discover_tools(&state.http, &row, secret.as_deref()).await
        };
    let discovered = match found {
        Ok(found) => found,
        Err(e) => {
            let e = SyncError::Upstream(e);
            record_failure(state, &row, actor, &e).await;
            return Err(e);
        }
    };
    store_found(state, &row, actor, discovered).await
}

/// A local server is asked through its running program.
async fn sync_local(
    state: &AppState,
    row: &crate::store::McpServerRow,
    actor: &Actor,
) -> Result<SyncOutcome, SyncError> {
    let found = state
        .local
        .discover(row.id, super::upstream::Options::default())
        .await;
    match found {
        Ok(found) => store_found(state, row, actor, found).await,
        Err(e) => {
            let e = SyncError::Upstream(e);
            record_failure(state, row, actor, &e).await;
            Err(e)
        }
    }
}

/// Stores what a discovery found.
async fn store_found(
    state: &AppState,
    row: &crate::store::McpServerRow,
    actor: &Actor,
    discovered: super::upstream::Discovered,
) -> Result<SyncOutcome, SyncError> {
    // A name the registry cannot hold is left out, not an error.
    let mut skipped = discovered.skipped;
    let mut found = discovered.tools;
    let listed = found.len();
    found.retain(|t| !t.name.is_empty() && t.name.len() <= super::MAX_TOOL_NAME_BYTES);
    skipped += listed - found.len();

    let mut tx = state.store.begin_immediate().await?;
    // A server deleted while the upstream answered: nothing to store.
    if !tx.record_sync(row.id, None).await? {
        return Err(SyncError::NotFound);
    }
    let delta = tx.upsert_tools(row.id, found).await?;
    tx.audit(AuditEntry {
        actor_user_id: actor.user_id,
        actor_email: &actor.email,
        action: "mcp.sync",
        target_type: "mcp_server",
        target_id: Some(row.id),
        summary: &format!(
            "Synced tools of {}: {} new, {} changed, {} removed, {} skipped",
            row.name,
            delta.added.len(),
            delta.updated.len(),
            delta.removed.len(),
            skipped
        ),
    })
    .await?;
    tx.commit().await?;
    state.refresh().await?;
    Ok(SyncOutcome { delta, skipped })
}

/// Stores our summary of a failure; a failure to store it is only logged.
async fn record_failure(
    state: &AppState,
    row: &crate::store::McpServerRow,
    actor: &Actor,
    error: &SyncError,
) {
    let summary = error.to_string();
    let result = async {
        let mut tx = state.store.begin().await?;
        if tx.record_sync(row.id, Some(&summary)).await? {
            tx.audit(AuditEntry {
                actor_user_id: actor.user_id,
                actor_email: &actor.email,
                action: "mcp.sync_failed",
                target_type: "mcp_server",
                target_id: Some(row.id),
                summary: &format!("Sync of {} failed: {summary}", row.name),
            })
            .await?;
        }
        tx.commit().await
    }
    .await;
    if let Err(e) = result {
        tracing::warn!(server = %row.name, error = %e, "could not record a failed MCP sync");
    }
}
