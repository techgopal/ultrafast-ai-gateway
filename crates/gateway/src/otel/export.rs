//! The queue and the task that sends spans to the collector.
//!
//! `offer` is on the request path: it decides sampling, clones the record
//! and does a non-blocking channel send. The task builds the spans, batches
//! them and posts them. A collector that is slow, down or answering with an
//! error costs spans (counted), never a call: the queue is bounded and a
//! batch that fails is dropped, not retried.

use std::sync::Arc;
use std::time::Duration;

use rand::RngExt;
use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{interval_at, Instant, MissedTickBehavior};

use super::span::{span_count, spans_of};
use crate::metrics::Metrics;
use crate::secrets::fill_random;
use crate::telemetry::RequestRecord;

/// Records waiting to become spans.
pub const QUEUE_CAPACITY: usize = 4096;
/// A batch is sent when it holds this many spans...
const MAX_BATCH_SPANS: usize = 512;
/// ...or this long has passed.
const FLUSH_EVERY: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_CAP: Duration = Duration::from_secs(5);

pub struct OtelConfig {
    /// The OTLP base URL, like `http://localhost:4318`; `/v1/traces` is
    /// added unless the URL already ends with it.
    pub endpoint: String,
    pub headers: Vec<(String, String)>,
    pub service_name: String,
    /// 0.0 to 1.0, decided per trace.
    pub sample_ratio: f64,
}

#[derive(Clone)]
pub struct Exporter {
    tx: mpsc::Sender<RequestRecord>,
    sample_ratio: f64,
    metrics: Arc<Metrics>,
}

impl Exporter {
    /// Starts the task. It ends after `stop` turns true: it sends what is
    /// queued (for at most 5 seconds) and returns.
    pub fn spawn(
        cfg: OtelConfig,
        http: reqwest::Client,
        metrics: Arc<Metrics>,
        stop: watch::Receiver<bool>,
    ) -> (Self, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let exporter = Self {
            tx,
            sample_ratio: cfg.sample_ratio.clamp(0.0, 1.0),
            metrics: metrics.clone(),
        };
        let task = tokio::spawn(run(cfg, http, metrics, rx, stop));
        (exporter, task)
    }

    /// Queues a copy of the call for export. Never blocks. A call that is
    /// not sampled is not queued (and is no loss); a call that finds the
    /// queue full is dropped and counted.
    pub fn offer(&self, record: &RequestRecord) {
        let keep = match &record.trace_parent {
            // The caller decided.
            Some(parent) => parent.sampled,
            None if self.sample_ratio >= 1.0 => true,
            None if self.sample_ratio <= 0.0 => false,
            None => rand::rng().random::<f64>() < self.sample_ratio,
        };
        if !keep {
            return;
        }
        if self.tx.try_send(record.clone()).is_err() {
            self.metrics.otel_dropped(span_count(record));
        }
    }

    /// How many records wait to be exported.
    pub fn queued(&self) -> usize {
        QUEUE_CAPACITY - self.tx.capacity()
    }
}

/// Resolves when `stop` is true or its sender is gone.
async fn stopped(stop: &mut watch::Receiver<bool>) {
    loop {
        if *stop.borrow() || stop.changed().await.is_err() {
            return;
        }
    }
}

/// The URL spans are posted to: the OTLP base URL plus `/v1/traces`, or the
/// URL itself when it already ends so.
pub(crate) fn traces_url(endpoint: &str) -> String {
    let base = endpoint.trim().trim_end_matches('/');
    if base.ends_with("/v1/traces") {
        base.to_string()
    } else {
        format!("{base}/v1/traces")
    }
}

fn random_ids() -> impl FnMut() -> [u8; 8] {
    || loop {
        let mut id = [0u8; 8];
        fill_random(&mut id);
        if id != [0; 8] {
            return id;
        }
    }
}

fn spans_for(record: &RequestRecord, out: &mut Vec<Value>) {
    let trace_id = record.trace_parent.map_or_else(
        || loop {
            let mut id = [0u8; 16];
            fill_random(&mut id);
            if id != [0; 16] {
                return id;
            }
        },
        |p| p.trace_id,
    );
    out.extend(spans_of(record, &mut random_ids(), trace_id));
}

async fn run(
    cfg: OtelConfig,
    http: reqwest::Client,
    metrics: Arc<Metrics>,
    mut rx: mpsc::Receiver<RequestRecord>,
    mut stop: watch::Receiver<bool>,
) {
    let mut cfg = cfg;
    cfg.endpoint = traces_url(&cfg.endpoint);
    let sender = Sender { cfg, http, metrics };
    let mut batch: Vec<Value> = Vec::new();
    let mut tick = interval_at(Instant::now() + FLUSH_EVERY, FLUSH_EVERY);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            record = rx.recv() => match record {
                Some(record) => {
                    spans_for(&record, &mut batch);
                    if batch.len() >= MAX_BATCH_SPANS {
                        sender.send(std::mem::take(&mut batch)).await;
                    }
                }
                None => break,
            },
            _ = tick.tick() => {
                if !batch.is_empty() {
                    sender.send(std::mem::take(&mut batch)).await;
                }
            }
            () = stopped(&mut stop) => break,
        }
    }
    // The last flush: what is queued, within the cap.
    let deadline = Instant::now() + SHUTDOWN_CAP;
    while let Ok(record) = rx.try_recv() {
        spans_for(&record, &mut batch);
    }
    while !batch.is_empty() {
        let rest = batch.split_off(batch.len().min(MAX_BATCH_SPANS));
        let chunk = std::mem::replace(&mut batch, rest);
        let n = chunk.len() as u64;
        let left = deadline.saturating_duration_since(Instant::now());
        if tokio::time::timeout(left, sender.send(chunk))
            .await
            .is_err()
        {
            sender.metrics.otel_failure();
            sender.metrics.otel_dropped(n + batch.len() as u64);
            return;
        }
    }
}

struct Sender {
    cfg: OtelConfig,
    http: reqwest::Client,
    metrics: Arc<Metrics>,
}

impl Sender {
    /// Posts one batch. Whatever the answer, the batch is gone afterwards.
    async fn send(&self, spans: Vec<Value>) {
        let n = spans.len() as u64;
        let body = json!({ "resourceSpans": [{
            "resource": { "attributes": [
                { "key": "service.name", "value": { "stringValue": self.cfg.service_name } },
                { "key": "service.version", "value": { "stringValue": env!("CARGO_PKG_VERSION") } },
            ] },
            "scopeSpans": [{ "scope": { "name": "ultrafast" }, "spans": spans }],
        }] });
        let mut request = self
            .http
            .post(&self.cfg.endpoint)
            .timeout(REQUEST_TIMEOUT)
            .json(&body);
        for (name, value) in &self.cfg.headers {
            request = request.header(name, value);
        }
        match request.send().await {
            Ok(resp) if resp.status().is_success() => self.metrics.otel_exported(n),
            outcome => {
                // The status only: neither the URL nor a header is logged.
                tracing::warn!(
                    status = outcome.as_ref().ok().map(|r| r.status().as_u16()),
                    "trace export failed; the batch is dropped"
                );
                self.metrics.otel_failure();
                self.metrics.otel_dropped(n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::traces_url;

    #[test]
    fn the_traces_path_is_added_once() {
        assert_eq!(traces_url("http://h:4318"), "http://h:4318/v1/traces");
        assert_eq!(traces_url("http://h:4318/"), "http://h:4318/v1/traces");
        assert_eq!(traces_url("http://h/v1/traces"), "http://h/v1/traces");
        assert_eq!(traces_url("http://h/p/"), "http://h/p/v1/traces");
    }
}
