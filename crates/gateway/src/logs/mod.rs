//! Request logs: every authenticated `/v1` call is one row of
//! `request_logs`.
//!
//! The proxy hands a [`RequestRecord`] to the [`LogSink`], which puts it on
//! a bounded queue and returns: a request is never delayed by logging. One
//! writer task takes the records off the queue and writes them in batches
//! ([`writer`]); a queue that is full drops the record and counts it
//! ([`LogStats::dropped`]). Old rows are deleted by [`retention`]. Only
//! metadata is stored: no prompt, no answer, no credential.

pub mod retention;
pub mod writer;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;
use ultrafast_translate::types::Usage;

use crate::app::AppState;
use crate::store::NewLog;
use crate::telemetry::{AttemptOutcome, RequestRecord, RequestSink};

/// How many records the queue holds before it drops.
pub const QUEUE_CAPACITY: usize = 10_000;

/// Counters of the log pipeline. They are for `/metrics` and for tests.
#[derive(Debug, Default)]
pub struct LogStats {
    /// Records the queue did not take: it was full or closed.
    pub dropped: AtomicU64,
    /// Records of batches the database refused twice (a write is retried
    /// once).
    pub write_failures: AtomicU64,
    /// Records written.
    pub written: AtomicU64,
    /// Batches written.
    pub batches: AtomicU64,
    /// Micro-dollars of the priced records of the batches, whether or not
    /// the write succeeded: the money was spent either way.
    pub cost_micros: AtomicU64,
}

/// The sink of the gateway: a bounded queue in front of the writer.
pub struct LogSink {
    queue: mpsc::Sender<RequestRecord>,
    stats: Arc<LogStats>,
}

impl LogSink {
    /// A sink and the receiving end of its queue, for the writer.
    pub fn channel(capacity: usize) -> (Self, mpsc::Receiver<RequestRecord>) {
        let (queue, rx) = mpsc::channel(capacity);
        (
            Self {
                queue,
                stats: Arc::default(),
            },
            rx,
        )
    }

    pub fn stats(&self) -> Arc<LogStats> {
        self.stats.clone()
    }
}

impl RequestSink for LogSink {
    /// Never waits: a full or closed queue drops the record and counts it.
    fn record(&self, record: RequestRecord) {
        if self.queue.try_send(record).is_err() {
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// What a model costs per million tokens, in millionths of a dollar;
/// `None` is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price {
    pub input_micros: Option<i64>,
    pub output_micros: Option<i64>,
}

/// The price of `(provider, model)` as of now; `None` when the catalog does
/// not hold the model.
pub type PriceLookup = Arc<dyn Fn(&str, &str) -> Option<Price> + Send + Sync>;

/// Prices read from the snapshot of the state, so the writer never queries
/// the database for them.
pub fn snapshot_prices(state: Arc<AppState>) -> PriceLookup {
    Arc::new(move |provider, model| {
        state.snapshot.load().model(provider, model).map(|m| Price {
            input_micros: m.input_price_micros,
            output_micros: m.output_price_micros,
        })
    })
}

/// The cost of a call in millionths of a dollar, and whether it is known.
///
/// Known (`priced`) means there is usage, the model has a price, and every
/// side that used tokens has a price (an embeddings model has no output
/// price and no output tokens). Otherwise the cost is 0 and `priced` is
/// false. The sum is rounded half up, once.
pub fn cost(usage: Option<Usage>, price: Option<Price>) -> (i64, bool) {
    let (Some(usage), Some(price)) = (usage, price) else {
        return (0, false);
    };
    let sides = [
        (u128::from(usage.input_tokens), price.input_micros),
        (u128::from(usage.output_tokens), price.output_micros),
    ];
    if sides.iter().all(|(tokens, _)| *tokens == 0) {
        return (0, false);
    }
    let mut total: u128 = 0;
    for (tokens, price) in sides {
        match (tokens, price) {
            (0, _) => {}
            (tokens, Some(p)) => total += tokens * u128::from(p.max(0).unsigned_abs()),
            (_, None) => return (0, false),
        }
    }
    let micros = (total + 500_000) / 1_000_000;
    (i64::try_from(micros).unwrap_or(i64::MAX), true)
}

fn outcome_name(outcome: AttemptOutcome) -> &'static str {
    match outcome {
        AttemptOutcome::Ok => "ok",
        AttemptOutcome::Retryable => "retryable",
        AttemptOutcome::Fatal => "fatal",
        AttemptOutcome::CircuitOpen => "circuit_open",
        AttemptOutcome::Skipped => "skipped",
        AttemptOutcome::Cached => "cached",
    }
}

/// The row for a record. The provider and model are those of the attempt
/// that answered (the last one that was `Ok`, or the cached answer of a
/// hit, whose other targets are listed as skipped), else of the last
/// attempt.
pub fn row_of(record: &RequestRecord, prices: &PriceLookup) -> NewLog {
    let answered = record
        .attempts
        .iter()
        .rev()
        .find(|a| matches!(a.outcome, AttemptOutcome::Ok | AttemptOutcome::Cached))
        .or_else(|| record.attempts.last());
    let price = answered.and_then(|a| prices(&a.provider, &a.model));
    // A hit used no provider: it is free, and known to be.
    let (cost_micros, priced) = if record.cached {
        (0, true)
    } else {
        cost(record.usage, price)
    };
    let attempts: Vec<serde_json::Value> = record
        .attempts
        .iter()
        .map(|a| {
            serde_json::json!({
                "provider": a.provider,
                "model": a.model,
                "outcome": outcome_name(a.outcome),
                "status": a.status,
                "duration_ms": a.duration_ms,
            })
        })
        .collect();
    NewLog {
        at: record.started_at.clone(),
        key_id: record.key_id,
        user_id: record.user_id,
        team_id: record.team_id,
        requested: record.requested.clone(),
        endpoint: record.endpoint.to_string(),
        stream: record.stream,
        status: i64::from(record.status),
        provider: answered.map(|a| a.provider.clone()),
        model: answered.map(|a| a.model.clone()),
        input_tokens: record.usage.map(|u| i64::from(u.input_tokens)),
        output_tokens: record.usage.map(|u| i64::from(u.output_tokens)),
        cost_micros,
        priced,
        cached: record.cached,
        estimated: record.estimated,
        duration_ms: i64::try_from(record.duration_ms).unwrap_or(i64::MAX),
        attempts: serde_json::Value::Array(attempts).to_string(),
        tags: crate::tags::to_stored(&record.tags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input_tokens: u32, output_tokens: u32) -> Option<Usage> {
        Some(Usage {
            input_tokens,
            output_tokens,
        })
    }

    fn price(input: Option<i64>, output: Option<i64>) -> Option<Price> {
        Some(Price {
            input_micros: input,
            output_micros: output,
        })
    }

    #[test]
    fn cost_rounds_half_up() {
        // 0.4, 0.5 and 0.6 of a micro-dollar.
        assert_eq!(cost(usage(1, 0), price(Some(400_000), None)), (0, true));
        assert_eq!(cost(usage(1, 0), price(Some(500_000), None)), (1, true));
        assert_eq!(cost(usage(1, 0), price(Some(600_000), None)), (1, true));
        // The sum is rounded once: 0.4 + 0.4 is 0.8, which is 1.
        assert_eq!(
            cost(usage(1, 1), price(Some(400_000), Some(400_000))),
            (1, true)
        );
    }

    #[test]
    fn cost_does_not_overflow() {
        let big = price(Some(i64::MAX), Some(i64::MAX));
        assert_eq!(cost(usage(u32::MAX, u32::MAX), big), (i64::MAX, true));
    }

    #[test]
    fn what_is_not_known_is_not_priced() {
        assert_eq!(cost(None, price(Some(1), Some(1))), (0, false));
        assert_eq!(cost(usage(5, 5), None), (0, false));
        assert_eq!(cost(usage(5, 5), price(None, None)), (0, false));
        assert_eq!(cost(usage(0, 0), price(Some(1), Some(1))), (0, false));
        assert_eq!(cost(usage(5, 5), price(Some(1), None)), (0, false));
        assert_eq!(cost(usage(5, 0), price(Some(1_000_000), None)), (5, true));
    }

    #[test]
    fn an_estimated_record_is_priced_like_a_reported_one_and_marked() {
        use crate::telemetry::{Attempt, AttemptOutcome};
        let record = RequestRecord {
            key_id: Some(1),
            user_id: None,
            team_id: None,
            requested: "p/m".into(),
            endpoint: "chat",
            stream: true,
            status: 499,
            usage: usage(1_000, 500),
            attempts: vec![Attempt {
                provider: "p".into(),
                model: "m".into(),
                outcome: AttemptOutcome::Retryable,
                status: Some(200),
                duration_ms: 1,
                offset_ms: 0,
            }],
            cached: false,
            estimated: true,
            started_at: "2999-01-01 00:00:00".into(),
            duration_ms: 1,
            tags: Default::default(),
            trace_parent: None,
            provider_kinds: Vec::new(),
        };
        let prices: PriceLookup = Arc::new(|_, _| price(Some(2_000_000), Some(4_000_000)));
        let row = row_of(&record, &prices);
        assert!(row.estimated);
        assert!(row.priced);
        assert_eq!(row.cost_micros, 4_000);
        assert_eq!(
            (row.input_tokens, row.output_tokens),
            (Some(1_000), Some(500))
        );
        let reported = RequestRecord {
            estimated: false,
            ..record
        };
        assert!(!row_of(&reported, &prices).estimated);
    }
}
