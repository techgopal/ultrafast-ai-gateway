//! # Advanced Routing and Load Balancing Module
//!
//! This module provides intelligent request routing, load balancing, and provider
//! health monitoring for the Ultrafast Gateway. It includes circuit breakers,
//! health checks, and adaptive routing strategies.
//!
//! ## Overview
//!
//! The advanced routing system provides:
//! - **Intelligent Provider Selection**: Route requests to the best available provider
//! - **Health Monitoring**: Real-time provider health tracking
//! - **Circuit Breakers**: Automatic failover and recovery
//! - **Load Balancing**: Distribute requests across healthy providers
//! - **Adaptive Routing**: Dynamic routing based on performance metrics
//! - **Failover Strategies**: Automatic fallback to backup providers
//!
//! ## Routing Strategies
//!
//! The system supports multiple routing strategies:
//!
//! - **Single Provider**: Route all requests to a single provider
//! - **Load Balancing**: Distribute requests across multiple providers
//! - **Failover**: Primary provider with automatic fallback
//! - **A/B Testing**: Route requests to different providers for testing
//! - **Geographic Routing**: Route based on geographic location
//! - **Cost-Based Routing**: Route to the most cost-effective provider
//!
//! ## Health Monitoring
//!
//! Real-time provider health tracking includes:
//!
//! - **Latency Monitoring**: Track response times for each provider
//! - **Success Rate Tracking**: Monitor success/failure ratios
//! - **Consecutive Failures**: Track consecutive failure counts
//! - **Automatic Health Checks**: Periodic provider health verification
//! - **Degraded State Detection**: Identify underperforming providers
//!
//! ## Circuit Breaker Pattern
//!
//! The system implements circuit breakers for each provider:
//!
//! - **Closed State**: Normal operation, requests allowed
//! - **Open State**: Provider failing, requests blocked
//! - **Half-Open State**: Testing if provider has recovered
//! - **Automatic Recovery**: Automatic state transitions based on health
//!
//! ## Usage
//!
//! ```rust
//! use ultrafast_gateway::advanced_routing::{AdvancedRouter, RoutingConfig};
//! use ultrafast_models_sdk::routing::RoutingStrategy;
//!
//! // Create advanced router
//! let config = RoutingConfig {
//!     strategy: RoutingStrategy::LoadBalancing,
//!     health_check_interval: Duration::from_secs(30),
//!     failover_threshold: 0.8,
//! };
//!
//! let router = AdvancedRouter::new(RoutingStrategy::LoadBalancing, config);
//!
//! // Select provider for request
//! let selection = router.select_provider(&providers, &context).await;
//!
//! // Update provider health after request
//! router.update_provider_health("openai", true, 150).await;
//! ```
//!
//! ## Health Check Configuration
//!
//! Health checks can be configured via `RoutingConfig`:
//!
//! ```toml
//! [routing]
//! strategy = "load_balancing"
//! health_check_interval = "30s"
//! failover_threshold = 0.8
//! ```
//!
//! ## Performance Metrics
//!
//! The routing system tracks comprehensive metrics:
//!
//! - **Provider Latency**: Average response times per provider
//! - **Success Rates**: Request success percentages
//! - **Failure Counts**: Consecutive failure tracking
//! - **Health Status**: Current health state of each provider
//! - **Routing Decisions**: Provider selection statistics
//!
//! ## Failover Strategies
//!
//! The system supports multiple failover strategies:
//!
//! - **Immediate Failover**: Switch to backup on first failure
//! - **Threshold-Based**: Switch after N consecutive failures
//! - **Latency-Based**: Switch when latency exceeds threshold
//! - **Cost-Based**: Switch to cheaper provider when possible
//!
//! ## Monitoring and Alerts
//!
//! The routing system provides monitoring capabilities:
//!
//! - **Health Status Endpoints**: Real-time provider health
//! - **Performance Metrics**: Detailed routing statistics
//! - **Circuit Breaker Status**: Current state of each provider
//! - **Alert Integration**: Notifications for unhealthy providers

// Advanced routing and load balancing module with provider health checks
use crate::config::RoutingConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use ultrafast_models_sdk::providers::{HealthStatus, Provider};
use ultrafast_models_sdk::routing::{ProviderSelection, Router, RoutingContext, RoutingStrategy};

/// Global health checker storage for thread-safe access.
///
/// Uses `OnceLock` to ensure the health checker is initialized only once
/// and shared across all threads.
static HEALTH_CHECKER: OnceLock<Arc<RwLock<HealthChecker>>> = OnceLock::new();

/// Get the global health checker instance.
///
/// Returns a reference to the global health checker, initializing it
/// if it hasn't been initialized yet.
fn get_health_checker() -> &'static Arc<RwLock<HealthChecker>> {
    HEALTH_CHECKER.get_or_init(|| Arc::new(RwLock::new(HealthChecker::new())))
}

/// Health status for a specific provider.
///
/// Tracks the current health state, performance metrics, and failure
/// statistics for a provider.
///
/// # Example
///
/// ```rust
/// let status = ProviderHealthStatus {
///     provider_id: "openai".to_string(),
///     status: HealthStatus::Healthy,
///     last_check_timestamp: chrono::Utc::now().timestamp() as u64,
///     consecutive_failures: 0,
///     average_latency_ms: 150.0,
///     success_rate: 0.99,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealthStatus {
    /// Provider identifier
    pub provider_id: String,
    /// Current health status (Healthy, Degraded, Unhealthy, Unknown)
    pub status: HealthStatus,
    /// Timestamp of the last health check
    pub last_check_timestamp: u64,
    /// Number of consecutive failures
    pub consecutive_failures: u32,
    /// Average response latency in milliseconds
    pub average_latency_ms: f64,
    /// Success rate as a percentage (0.0 to 1.0)
    pub success_rate: f64,
}

impl Default for ProviderHealthStatus {
    fn default() -> Self {
        Self {
            provider_id: String::new(),
            status: HealthStatus::Unknown,
            last_check_timestamp: chrono::Utc::now().timestamp() as u64,
            consecutive_failures: 0,
            average_latency_ms: 0.0,
            success_rate: 1.0,
        }
    }
}

/// Health checker for monitoring provider health and performance.
///
/// Manages health checks, tracks performance metrics, and provides
/// health status information for routing decisions.
///
/// # Thread Safety
///
/// All operations are thread-safe and can be used concurrently.
///
/// # Example
///
/// ```rust
/// let mut health_checker = HealthChecker::new();
/// health_checker.set_config(routing_config);
///
/// let status = health_checker.check_provider_health("openai", &provider).await;
/// ```
#[derive(Debug)]
pub struct HealthChecker {
    /// Health status for each provider
    provider_health: HashMap<String, ProviderHealthStatus>,
    /// Routing configuration for health check settings
    config: Option<RoutingConfig>,
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthChecker {
    /// Create a new health checker instance.
    ///
    /// Initializes an empty health checker with no configuration.
    /// Use `set_config()` to configure health check behavior.
    ///
    /// # Returns
    ///
    /// Returns a new `HealthChecker` instance.
    pub fn new() -> Self {
        Self {
            provider_health: HashMap::new(),
            config: None,
        }
    }

    /// Set the routing configuration for health checks.
    ///
    /// Configures health check intervals, thresholds, and other
    /// routing-related settings.
    ///
    /// # Arguments
    ///
    /// * `config` - Routing configuration with health check settings
    pub fn set_config(&mut self, config: RoutingConfig) {
        self.config = Some(config);
    }

    /// Check the health of a specific provider.
    ///
    /// Performs a health check on the provider and updates the health
    /// status with current metrics.
    ///
    /// # Arguments
    ///
    /// * `provider_id` - The provider identifier
    /// * `provider` - The provider instance to check
    ///
    /// # Returns
    ///
    /// Returns the updated health status for the provider.
    pub async fn check_provider_health(
        &mut self,
        provider_id: &str,
        provider: &Arc<dyn Provider>,
    ) -> ProviderHealthStatus {
        let start = Instant::now();

        match provider.health_check().await {
            Ok(health) => {
                let latency = start.elapsed().as_millis() as f64;

                let status = self
                    .provider_health
                    .entry(provider_id.to_string())
                    .or_default();
                status.provider_id = provider_id.to_string();
                status.status = health.status;
                status.last_check_timestamp = chrono::Utc::now().timestamp() as u64;
                status.consecutive_failures = 0;

                // Update average latency with exponential moving average
                if status.average_latency_ms == 0.0 {
                    status.average_latency_ms = latency;
                } else {
                    status.average_latency_ms = status.average_latency_ms * 0.8 + latency * 0.2;
                }

                // Update success rate
                status.success_rate = status.success_rate * 0.9 + 0.1;

                status.clone()
            }
            Err(_) => {
                let status = self
                    .provider_health
                    .entry(provider_id.to_string())
                    .or_default();
                status.provider_id = provider_id.to_string();
                status.status = HealthStatus::Unhealthy;
                status.last_check_timestamp = chrono::Utc::now().timestamp() as u64;
                status.consecutive_failures += 1;

                // Update success rate
                status.success_rate *= 0.9;

                status.clone()
            }
        }
    }

    pub fn get_healthy_providers(&self) -> Vec<String> {
        self.provider_health
            .iter()
            .filter(|(_, health)| {
                matches!(
                    health.status,
                    HealthStatus::Healthy | HealthStatus::Degraded
                ) && health.consecutive_failures < 3
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn should_use_provider(&self, provider_id: &str) -> bool {
        if let Some(health) = self.provider_health.get(provider_id) {
            let config = self.config.as_ref();
            let failover_threshold = config.map(|c| c.failover_threshold).unwrap_or(0.8);

            health.success_rate >= failover_threshold && health.consecutive_failures < 5
        } else {
            true // Allow unknown providers to be tried
        }
    }

    pub fn get_provider_stats(&self) -> HashMap<String, ProviderHealthStatus> {
        self.provider_health.clone()
    }

    pub fn is_provider_healthy(&self, provider_id: &str) -> bool {
        if let Some(health) = self.provider_health.get(provider_id) {
            matches!(
                health.status,
                HealthStatus::Healthy | HealthStatus::Degraded
            ) && health.consecutive_failures < 3
        } else {
            true
        }
    }
}

pub struct AdvancedRouter {
    base_router: Router,
    health_checker: Arc<RwLock<HealthChecker>>,
    config: RoutingConfig,
}

impl AdvancedRouter {
    pub fn new(strategy: RoutingStrategy, config: RoutingConfig) -> Self {
        let health_checker = get_health_checker().clone();

        Self {
            base_router: Router::new(strategy),
            health_checker,
            config,
        }
    }

    pub async fn select_provider(
        &self,
        providers: &HashMap<String, Arc<dyn Provider>>,
        context: &RoutingContext,
    ) -> Option<ProviderSelection> {
        // First, filter out unhealthy providers
        let health_checker = self.health_checker.read().await;
        let healthy_provider_ids: Vec<String> = providers
            .keys()
            .filter(|id| health_checker.is_provider_healthy(id))
            .cloned()
            .collect();

        drop(health_checker);

        if healthy_provider_ids.is_empty() {
            tracing::warn!("No healthy providers available");
            return None;
        }

        // Use the base router to select from healthy providers
        self.base_router
            .select_provider(&healthy_provider_ids, context)
    }

    pub async fn update_provider_health(&self, provider_id: &str, success: bool, latency_ms: u64) {
        let mut health_checker = self.health_checker.write().await;
        let status = health_checker
            .provider_health
            .entry(provider_id.to_string())
            .or_default();

        if success {
            status.consecutive_failures = 0;
            status.success_rate = status.success_rate * 0.9 + 0.1;
        } else {
            status.consecutive_failures += 1;
            status.success_rate *= 0.9;
        }

        // Update average latency
        if status.average_latency_ms == 0.0 {
            status.average_latency_ms = latency_ms as f64;
        } else {
            status.average_latency_ms = status.average_latency_ms * 0.8 + latency_ms as f64 * 0.2;
        }

        status.last_check_timestamp = chrono::Utc::now().timestamp() as u64;
    }

    pub async fn start_health_monitoring(&self, providers: HashMap<String, Arc<dyn Provider>>) {
        let health_checker = self.health_checker.clone();
        let interval = self.config.health_check_interval;

        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                interval_timer.tick().await;

                let mut checker = health_checker.write().await;

                for (provider_id, provider) in &providers {
                    let _health_status = checker.check_provider_health(provider_id, provider).await;

                    tracing::debug!(
                        provider = provider_id,
                        status = ?_health_status.status,
                        latency_ms = _health_status.average_latency_ms,
                        "Provider health check completed"
                    );
                }

                drop(checker);

                // Small delay between provider checks to avoid overwhelming
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
    }

    pub async fn get_routing_stats(&self) -> RoutingStats {
        let health_checker = self.health_checker.read().await;
        let provider_stats = health_checker.get_provider_stats();

        let total_providers = provider_stats.len();
        let healthy_providers = provider_stats
            .values()
            .filter(|status| matches!(status.status, HealthStatus::Healthy))
            .count();
        let degraded_providers = provider_stats
            .values()
            .filter(|status| matches!(status.status, HealthStatus::Degraded))
            .count();
        let unhealthy_providers = provider_stats
            .values()
            .filter(|status| matches!(status.status, HealthStatus::Unhealthy))
            .count();

        let average_latency = if !provider_stats.is_empty() {
            provider_stats
                .values()
                .map(|status| status.average_latency_ms)
                .sum::<f64>()
                / provider_stats.len() as f64
        } else {
            0.0
        };

        RoutingStats {
            total_providers,
            healthy_providers,
            degraded_providers,
            unhealthy_providers,
            average_latency_ms: average_latency,
            provider_details: provider_stats,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingStats {
    pub total_providers: usize,
    pub healthy_providers: usize,
    pub degraded_providers: usize,
    pub unhealthy_providers: usize,
    pub average_latency_ms: f64,
    pub provider_details: HashMap<String, ProviderHealthStatus>,
}

// Failover and circuit breaker functionality
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    failure_threshold: u32,
    recovery_timeout: Duration,
    #[allow(dead_code)]
    half_open_max_calls: u32,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(60),
            half_open_max_calls: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    Closed,   // Normal operation
    Open,     // Failing, rejecting requests
    HalfOpen, // Testing if service has recovered
}

pub struct ProviderCircuitBreaker {
    state: CircuitState,
    failure_count: u32,
    last_failure_time: Option<Instant>,
    config: CircuitBreaker,
}

impl ProviderCircuitBreaker {
    pub fn new(config: CircuitBreaker) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            last_failure_time: None,
            config,
        }
    }

    pub fn can_execute(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(last_failure) = self.last_failure_time {
                    if last_failure.elapsed() >= self.config.recovery_timeout {
                        self.state = CircuitState::HalfOpen;
                        self.failure_count = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.state = CircuitState::Closed;
        self.last_failure_time = None;
    }

    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure_time = Some(Instant::now());

        match self.state {
            CircuitState::Closed => {
                if self.failure_count >= self.config.failure_threshold {
                    self.state = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                self.state = CircuitState::Open;
            }
            CircuitState::Open => {
                // Already open, do nothing
            }
        }
    }

    pub fn get_state(&self) -> CircuitState {
        self.state.clone()
    }
}
