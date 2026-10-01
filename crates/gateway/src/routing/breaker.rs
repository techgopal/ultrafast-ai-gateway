//! The circuit breaker of one target.

use std::time::Duration;

/// When a target is cut off and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakerSettings {
    /// Retryable failures within `window` that open the breaker.
    pub failures: u32,
    pub window: Duration,
    /// How long it stays open before one trial call is let through.
    pub open: Duration,
}

impl BreakerSettings {
    /// 5 failures in 60 s, open for 30 s.
    pub const DEFAULT: BreakerSettings = BreakerSettings {
        failures: 5,
        window: Duration::from_secs(60),
        open: Duration::from_secs(30),
    };
}
