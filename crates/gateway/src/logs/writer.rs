//! The task that writes the queue of records to `request_logs`.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::{row_of, LogStats, PriceLookup};
use crate::store::Store;
use crate::telemetry::RequestRecord;

/// When a batch is written: it holds this many records, or this long has
/// passed since its first record.
#[derive(Debug, Clone, Copy)]
pub struct WriterConfig {
    pub max_batch: usize,
    pub max_wait: Duration,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            max_batch: 500,
            max_wait: Duration::from_secs(1),
        }
    }
}

/// Resolves when `stop` is true or its sender is gone.
async fn stopped(stop: &mut watch::Receiver<bool>) {
    loop {
        if *stop.borrow() {
            return;
        }
        if stop.changed().await.is_err() {
            return;
        }
    }
}

async fn flush(store: &Store, prices: &PriceLookup, stats: &LogStats, batch: &[RequestRecord]) {
    if batch.is_empty() {
        return;
    }
    let rows: Vec<_> = batch.iter().map(|r| row_of(r, prices)).collect();
    match store.insert_logs(&rows).await {
        Ok(()) => {
            stats
                .written
                .fetch_add(rows.len() as u64, Ordering::Relaxed);
            stats.batches.fetch_add(1, Ordering::Relaxed);
        }
        Err(e) => {
            // The batch is lost; the calls it describes were answered.
            stats
                .dropped
                .fetch_add(rows.len() as u64, Ordering::Relaxed);
            tracing::error!(error = %e, lost = rows.len(), "could not write request logs");
        }
    }
}

/// Writes records in batches of at most `max_batch`, one transaction each.
/// When `stop` becomes true (or its sender is dropped) the queue is closed,
/// what is in it is written, and the task ends.
pub fn spawn(
    store: Store,
    mut queue: mpsc::Receiver<RequestRecord>,
    prices: PriceLookup,
    stats: Arc<LogStats>,
    config: WriterConfig,
    mut stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let max_batch = config.max_batch.max(1);
    tokio::spawn(async move {
        let mut running = true;
        while running {
            let first = tokio::select! {
                biased;
                () = stopped(&mut stop) => break,
                next = queue.recv() => match next {
                    Some(record) => record,
                    None => break,
                },
            };
            let mut batch = vec![first];
            let deadline = Instant::now() + config.max_wait;
            while batch.len() < max_batch {
                tokio::select! {
                    biased;
                    () = stopped(&mut stop) => { running = false; break }
                    next = tokio::time::timeout_at(deadline, queue.recv()) => match next {
                        Ok(Some(record)) => batch.push(record),
                        // The wait is over, or the queue is closed.
                        Ok(None) => { running = false; break }
                        Err(_) => break,
                    },
                }
            }
            flush(&store, &prices, &stats, &batch).await;
        }
        // Drain: nothing new is accepted, what is queued is written.
        queue.close();
        loop {
            let mut batch = Vec::new();
            while batch.len() < max_batch {
                match queue.try_recv() {
                    Ok(record) => batch.push(record),
                    Err(_) => break,
                }
            }
            if batch.is_empty() {
                return;
            }
            flush(&store, &prices, &stats, &batch).await;
        }
    })
}
