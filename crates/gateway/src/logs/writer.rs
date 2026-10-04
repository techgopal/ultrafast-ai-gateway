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
    /// How long to wait before the one retry of a failed write.
    pub retry_delay: Duration,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            max_batch: 500,
            max_wait: Duration::from_secs(1),
            retry_delay: Duration::from_millis(200),
        }
    }
}

/// Told the cost, in micro-dollars, of every record that was priced, once
/// per record and before it is written: the budgets count it. It runs in
/// the writer task, off the request path, and must not block.
pub type Accountant = Arc<dyn Fn(&RequestRecord, u64) + Send + Sync>;

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

async fn flush(
    store: &Store,
    prices: &PriceLookup,
    stats: &LogStats,
    retry_delay: Duration,
    account: &Accountant,
    batch: &[RequestRecord],
) {
    if batch.is_empty() {
        return;
    }
    let rows: Vec<_> = batch.iter().map(|r| row_of(r, prices)).collect();
    // Once per record, whether or not the write below succeeds: the money
    // was spent either way.
    for (record, row) in batch.iter().zip(&rows) {
        if row.priced && row.cost_micros > 0 {
            let micros = u64::try_from(row.cost_micros).unwrap_or(0);
            stats.cost_micros.fetch_add(micros, Ordering::Relaxed);
            account(record, micros);
        }
    }
    // One retry with the same rows: a busy database is usually free again.
    let mut result = store.insert_logs(&rows).await;
    if let Err(e) = &result {
        tracing::warn!(error = %e, "could not write request logs, trying once more");
        tokio::time::sleep(retry_delay).await;
        result = store.insert_logs(&rows).await;
    }
    match result {
        Ok(()) => {
            stats
                .written
                .fetch_add(rows.len() as u64, Ordering::Relaxed);
            stats.batches.fetch_add(1, Ordering::Relaxed);
        }
        Err(e) => {
            // The batch is lost; the calls it describes were answered.
            stats
                .write_failures
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
    queue: mpsc::Receiver<RequestRecord>,
    prices: PriceLookup,
    stats: Arc<LogStats>,
    config: WriterConfig,
    stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    spawn_accounted(
        store,
        queue,
        prices,
        stats,
        config,
        stop,
        Arc::new(|_, _| {}),
    )
}

/// Like [`spawn`], and tells `account` the cost of every priced record.
pub fn spawn_accounted(
    store: Store,
    mut queue: mpsc::Receiver<RequestRecord>,
    prices: PriceLookup,
    stats: Arc<LogStats>,
    config: WriterConfig,
    mut stop: watch::Receiver<bool>,
    account: Accountant,
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
            flush(
                &store,
                &prices,
                &stats,
                config.retry_delay,
                &account,
                &batch,
            )
            .await;
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
            flush(
                &store,
                &prices,
                &stats,
                config.retry_delay,
                &account,
                &batch,
            )
            .await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::LogSink;
    use crate::telemetry::RequestSink;

    fn record() -> RequestRecord {
        RequestRecord {
            key_id: Some(1),
            user_id: None,
            team_id: None,
            requested: "r".into(),
            endpoint: "chat",
            stream: false,
            status: 200,
            usage: None,
            attempts: Vec::new(),
            cached: false,
            estimated: false,
            started_at: "2999-01-01 00:00:00".into(),
            duration_ms: 1,
        }
    }

    async fn rename(store: &Store, from: &str, to: &str) {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {from} RENAME TO {to}"
        )))
        .execute(store.pool())
        .await
        .unwrap();
    }

    fn start(store: &Store, retry_ms: u64) -> (LogSink, watch::Sender<bool>, JoinHandle<()>) {
        let (sink, rx) = LogSink::channel(10);
        let (stop, stopped) = watch::channel(false);
        let writer = spawn(
            store.clone(),
            rx,
            Arc::new(|_, _| None),
            sink.stats(),
            WriterConfig {
                max_batch: 10,
                max_wait: Duration::from_millis(10),
                retry_delay: Duration::from_millis(retry_ms),
            },
            stopped,
        );
        (sink, stop, writer)
    }

    #[tokio::test]
    async fn a_failed_write_is_retried_once_with_the_same_rows() {
        let store = Store::open_in_memory().await.unwrap();
        rename(&store, "request_logs", "away").await;
        let (sink, stop, writer) = start(&store, 400);
        sink.record(record());
        // The first try fails; the table is back before the retry.
        tokio::time::sleep(Duration::from_millis(150)).await;
        rename(&store, "away", "request_logs").await;
        tokio::time::sleep(Duration::from_millis(600)).await;
        stop.send(true).unwrap();
        writer.await.unwrap();
        let stats = sink.stats();
        assert_eq!(stats.written.load(Ordering::Relaxed), 1);
        assert_eq!(stats.write_failures.load(Ordering::Relaxed), 0);
        assert_eq!(stats.dropped.load(Ordering::Relaxed), 0);
        assert_eq!(store.recent_logs(10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_second_failure_is_counted_apart_from_queue_drops() {
        let store = Store::open_in_memory().await.unwrap();
        rename(&store, "request_logs", "away").await;
        let (sink, stop, writer) = start(&store, 20);
        sink.record(record());
        tokio::time::sleep(Duration::from_millis(300)).await;
        stop.send(true).unwrap();
        writer.await.unwrap();
        let stats = sink.stats();
        assert_eq!(stats.write_failures.load(Ordering::Relaxed), 1);
        assert_eq!(stats.dropped.load(Ordering::Relaxed), 0);
        assert_eq!(stats.written.load(Ordering::Relaxed), 0);
    }
}
