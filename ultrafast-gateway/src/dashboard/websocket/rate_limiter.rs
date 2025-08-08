use crate::gateway_error::GatewayError;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct WebSocketRateLimiter {
    limits: Arc<RwLock<HashMap<String, RateLimit>>>,
    config: RateLimitConfig,
}

#[derive(Debug, Clone)]
struct RateLimit {
    tokens: u32,
    last_refill: Instant,
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub messages_per_minute: u32,
    pub burst_size: u32,
    pub refill_interval: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            messages_per_minute: 60,
            burst_size: 10,
            refill_interval: Duration::from_secs(1),
        }
    }
}

impl Default for WebSocketRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSocketRateLimiter {
    pub fn new() -> Self {
        Self {
            limits: Arc::new(RwLock::new(HashMap::new())),
            config: RateLimitConfig::default(),
        }
    }

    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            limits: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    pub async fn check_rate_limit(&self, user_id: &str) -> Result<bool, GatewayError> {
        let mut limits = self.limits.write().await;
        let now = Instant::now();

        let rate_limit = limits
            .entry(user_id.to_string())
            .or_insert_with(|| RateLimit {
                tokens: self.config.burst_size,
                last_refill: now,
            });

        // Refill tokens based on time elapsed
        let time_elapsed = now.duration_since(rate_limit.last_refill);
        if time_elapsed >= self.config.refill_interval {
            let intervals_elapsed = time_elapsed.as_secs() / self.config.refill_interval.as_secs();
            let tokens_to_add = (intervals_elapsed as u32) * (self.config.messages_per_minute / 60);
            rate_limit.tokens =
                std::cmp::min(self.config.burst_size, rate_limit.tokens + tokens_to_add);
            rate_limit.last_refill = now;
        }

        // Check if we have tokens available
        if rate_limit.tokens > 0 {
            rate_limit.tokens -= 1;
            Ok(true)
        } else {
            tracing::warn!("Rate limit exceeded for user: {}", user_id);
            Ok(false)
        }
    }

    pub async fn reset_rate_limit(&self, user_id: &str) -> Result<(), GatewayError> {
        let mut limits = self.limits.write().await;
        limits.remove(user_id);
        Ok(())
    }

    pub async fn get_remaining_tokens(&self, user_id: &str) -> Option<u32> {
        let limits = self.limits.read().await;
        limits.get(user_id).map(|limit| limit.tokens)
    }
}
