//! Deletes request logs and alert events older than the retention setting, in
//! small batches so the tables are never locked for long.

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

/// Deletes every request log older than `cutoff`, `batch` rows at a time
/// with `pause` between the batches.
pub async fn purge(store: &Store, cutoff: &str, batch: i64, pause: Duration) -> Result<Purged> {
    purge_with(cutoff, batch, pause, |cutoff, batch| {
        store.delete_logs_before(cutoff, batch)
    })
    .await
}

/// [`purge`] for the alert events (the history of what fired and resolved).
pub async fn purge_alert_events(
    store: &Store,
    cutoff: &str,
    batch: i64,
    pause: Duration,
) -> Result<Purged> {
    purge_with(cutoff, batch, pause, |cutoff, batch| {
        store.delete_alert_events_before(cutoff, batch)
    })
    .await
}

async fn purge_with<'a, F, Fut>(
    cutoff: &'a str,
    batch: i64,
    pause: Duration,
    delete: F,
) -> Result<Purged>
where
    F: Fn(&'a str, i64) -> Fut,
    Fut: std::future::Future<Output = Result<u64>>,
{
    let batch = batch.max(1);
    let mut purged = Purged {
        rows: 0,
        batches: 0,
    };
    loop {
        let deleted = delete(cutoff, batch).await?;
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

/// One pass by the current setting: the request logs, then the alert
/// events. The result is the request logs'; a failure of either is returned
/// after the other has run.
async fn pass(store: &Store, config: &RetentionConfig) -> Result<Purged> {
    let days = store.log_retention_days().await?;
    let cutoff = after(-days.saturating_mul(86_400));
    let logs = purge(store, &cutoff, config.batch, config.pause).await;
    match purge_alert_events(store, &cutoff, config.batch, config.pause).await {
        Ok(p) if p.rows > 0 => tracing::info!(rows = p.rows, "deleted old alert events"),
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = %e, "could not delete old alert events");
            logs?;
            return Err(e);
        }
    }
    logs
}

/// A pass, then the planner's statistics, which follow the table as it grows
/// and shrinks. A failed optimize is only logged.
async fn pass_and_optimize(store: &Store, config: &RetentionConfig) -> Result<Purged> {
    let done = pass(store, config).await;
    if let Err(e) = store.optimize().await {
        tracing::warn!(error = %e, "could not optimize the database");
    }
    done
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
                done = pass_and_optimize(&store, &config) => done,
            };
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
