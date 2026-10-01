//! The routing engine: which targets a call tries and in what order,
//! retries with backoff, timeouts and the circuit breaker of each target.

pub mod breaker;
pub mod select;

use std::time::Duration;

pub use breaker::BreakerSettings;
pub use select::plan;

/// A model of a provider, by name, with the id of the catalog row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TargetRef {
    pub provider: String,
    pub model: String,
    pub model_id: i64,
}

/// How a call is tried: used for a route, and by default for a direct call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Extra tries of a target after the first, on a retryable failure.
    pub retries: u32,
    /// How long until the first byte (a stream: the first event).
    pub first_token_timeout: Duration,
    /// How long the whole request may take, every try included.
    pub total_timeout: Duration,
    pub breaker: BreakerSettings,
}

impl Settings {
    /// What a direct `provider/model` call uses.
    pub const DIRECT: Settings = Settings {
        retries: 2,
        first_token_timeout: Duration::from_secs(30),
        total_timeout: Duration::from_secs(300),
        breaker: BreakerSettings::DEFAULT,
    };
}
