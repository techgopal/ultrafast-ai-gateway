//! # Circuit Breaker Module
//!
//! This module provides a robust circuit breaker implementation for the Ultrafast Models SDK.
//! Circuit breakers prevent cascading failures by automatically detecting failing providers
//! and temporarily blocking requests to them, allowing them to recover.
//!
//! ## Overview
//!
//! The circuit breaker pattern helps maintain system stability by:
//! - **Failure Detection**: Automatically detecting when providers are failing
//! - **Failure Isolation**: Preventing cascading failures across the system
//! - **Automatic Recovery**: Allowing providers to recover automatically
//! - **Graceful Degradation**: Providing fallback mechanisms during outages
//! - **Performance Monitoring**: Tracking provider health and performance
//!
//! ## Circuit Breaker States
//!
//! The circuit breaker operates in three states:
//!
//! ### Closed State (Normal Operation)
//! - All requests are allowed to pass through
//! - Failures are counted and tracked
//! - When failure threshold is reached, transitions to Open state
//!
//! ### Open State (Failure Detected)
//! - All requests are immediately rejected
//! - No calls are made to the failing provider
//! - After recovery timeout, transitions to Half-Open state
//!
//! ### Half-Open State (Testing Recovery)
//! - Limited number of test requests are allowed
//! - Success transitions back to Closed state
//! - Failure transitions back to Open state
//!
//! ## Configuration
//!
//! Circuit breakers can be configured with:
//! - **Failure Threshold**: Number of failures before opening circuit
//! - **Recovery Timeout**: Time to wait before testing recovery
//! - **Request Timeout**: Maximum time to wait for individual requests
//! - **Half-Open Max Calls**: Number of test requests in half-open state
//!
//! ## Usage Examples
//!
//! ### Basic Circuit Breaker Usage
//!
//! ```rust
//! use ultrafast_models_sdk::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
//! use std::time::Duration;
//!
//! // Create circuit breaker configuration
//! let config = CircuitBreakerConfig {
//!     failure_threshold: 5,
//!     recovery_timeout: Duration::from_secs(30),
//!     request_timeout: Duration::from_secs(10),
//!     half_open_max_calls: 3,
//! };
//!
//! // Create circuit breaker for a provider
//! let circuit_breaker = CircuitBreaker::new("openai".to_string(), config);
//!
//! // Execute operation with circuit breaker protection
//! let result = circuit_breaker.call(|| async {
//!     // Your provider call here
//!     provider.chat_completion(request).await
//! }).await;
//!
//! match result {
//!     Ok(response) => println!("Success: {}", response.choices[0].message.content),
//!     Err(CircuitBreakerError::Open) => {
//!         println!("Circuit breaker is open - provider is failing");
//!     }
//!     Err(CircuitBreakerError::Timeout) => {
//!         println!("Request timed out");
//!     }
//! }
//! ```
//!
//! ### Circuit Breaker with Metrics
//!
//! ```rust
//! use ultrafast_models_sdk::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
//!
//! let circuit_breaker = CircuitBreaker::new("anthropic".to_string(), config);
//!
//! // Get circuit breaker metrics
//! let metrics = circuit_breaker.get_metrics().await;
//! println!("Circuit state: {:?}", metrics.state);
//! println!("Failure count: {}", metrics.failure_count);
//! println!("Success count: {}", metrics.success_count);
//!
//! // Force circuit breaker states (for testing)
//! circuit_breaker.force_open().await;
//! circuit_breaker.force_closed().await;
//! ```
//!
//! ### Integration with Provider System
//!
//! ```rust
//! use ultrafast_models_sdk::{UltrafastClient, CircuitBreakerConfig};
//!
//! let client = UltrafastClient::standalone()
//!     .with_openai("your-key")
//!     .with_circuit_breaker_config(CircuitBreakerConfig {
//!         failure_threshold: 3,
//!         recovery_timeout: Duration::from_secs(60),
//!         request_timeout: Duration::from_secs(30),
//!         half_open_max_calls: 2,
//!     })
//!     .build()?;
//!
//! // The client automatically uses circuit breakers for all providers
//! let response = client.chat_completion(request).await?;
//! ```
//!
//! ## Best Practices
//!
//! - **Appropriate Thresholds**: Set failure thresholds based on expected failure rates
//! - **Monitoring**: Monitor circuit breaker metrics and alert on state changes
//! - **Fallback Strategies**: Implement fallback providers when circuits are open
//! - **Testing**: Test circuit breaker behavior with controlled failures
//! - **Configuration**: Adjust timeouts based on provider response characteristics
//!
//! ## Performance Considerations
//!
//! - **Low Overhead**: Circuit breaker adds minimal latency to successful requests
//! - **Memory Efficient**: Minimal memory footprint for tracking state
//! - **Thread Safe**: Safe for concurrent access across multiple threads
//! - **Fast State Transitions**: State changes are atomic and fast

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;

/// Circuit breaker specific errors.
///
/// These errors occur when the circuit breaker prevents operations
/// due to detected failures or timeouts.
#[derive(Debug, Error)]
pub enum CircuitBreakerError {
    /// Circuit breaker is open due to too many failures
    #[error("Circuit breaker is open - too many failures")]
    Open,
    /// Request timed out while circuit breaker was processing
    #[error("Circuit breaker timeout")]
    Timeout,
}

/// Circuit breaker operational states.
///
/// The circuit breaker operates in three distinct states to manage
/// provider failures and recovery.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    /// Normal operation - requests are allowed
    Closed,
    /// Failure detected - requests are blocked
    Open,
    /// Testing recovery - limited requests allowed
    HalfOpen,
}

/// Configuration for circuit breaker behavior.
///
/// This struct defines the parameters that control how the circuit breaker
/// detects failures, manages state transitions, and handles recovery.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::circuit_breaker::CircuitBreakerConfig;
/// use std::time::Duration;
///
/// let config = CircuitBreakerConfig {
///     failure_threshold: 5,
///     recovery_timeout: Duration::from_secs(30),
///     request_timeout: Duration::from_secs(10),
///     half_open_max_calls: 3,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before opening the circuit
    pub failure_threshold: u32,
    /// Time to wait before testing if the provider has recovered
    #[serde(with = "crate::common::duration_serde")]
    pub recovery_timeout: Duration,
    /// Maximum time to wait for individual requests
    #[serde(with = "crate::common::duration_serde")]
    pub request_timeout: Duration,
    /// Maximum number of test calls allowed in half-open state
    pub half_open_max_calls: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(60),
            request_timeout: Duration::from_secs(30),
            half_open_max_calls: 3,
        }
    }
}

/// Internal state tracking for circuit breaker.
///
/// This struct maintains the internal state of the circuit breaker,
/// including failure counts, timing information, and current state.
#[derive(Debug)]
struct CircuitBreakerState {
    /// Current circuit breaker state
    state: CircuitState,
    /// Number of consecutive failures
    failure_count: u32,
    /// Number of consecutive successes
    success_count: u32,
    /// Timestamp of the last failure
    last_failure_time: Option<Instant>,
    /// Timestamp of the last success
    last_success_time: Option<Instant>,
    /// Number of calls made in half-open state
    half_open_calls: u32,
}

impl Default for CircuitBreakerState {
    fn default() -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: None,
            last_success_time: None,
            half_open_calls: 0,
        }
    }
}

/// Circuit breaker implementation for provider failure management.
///
/// This struct provides the main circuit breaker functionality,
/// automatically detecting failures and managing state transitions
/// to prevent cascading failures.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
/// use std::time::Duration;
///
/// let config = CircuitBreakerConfig::default();
/// let circuit_breaker = CircuitBreaker::new("openai".to_string(), config);
///
/// // Execute operation with circuit breaker protection
/// let result = circuit_breaker.call(|| async {
///     // Your provider operation here
///     Ok("success")
/// }).await;
/// ```
pub struct CircuitBreaker {
    /// Circuit breaker configuration
    config: CircuitBreakerConfig,
    /// Internal state with thread-safe access
    state: Arc<RwLock<CircuitBreakerState>>,
    /// Name identifier for this circuit breaker
    name: String,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the specified configuration.
    ///
    /// # Arguments
    ///
    /// * `name` - Unique identifier for this circuit breaker
    /// * `config` - Configuration parameters for circuit breaker behavior
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
    ///
    /// let config = CircuitBreakerConfig::default();
    /// let circuit_breaker = CircuitBreaker::new("anthropic".to_string(), config);
    /// ```
    pub fn new(name: String, config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(CircuitBreakerState::default())),
            name,
        }
    }

    /// Execute an operation with circuit breaker protection.
    ///
    /// This method automatically manages the circuit breaker state based on
    /// the success or failure of the provided operation.
    ///
    /// # Arguments
    ///
    /// * `operation` - The operation to execute with circuit breaker protection
    ///
    /// # Returns
    ///
    /// Returns the result of the operation, or a circuit breaker error if
    /// the operation should be blocked.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::circuit_breaker::{CircuitBreaker, CircuitBreakerError};
    ///
    /// let circuit_breaker = CircuitBreaker::new("provider".to_string(), config);
    ///
    /// let result = circuit_breaker.call(|| async {
    ///     // Your provider call here
    ///     provider.chat_completion(request).await
    /// }).await;
    ///
    /// match result {
    ///     Ok(response) => println!("Success"),
    ///     Err(CircuitBreakerError::Open) => println!("Circuit is open"),
    ///     Err(CircuitBreakerError::Timeout) => println!("Request timed out"),
    /// }
    /// ```
    pub async fn call<F, Fut, T, E>(&self, operation: F) -> Result<T, CircuitBreakerError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::error::Error + Send + Sync + 'static,
    {
        // Check if we can make the call based on current state
        if !self.can_execute().await {
            tracing::warn!("Circuit breaker {} is OPEN - blocking request", self.name);
            return Err(CircuitBreakerError::Open);
        }

        // Execute the operation with timeout
        let _start_time = Instant::now();
        let result = tokio::time::timeout(self.config.request_timeout, operation()).await;

        match result {
            Ok(Ok(success_result)) => {
                // Operation succeeded - update circuit breaker state
                self.on_success().await;
                Ok(success_result)
            }
            Ok(Err(_)) => {
                // Operation failed - update circuit breaker state
                self.on_failure().await;
                Err(CircuitBreakerError::Open)
            }
            Err(_) => {
                // Operation timed out - update circuit breaker state
                self.on_failure().await;
                Err(CircuitBreakerError::Timeout)
            }
        }
    }

    /// Check if the circuit breaker allows execution.
    ///
    /// This method determines whether the current state allows
    /// the execution of operations.
    async fn can_execute(&self) -> bool {
        let mut state = self.state.write().await;

        match state.state {
            CircuitState::Closed => {
                // Normal operation - always allow
                true
            }
            CircuitState::Open => {
                // Check if recovery timeout has elapsed
                if let Some(last_failure) = state.last_failure_time {
                    if last_failure.elapsed() >= self.config.recovery_timeout {
                        // Transition to half-open state
                        state.state = CircuitState::HalfOpen;
                        state.half_open_calls = 0;
                        tracing::info!("Circuit breaker {} transitioning to HALF-OPEN", self.name);
                        true
                    } else {
                        // Still in open state - block requests
                        false
                    }
                } else {
                    // No failure recorded - should not happen in open state
                    false
                }
            }
            CircuitState::HalfOpen => {
                // Allow limited number of test calls
                if state.half_open_calls < self.config.half_open_max_calls {
                    state.half_open_calls += 1;
                    true
                } else {
                    // Max test calls reached - block requests
                    false
                }
            }
        }
    }

    /// Handle successful operation completion.
    ///
    /// Updates the circuit breaker state when an operation succeeds,
    /// potentially transitioning from half-open to closed state.
    async fn on_success(&self) {
        let mut state = self.state.write().await;

        state.success_count += 1;
        state.last_success_time = Some(Instant::now());

        match state.state {
            CircuitState::Closed => {
                // Reset failure count on success
                state.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                // Success in half-open state - transition to closed
                state.state = CircuitState::Closed;
                state.failure_count = 0;
                state.half_open_calls = 0;
                tracing::info!("Circuit breaker {} transitioning to CLOSED", self.name);
            }
            CircuitState::Open => {
                // Should not happen - open state should block all requests
                tracing::warn!(
                    "Unexpected success in OPEN state for circuit breaker {}",
                    self.name
                );
            }
        }
    }

    /// Handle operation failure.
    ///
    /// Updates the circuit breaker state when an operation fails,
    /// potentially transitioning to open state.
    async fn on_failure(&self) {
        let mut state = self.state.write().await;

        state.failure_count += 1;
        state.last_failure_time = Some(Instant::now());

        match state.state {
            CircuitState::Closed => {
                // Check if failure threshold reached
                if state.failure_count >= self.config.failure_threshold {
                    state.state = CircuitState::Open;
                    tracing::warn!(
                        "Circuit breaker {} transitioning to OPEN after {} failures",
                        self.name,
                        state.failure_count
                    );
                }
            }
            CircuitState::HalfOpen => {
                // Failure in half-open state - transition back to open
                state.state = CircuitState::Open;
                state.half_open_calls = 0;
                tracing::warn!("Circuit breaker {} transitioning back to OPEN", self.name);
            }
            CircuitState::Open => {
                // Already open - just update failure count
                // This can happen if we're still processing requests that were allowed
            }
        }
    }

    /// Get the current circuit breaker state.
    ///
    /// Returns the current operational state of the circuit breaker.
    pub async fn get_state(&self) -> CircuitState {
        let state = self.state.read().await;
        state.state
    }

    /// Get comprehensive metrics for this circuit breaker.
    ///
    /// Returns detailed metrics including state, failure/success counts,
    /// and timing information.
    pub async fn get_metrics(&self) -> CircuitBreakerMetrics {
        let state = self.state.read().await;

        CircuitBreakerMetrics {
            name: self.name.clone(),
            state: state.state,
            failure_count: state.failure_count,
            success_count: state.success_count,
            last_failure_time: state.last_failure_time,
            last_success_time: state.last_success_time,
        }
    }

    /// Force the circuit breaker to open state.
    ///
    /// This method manually opens the circuit breaker, useful for testing
    /// or emergency situations.
    pub async fn force_open(&self) {
        let mut state = self.state.write().await;
        state.state = CircuitState::Open;
        state.last_failure_time = Some(Instant::now());
        tracing::info!("Circuit breaker {} manually forced to OPEN", self.name);
    }

    /// Force the circuit breaker to closed state.
    ///
    /// This method manually closes the circuit breaker, useful for testing
    /// or when you want to reset the circuit breaker state.
    pub async fn force_closed(&self) {
        let mut state = self.state.write().await;
        state.state = CircuitState::Closed;
        state.failure_count = 0;
        state.success_count = 0;
        state.half_open_calls = 0;
        state.last_failure_time = None;
        state.last_success_time = None;
        tracing::info!("Circuit breaker {} manually forced to CLOSED", self.name);
    }
}

/// Metrics and statistics for circuit breaker performance.
///
/// This struct provides comprehensive metrics about the circuit breaker's
/// performance and state history.
#[derive(Debug)]
pub struct CircuitBreakerMetrics {
    /// Circuit breaker name/identifier
    pub name: String,
    /// Current operational state
    pub state: CircuitState,
    /// Total number of failures recorded
    pub failure_count: u32,
    /// Total number of successes recorded
    pub success_count: u32,
    /// Timestamp of the last failure
    pub last_failure_time: Option<Instant>,
    /// Timestamp of the last success
    pub last_success_time: Option<Instant>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Test circuit breaker in closed state with successful operations
    #[tokio::test]
    async fn test_circuit_breaker_closed_state() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            recovery_timeout: Duration::from_secs(1),
            request_timeout: Duration::from_secs(1),
            half_open_max_calls: 2,
        };

        let circuit_breaker = CircuitBreaker::new("test".to_string(), config);

        // Should be closed initially
        assert_eq!(circuit_breaker.get_state().await, CircuitState::Closed);

        // Successful operation should remain closed
        let result = circuit_breaker
            .call(|| async { Ok::<String, std::io::Error>("success".to_string()) })
            .await;
        assert!(result.is_ok());
        assert_eq!(circuit_breaker.get_state().await, CircuitState::Closed);
    }

    /// Test circuit breaker opening after consecutive failures
    #[tokio::test]
    async fn test_circuit_breaker_opens_on_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_secs(1),
            request_timeout: Duration::from_secs(1),
            half_open_max_calls: 2,
        };

        let circuit_breaker = CircuitBreaker::new("test".to_string(), config);

        // First failure
        let result = circuit_breaker
            .call(|| async { Err::<String, std::io::Error>(std::io::Error::other("failure")) })
            .await;
        assert!(result.is_err());
        assert_eq!(circuit_breaker.get_state().await, CircuitState::Closed);

        // Second failure should open the circuit
        let result = circuit_breaker
            .call(|| async { Err::<String, std::io::Error>(std::io::Error::other("failure")) })
            .await;
        assert!(result.is_err());
        assert_eq!(circuit_breaker.get_state().await, CircuitState::Open);
    }

    /// Test circuit breaker recovery from open state
    #[tokio::test]
    async fn test_circuit_breaker_recovery() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(100), // Short timeout for testing
            request_timeout: Duration::from_secs(1),
            half_open_max_calls: 2,
        };

        let circuit_breaker = CircuitBreaker::new("test".to_string(), config);

        // Cause circuit to open
        let _ = circuit_breaker
            .call(|| async { Err::<String, std::io::Error>(std::io::Error::other("failure")) })
            .await;
        assert_eq!(circuit_breaker.get_state().await, CircuitState::Open);

        // Wait for recovery timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        // After timeout, the next allowed call will transition to Half-Open and, on success,
        // immediately move to Closed. We assert the end result is Closed after a successful call.
        let result = circuit_breaker
            .call(|| async { Ok::<String, std::io::Error>("success".to_string()) })
            .await;
        assert!(result.is_ok());
        assert_eq!(circuit_breaker.get_state().await, CircuitState::Closed);
    }

    /// Test circuit breaker timeout handling
    #[tokio::test]
    async fn test_circuit_breaker_timeout() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(1),
            request_timeout: Duration::from_millis(50), // Short timeout
            half_open_max_calls: 2,
        };

        let circuit_breaker = CircuitBreaker::new("test".to_string(), config);

        // Operation that takes longer than timeout
        let result = circuit_breaker
            .call(|| async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok::<String, std::io::Error>("success".to_string())
            })
            .await;

        assert!(matches!(result, Err(CircuitBreakerError::Timeout)));
    }
}
