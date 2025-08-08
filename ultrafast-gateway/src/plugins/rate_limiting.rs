// DEPRECATED: Rate limiting is now handled by the centralized auth module
// This plugin remains for backwards compatibility but should not be used
// Use the auth middleware with rate_limiting configuration instead

use crate::config::PluginConfig;
use crate::gateway_error::GatewayError;
use axum::body::Body;
use axum::http::{Request, Response};
use dashmap::DashMap;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct RateLimitingPlugin {
    name: String,
    enabled: bool,
    limits: HashMap<String, u32>, // requests per minute
    counters: DashMap<String, RateLimitCounter>,
}

#[derive(Clone, Debug)]
struct RateLimitCounter {
    requests: u32,
    window_start: Instant,
}

impl RateLimitCounter {
    fn new() -> Self {
        Self {
            requests: 0,
            window_start: Instant::now(),
        }
    }

    fn is_expired(&self) -> bool {
        self.window_start.elapsed() >= Duration::from_secs(60)
    }

    fn reset(&mut self) {
        self.requests = 0;
        self.window_start = Instant::now();
    }

    fn increment(&mut self) -> u32 {
        if self.is_expired() {
            self.reset();
        }
        self.requests += 1;
        self.requests
    }
}

impl RateLimitingPlugin {
    pub fn new(config: &PluginConfig) -> Result<Self, GatewayError> {
        let mut limits = HashMap::new();
        
        if let Some(requests_per_minute) = config.config.get("requests_per_minute") {
            if let Some(value) = requests_per_minute.as_u64() {
                limits.insert("default".to_string(), value as u32);
            }
        }

        Ok(Self {
            name: config.name.clone(),
            enabled: config.enabled,
            limits,
            counters: DashMap::new(),
        })
    }

    fn get_client_id(&self, request: &Request<Body>) -> String {
        // Extract client ID from headers or use IP address
        request
            .headers()
            .get("x-api-key")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("anonymous")
            .to_string()
    }

    fn check_rate_limit(&self, client_id: &str) -> Result<(), GatewayError> {
        let limit = self.limits.get("default").unwrap_or(&100);
        
        let mut counter = self.counters
            .entry(client_id.to_string())
            .or_insert_with(RateLimitCounter::new);
        
        let current_requests = counter.increment();
        
        if current_requests > *limit {
            return Err(GatewayError::RateLimit {
                message: format!("Rate limit exceeded: {} requests per minute", limit),
            });
        }
        
        Ok(())
    }
}

impl RateLimitingPlugin {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub async fn before_request(&self, request: &mut Request<Body>) -> Result<(), GatewayError> {
        let client_id = self.get_client_id(request);
        self.check_rate_limit(&client_id)?;
        Ok(())
    }

    pub async fn after_response(&self, _response: &mut Response<Body>) -> Result<(), GatewayError> {
        // Could track successful requests here
        Ok(())
    }

    pub async fn on_error(&self, _error: &GatewayError) -> Result<(), GatewayError> {
        // Could track failed requests here
        Ok(())
    }
} 