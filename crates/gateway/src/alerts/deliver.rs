//! Delivery of stored alert events to channels.
//!
//! [`Deliverer::offer`] is a non-blocking send into a bounded queue; a full
//! queue drops the job and counts it. One task takes jobs off the queue (at
//! most [`MAX_JOBS_IN_FLIGHT`] at a time, so a burst waits in the queue and
//! not in memory), and each job sends to its channels from one task per
//! channel: a host that is down delays only its own retries. When every
//! channel of a job has finished, the outcomes are written to the event in
//! one update.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{mpsc, watch, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

use super::payload;
use super::sign::{header_value, SIGNATURE_HEADER};
use crate::metrics::Metrics;
use crate::secrets::Cipher;
use crate::store::{AlertEventRow, ChannelRow, Store};

/// Jobs waiting for a free slot.
pub const QUEUE_CAPACITY: usize = 1024;
/// How long one try may take.
pub const TRY_TIMEOUT: Duration = Duration::from_secs(10);
/// Jobs being delivered at the same moment.
const MAX_JOBS_IN_FLIGHT: usize = 64;

#[derive(Clone, Debug)]
pub struct DeliveryConfig {
    /// The waits before the second and the third try.
    pub retry_delays: Vec<Duration>,
    /// For each try.
    pub timeout: Duration,
    /// How long the task waits for deliveries in progress once told to stop.
    pub shutdown_cap: Duration,
}

impl Default for DeliveryConfig {
    /// Three tries: at once, after 5 seconds and after 30 seconds, each
    /// with 10 seconds to answer.
    fn default() -> Self {
        Self {
            retry_delays: vec![Duration::from_secs(5), Duration::from_secs(30)],
            timeout: TRY_TIMEOUT,
            shutdown_cap: Duration::from_secs(5),
        }
    }
}

struct Job {
    event_id: i64,
    channel_ids: Vec<i64>,
}

#[derive(Clone)]
pub struct Deliverer {
    tx: mpsc::Sender<Job>,
    metrics: Arc<Metrics>,
}

/// One post to a channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub ok: bool,
    /// The HTTP status, when the receiver answered.
    pub status: Option<u16>,
    /// Why it failed. Never holds the URL.
    pub error: Option<String>,
}

/// What is stored on the event for one channel.
#[derive(Serialize)]
struct Delivery {
    channel_id: i64,
    channel_name: String,
    ok: bool,
    status: Option<u16>,
    tries: u32,
    error: Option<String>,
}

#[derive(Clone)]
struct Context {
    store: Store,
    cipher: Cipher,
    http: reqwest::Client,
    metrics: Arc<Metrics>,
    cfg: DeliveryConfig,
}

impl Deliverer {
    /// Starts the task. It ends after `stop` turns true: it takes what is
    /// queued, waits up to `shutdown_cap` for deliveries in progress and
    /// returns.
    pub fn spawn(
        store: Store,
        cipher: Cipher,
        http: reqwest::Client,
        metrics: Arc<Metrics>,
        cfg: DeliveryConfig,
        stop: watch::Receiver<bool>,
    ) -> (Self, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let deliverer = Self {
            tx,
            metrics: metrics.clone(),
        };
        let ctx = Context {
            store,
            cipher,
            http,
            metrics,
            cfg,
        };
        (deliverer, tokio::spawn(run(ctx, rx, stop)))
    }

    /// Queues the delivery of a stored event to these channels. Never
    /// blocks; a full queue drops the job and counts each channel as dropped.
    pub fn offer(&self, event_id: i64, channel_ids: Vec<i64>) {
        let n = channel_ids.len() as u64;
        if self
            .tx
            .try_send(Job {
                event_id,
                channel_ids,
            })
            .is_err()
        {
            self.metrics.alert_delivery("dropped", n);
        }
    }

    /// How many jobs wait for a slot.
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

async fn run(ctx: Context, mut rx: mpsc::Receiver<Job>, mut stop: watch::Receiver<bool>) {
    let slots = Arc::new(Semaphore::new(MAX_JOBS_IN_FLIGHT));
    let mut jobs: JoinSet<()> = JoinSet::new();
    loop {
        // A slot first: while all are busy the jobs wait in the queue,
        // which is bounded.
        let permit = tokio::select! {
            permit = slots.clone().acquire_owned() => permit.expect("the semaphore is never closed"),
            () = stopped(&mut stop) => break,
        };
        let job = tokio::select! {
            job = rx.recv() => match job {
                Some(job) => job,
                None => break,
            },
            () = stopped(&mut stop) => break,
        };
        let ctx = ctx.clone();
        jobs.spawn(async move {
            process(&ctx, job).await;
            drop(permit);
        });
        while jobs.try_join_next().is_some() {}
    }
    // What is queued still goes out, within the cap.
    while let Ok(job) = rx.try_recv() {
        let ctx = ctx.clone();
        jobs.spawn(async move { process(&ctx, job).await });
    }
    let drained = tokio::time::timeout(ctx.cfg.shutdown_cap, async {
        while jobs.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        tracing::warn!(
            unfinished = jobs.len(),
            "alert deliveries were cut short by shutdown"
        );
        jobs.abort_all();
        while jobs.join_next().await.is_some() {}
    }
}

/// Sends one event to its channels and stores what happened.
async fn process(ctx: &Context, job: Job) {
    let event = match ctx.store.alert_event(job.event_id).await {
        Ok(Some(event)) => event,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(error = %e, "could not read an alert event to deliver");
            return;
        }
    };
    let channels = match ctx.store.list_alert_channels().await {
        Ok(channels) => channels,
        Err(e) => {
            tracing::warn!(error = %e, "could not read the alert channels to deliver to");
            return;
        }
    };
    let event = Arc::new(event);
    let mut sends: JoinSet<(usize, Delivery)> = JoinSet::new();
    let mut seen = Vec::new();
    for id in job.channel_ids {
        // A channel deleted since is not delivered to; one named twice is
        // delivered to once.
        let Some(channel) = channels.iter().find(|c| c.id == id).cloned() else {
            continue;
        };
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let (ctx, event, position) = (ctx.clone(), event.clone(), seen.len() - 1);
        sends.spawn(async move { (position, deliver(&ctx, &channel, &event).await) });
    }
    let mut results = Vec::new();
    while let Some(done) = sends.join_next().await {
        match done {
            Ok(result) => results.push(result),
            Err(e) => tracing::warn!(error = %e, "an alert delivery task failed"),
        }
    }
    if results.is_empty() {
        return;
    }
    results.sort_by_key(|(position, _)| *position);
    let deliveries: Vec<Delivery> = results.into_iter().map(|(_, d)| d).collect();
    let json = serde_json::to_string(&deliveries).expect("deliveries serialize");
    if let Err(e) = ctx.store.set_alert_event_deliveries(event.id, &json).await {
        tracing::warn!(error = %e, "could not store the outcome of an alert delivery");
    }
}

/// Up to three tries to one channel.
async fn deliver(ctx: &Context, channel: &ChannelRow, event: &AlertEventRow) -> Delivery {
    let delivery = |ok, status, tries, error: Option<&str>| Delivery {
        channel_id: channel.id,
        channel_name: channel.name.clone(),
        ok,
        status,
        tries,
        error: error.map(str::to_string),
    };
    if !channel.enabled {
        return delivery(false, None, 0, Some("the channel is disabled"));
    }
    let (Some(url), Some(secret)) = (
        decrypted(&ctx.cipher, &channel.url_enc),
        decrypted(&ctx.cipher, &channel.secret_enc),
    ) else {
        ctx.metrics.alert_delivery("failed", 1);
        return delivery(
            false,
            None,
            0,
            Some("the URL or the secret of the channel could not be read"),
        );
    };
    let body = payload(&channel.kind, event);
    let mut tries = 0;
    let mut last;
    loop {
        tries += 1;
        last = send_once(&ctx.http, &url, &secret, &body, ctx.cfg.timeout).await;
        if last.ok {
            break;
        }
        match ctx.cfg.retry_delays.get(tries as usize - 1) {
            Some(wait) => tokio::time::sleep(*wait).await,
            None => break,
        }
    }
    ctx.metrics
        .alert_delivery(if last.ok { "ok" } else { "failed" }, 1);
    delivery(last.ok, last.status, tries, last.error.as_deref())
}

fn decrypted(cipher: &Cipher, bytes: &[u8]) -> Option<String> {
    String::from_utf8(cipher.decrypt(bytes).ok()?).ok()
}

/// One signed post. A 2xx answer is success; so is nothing else (redirects
/// are not followed). The error says what went wrong, never where.
pub async fn send_once(
    http: &reqwest::Client,
    url: &str,
    secret: &str,
    body: &[u8],
    timeout: Duration,
) -> Attempt {
    let t = time::OffsetDateTime::now_utc().unix_timestamp();
    let request = http
        .post(url)
        .timeout(timeout)
        .header("content-type", "application/json")
        .header(
            "user-agent",
            concat!("ultrafast/", env!("CARGO_PKG_VERSION")),
        )
        .header(SIGNATURE_HEADER, header_value(secret, t, body))
        .body(body.to_vec());
    match request.send().await {
        Ok(response) => {
            let status = response.status();
            Attempt {
                ok: status.is_success(),
                status: Some(status.as_u16()),
                error: (!status.is_success())
                    .then(|| format!("the receiver answered {}", status.as_u16())),
            }
        }
        Err(e) => Attempt {
            ok: false,
            status: None,
            error: Some(
                if e.is_timeout() {
                    "the receiver did not answer in time"
                } else if e.is_connect() {
                    "could not connect to the receiver"
                } else {
                    "the request could not be sent"
                }
                .to_string(),
            ),
        },
    }
}
