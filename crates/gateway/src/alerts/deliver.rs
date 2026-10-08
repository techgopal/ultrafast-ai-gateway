//! Delivery of stored alert events to channels.
//!
//! [`Deliverer::offer`] is a non-blocking send into a bounded queue; a full
//! queue drops the job, counts it and records a `dropped` outcome on the
//! event. One task takes jobs off the queue one at a time: it reads the event
//! and the channels (two quick reads) and starts a task per channel, so the
//! queue drains no faster than that and a flood backs up into it, where it is
//! dropped and recorded. Each channel has its own gate: at most
//! [`CHANNEL_CONCURRENCY`] deliveries to it are in progress (retries
//! included) and at most [`CHANNEL_BACKLOG`] more wait; the rest are
//! recorded as dropped. So a host that is down fills only its own gate. The
//! only shared limit is on HTTP sends in flight, and a delivery holds one of
//! those only while a request is out, not while it waits to retry. Each
//! channel's outcome is written to the event as soon as that channel is
//! done, so a healthy channel is not hidden behind a dead one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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
/// Deliveries to one channel in progress at the same moment.
pub const CHANNEL_CONCURRENCY: usize = 4;
/// Deliveries to one channel that wait for a slot; more are dropped.
pub const CHANNEL_BACKLOG: usize = 256;
/// Deliveries started and not finished, over all channels. At this many the
/// dispatcher stops taking jobs until one finishes, so a flood backs up into
/// the queue, which drops what does not fit.
pub const MAX_PENDING: usize = 1024;
/// HTTP requests out at the same moment, over all channels.
const SENDS_IN_FLIGHT: usize = 64;

#[derive(Clone, Debug)]
pub struct DeliveryConfig {
    /// The waits before the second and the third try.
    pub retry_delays: Vec<Duration>,
    /// For each try.
    pub timeout: Duration,
    /// How long the task waits for deliveries in progress once told to stop.
    pub shutdown_cap: Duration,
    /// Jobs that may wait for the dispatcher.
    pub queue_capacity: usize,
}

impl Default for DeliveryConfig {
    /// Three tries: at once, after 5 seconds and after 30 seconds, each
    /// with 10 seconds to answer.
    fn default() -> Self {
        Self {
            retry_delays: vec![Duration::from_secs(5), Duration::from_secs(30)],
            timeout: TRY_TIMEOUT,
            shutdown_cap: Duration::from_secs(5),
            queue_capacity: QUEUE_CAPACITY,
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
    store: Store,
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

impl Delivery {
    fn failed(channel: &ChannelRow, tries: u32, error: &str) -> Self {
        Self {
            channel_id: channel.id,
            channel_name: channel.name.clone(),
            ok: false,
            status: None,
            tries,
            error: Some(error.to_string()),
        }
    }
}

/// Limits on deliveries to one channel.
struct Gate {
    slots: Semaphore,
    waiting: AtomicUsize,
}

#[derive(Default)]
struct Gates {
    map: Mutex<HashMap<i64, Arc<Gate>>>,
}

impl Gates {
    fn of(&self, channel: i64) -> Arc<Gate> {
        let mut map = self
            .map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Gates nobody uses any more are forgotten.
        if map.len() >= 1024 {
            map.retain(|_, g| Arc::strong_count(g) > 1);
        }
        map.entry(channel)
            .or_insert_with(|| {
                Arc::new(Gate {
                    slots: Semaphore::new(CHANNEL_CONCURRENCY),
                    waiting: AtomicUsize::new(0),
                })
            })
            .clone()
    }
}

#[derive(Clone)]
struct Context {
    gates: Arc<Gates>,
    sends: Arc<Semaphore>,
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
        let (tx, rx) = mpsc::channel(cfg.queue_capacity.max(1));
        let deliverer = Self {
            tx,
            metrics: metrics.clone(),
            store: store.clone(),
        };
        let ctx = Context {
            gates: Arc::default(),
            sends: Arc::new(Semaphore::new(SENDS_IN_FLIGHT)),
            store,
            cipher,
            http,
            metrics,
            cfg,
        };
        (deliverer, tokio::spawn(run(ctx, rx, stop)))
    }

    /// Queues the delivery of a stored event to these channels. Never
    /// blocks. A full queue drops the job: each channel is counted as
    /// dropped and the event is given a `dropped` outcome for it.
    pub fn offer(&self, event_id: i64, channel_ids: Vec<i64>) {
        let job = Job {
            event_id,
            channel_ids,
        };
        if let Err(e) = self.tx.try_send(job) {
            let job = e.into_inner();
            self.metrics
                .alert_delivery("dropped", job.channel_ids.len() as u64);
            // The write is a task of its own: this call must not wait. With
            // no runtime there is nobody to write it.
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                let store = self.store.clone();
                runtime.spawn(async move { record_dropped(&store, job).await });
            }
        }
    }

    /// How many jobs wait for the dispatcher.
    pub fn queued(&self) -> usize {
        self.tx.max_capacity() - self.tx.capacity()
    }
}

const DROPPED_QUEUE: &str = "dropped: the delivery queue was full";
const DROPPED_BACKLOG: &str = "dropped: too many deliveries were waiting for this channel";

/// Stores a `dropped` outcome for each channel of a job that was not run.
async fn record_dropped(store: &Store, job: Job) {
    let Ok(channels) = store.list_alert_channels().await else {
        return;
    };
    let mut seen = Vec::new();
    let mut deliveries = Vec::new();
    for id in job.channel_ids {
        let Some(channel) = channels.iter().find(|c| c.id == id) else {
            continue;
        };
        if !seen.contains(&id) {
            seen.push(id);
            deliveries.push(Delivery::failed(channel, 0, DROPPED_QUEUE));
        }
    }
    if deliveries.is_empty() {
        return;
    }
    let json = serde_json::to_string(&deliveries).expect("deliveries serialize");
    if let Err(e) = store.set_alert_event_deliveries(job.event_id, &json).await {
        tracing::warn!(error = %e, "could not store a dropped alert delivery");
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
    let mut sends: JoinSet<()> = JoinSet::new();
    loop {
        let job = tokio::select! {
            job = rx.recv() => match job {
                Some(job) => job,
                None => break,
            },
            () = stopped(&mut stop) => break,
        };
        start(&ctx, job, &mut sends).await;
        while sends.try_join_next().is_some() {}
        // Too many in progress: take no more jobs until some finish.
        while sends.len() >= MAX_PENDING {
            tokio::select! {
                _ = sends.join_next() => {}
                () = stopped(&mut stop) => break,
            }
        }
    }
    // What is queued still goes out, within the cap.
    while let Ok(job) = rx.try_recv() {
        start(&ctx, job, &mut sends).await;
    }
    let drained = tokio::time::timeout(ctx.cfg.shutdown_cap, async {
        while sends.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        tracing::warn!(
            unfinished = sends.len(),
            "alert deliveries were cut short by shutdown"
        );
        sends.abort_all();
        while sends.join_next().await.is_some() {}
    }
}

/// The outcomes of one event so far, by the position of the channel in the
/// job. Whoever finishes adds its own and stores the list.
struct Outcomes {
    event_id: i64,
    done: tokio::sync::Mutex<Vec<(usize, Delivery)>>,
}

/// Reads an event and starts the delivery to each of its channels. A channel
/// that was deleted since the event was queued is left out of the outcomes.
async fn start(ctx: &Context, job: Job, sends: &mut JoinSet<()>) {
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
    let outcomes = Arc::new(Outcomes {
        event_id: event.id,
        done: tokio::sync::Mutex::new(Vec::new()),
    });
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
        let (ctx, event, outcomes, position) =
            (ctx.clone(), event.clone(), outcomes.clone(), seen.len() - 1);
        sends.spawn(async move {
            let delivery = deliver(&ctx, &channel, &event).await;
            let mut done = outcomes.done.lock().await;
            done.push((position, delivery));
            done.sort_by_key(|(position, _)| *position);
            let list: Vec<&Delivery> = done.iter().map(|(_, d)| d).collect();
            let json = serde_json::to_string(&list).expect("deliveries serialize");
            if let Err(e) = ctx
                .store
                .set_alert_event_deliveries(outcomes.event_id, &json)
                .await
            {
                tracing::warn!(error = %e, "could not store the outcome of an alert delivery");
            }
        });
    }
}

/// Up to three tries to one channel, inside the channel's gate.
async fn deliver(ctx: &Context, channel: &ChannelRow, event: &AlertEventRow) -> Delivery {
    if !channel.enabled {
        return Delivery::failed(channel, 0, "the channel is disabled");
    }
    let (Some(url), Some(secret)) = (
        decrypted(&ctx.cipher, &channel.url_enc),
        decrypted(&ctx.cipher, &channel.secret_enc),
    ) else {
        ctx.metrics.alert_delivery("failed", 1);
        return Delivery::failed(
            channel,
            0,
            "the URL or the secret of the channel could not be read",
        );
    };
    // The channel's own gate: a host that is down fills this and nothing else.
    let gate = ctx.gates.of(channel.id);
    if gate.waiting.fetch_add(1, Ordering::AcqRel) >= CHANNEL_BACKLOG {
        gate.waiting.fetch_sub(1, Ordering::AcqRel);
        ctx.metrics.alert_delivery("dropped", 1);
        return Delivery::failed(channel, 0, DROPPED_BACKLOG);
    }
    let slot = gate.slots.acquire().await;
    gate.waiting.fetch_sub(1, Ordering::AcqRel);
    let _slot = slot.expect("the semaphore is never closed");

    let body = payload(&channel.kind, event);
    let mut tries = 0;
    let mut last;
    loop {
        tries += 1;
        // Held for the request only, not for the wait before the next try.
        let send = ctx
            .sends
            .acquire()
            .await
            .expect("the semaphore is never closed");
        last = send_once(&ctx.http, &url, &secret, &body, ctx.cfg.timeout).await;
        drop(send);
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
    Delivery {
        channel_id: channel.id,
        channel_name: channel.name.clone(),
        ok: last.ok,
        status: last.status,
        tries,
        error: last.error,
    }
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
