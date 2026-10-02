//! Deletes request logs older than the retention setting, in small batches
//! so the table is never locked for long.

use std::time::Duration;

use anyhow::Result;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::store::{after, Store};

#[derive(Debug, Clone, Copy)]
pub struct RetentionConfig {
    /// Between two passes.
    pub interval: Duration,
    /// Rows deleted by one statement.
    pub batch: i64,
    /// Between two batches of one pass.
    pub pause: Duration,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(3600),
            batch: 1_000,
            pause: Duration::from_millis(100),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Purged {
    pub rows: u64,
    pub batches: u64,
}

/// Deletes every row older than `cutoff`, `batch` rows at a time with
/// `pause` between the batches.
pub async fn purge(store: &Store, cutoff: &str, batch: i64, pause: Duration) -> Result<Purged> {
    let batch = batch.max(1);
    let mut purged = Purged {
        rows: 0,
        batches: 0,
    };
    loop {
        let deleted = store.delete_logs_before(cutoff, batch).await?;
        if deleted == 0 {
            return Ok(purged);
        }
        purged.rows += deleted;
        purged.batches += 1;
        if deleted < u64::try_from(batch).unwrap_or(u64::MAX) {
            return Ok(purged);
        }
        tokio::time::sleep(pause).await;
    }
}

/// One pass by the current setting.
async fn pass(store: &Store, config: &RetentionConfig) -> Result<Purged> {
    let days = store.log_retention_days().await?;
    purge(
        store,
        &after(-days.saturating_mul(86_400)),
        config.batch,
        config.pause,
    )
    .await
}

/// Resolves when `stop` is true or its sender is gone.
async fn stopped(stop: &mut watch::Receiver<bool>) {
    loop {
        if *stop.borrow() || stop.changed().await.is_err() {
            return;
        }
    }
}

/// Runs a pass at the start and then every `interval`, until `stop`
/// becomes true or its sender is dropped. A failed pass is logged.
pub fn spawn(
    store: Store,
    config: RetentionConfig,
    mut stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            // A pass can be long; shutdown does not wait for it. Dropping it
            // between statements loses nothing: the next start continues.
            let done = tokio::select! {
                biased;
                () = stopped(&mut stop) => return,
                done = pass(&store, &config) => done,
            };
            // Statistics follow the table as it grows and shrinks.
            if let Err(e) = store.optimize().await {
                tracing::warn!(error = %e, "could not optimize the database");
            }
            match done {
                Ok(p) if p.rows > 0 => tracing::info!(rows = p.rows, "deleted old request logs"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "could not delete old request logs"),
            }
            tokio::select! {
                () = tokio::time::sleep(config.interval) => {}
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        return;
                    }
                }
            }
        }
    })
}
