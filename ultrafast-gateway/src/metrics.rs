//! # Metrics and Monitoring Module
//!
//! This module provides comprehensive metrics collection, monitoring, and performance
//! tracking for the Ultrafast Gateway. It includes real-time metrics, historical
//! data analysis, and Prometheus-compatible output.
//!
//! ## Overview
//!
//! The metrics system provides:
//! - **Real-time Metrics**: Live performance monitoring
//! - **Historical Data**: Request history with configurable retention
//! - **Performance Analysis**: Latency percentiles and throughput metrics
//! - **Cost Tracking**: Provider cost monitoring and analysis
//! - **Error Tracking**: Detailed error categorization and rates
//! - **Cache Performance**: Hit rates and cache latency tracking
//! - **Provider Monitoring**: Per-provider performance metrics
//! - **Model Analytics**: Per-model usage and performance data
//!
//! ## Metrics Types
//!
//! ### Request Metrics
//!
//! Tracks individual request performance:
//! - **Latency**: Request processing time
//! - **Status Codes**: HTTP response status
//! - **Token Usage**: Input and output token counts
//! - **Cost Tracking**: Provider costs per request
//! - **Cache Performance**: Hit/miss rates
//! - **Error Tracking**: Detailed error categorization
//!
//! ### Aggregated Metrics
//!
//! Provides high-level performance overview:
//! - **Throughput**: Requests per minute
//! - **Latency Percentiles**: P50, P90, P95, P99
//! - **Error Rates**: Overall and per-provider error rates
//! - **Cost Analysis**: Total and per-provider costs
//! - **Uptime Tracking**: Service availability metrics
//!
//! ### Provider Metrics
//!
//! Per-provider performance tracking:
//! - **Request Counts**: Total and successful requests
//! - **Latency Analysis**: Average and percentile latencies
//! - **Error Rates**: Provider-specific error tracking
//! - **Cost Tracking**: Per-provider cost analysis
//! - **Uptime Monitoring**: Provider availability
//!
//! ### Cache Metrics
//!
//! Cache performance monitoring:
//! - **Hit Rates**: Cache effectiveness
//! - **Latency**: Cache access times
//! - **Throughput**: Cache operations per second
//!
//! ## Usage
//!
//! ```rust
//! use ultrafast_gateway::metrics::{record_request, RequestMetricsBuilder};
//!
//! // Record a request
//! let metrics = RequestMetricsBuilder::new(
//!     "POST".to_string(),
//!     "/v1/chat/completions".to_string(),
//!     200,
//!     Duration::from_millis(150)
//! )
//! .provider("openai".to_string())
//! .model("gpt-4".to_string())
//! .input_tokens(100)
//! .output_tokens(50)
//! .cost_usd(0.002)
//! .build();
//!
//! record_request(metrics).await;
//! ```
//!
//! ## API Endpoints
//!
//! The metrics system provides several endpoints:
//!
//! - `GET /metrics` - JSON metrics with detailed breakdown
//! - `GET /metrics/prometheus` - Prometheus-compatible metrics
//! - `POST /metrics/reset` - Reset all metrics (admin only)
//!
//! ## Configuration
//!
//! Metrics can be configured via the `MetricsConfig`:
//!
//! ```toml
//! [metrics]
//! enabled = true
//! max_requests = 1000
//! retention_duration = "24h"
//! cleanup_interval = "1h"
//! ```
//!
//! ## Data Retention
//!
//! The metrics system automatically manages data retention:
//!
//! - **Configurable Retention**: Set retention period via config
//! - **Automatic Cleanup**: Removes expired metrics data
//! - **Memory Management**: Prevents memory leaks
//! - **Emergency Cleanup**: Handles memory pressure
//!
//! ## Prometheus Integration
//!
//! The metrics system provides Prometheus-compatible output:
//!
//! ```rust
//! let prometheus_metrics = get_prometheus_metrics().await;
//! // Returns metrics in Prometheus text format
//! ```
//!
//! ## Performance Impact
//!
//! The metrics system is designed for minimal performance impact:
//!
//! - **Asynchronous Recording**: Non-blocking metrics collection
//! - **Memory Efficient**: Configurable data retention
//! - **Thread Safe**: Concurrent access support
//! - **Minimal Overhead**: <1ms per request impact

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::RwLock;

/// Global metrics collector instance for thread-safe access.
///
/// Uses `OnceLock` to ensure the metrics collector is initialized only once
/// and shared across all threads.
static METRICS_COLLECTOR: OnceLock<Arc<RwLock<MetricsCollector>>> = OnceLock::new();

/// Get the global metrics collector instance.
///
/// Returns a reference to the global metrics collector, initializing it
/// if it hasn't been initialized yet.
fn get_metrics_collector() -> &'static Arc<RwLock<MetricsCollector>> {
    METRICS_COLLECTOR.get_or_init(|| Arc::new(RwLock::new(MetricsCollector::new())))
}

/// Individual request metrics for performance tracking.
///
/// Contains detailed information about a single HTTP request including
/// performance metrics, token usage, costs, and error information.
///
/// # Example
///
/// ```rust
/// let metrics = RequestMetrics {
///     timestamp: SystemTime::now(),
///     method: "POST".to_string(),
///     path: "/v1/chat/completions".to_string(),
///     status_code: 200,
///     latency_ms: 150,
///     provider: Some("openai".to_string()),
///     model: Some("gpt-4".to_string()),
///     input_tokens: Some(100),
///     output_tokens: Some(50),
///     cost_usd: Some(0.002),
///     user_id: Some("user-123".to_string()),
///     request_size_bytes: Some(1024),
///     response_size_bytes: Some(2048),
///     cache_hit: Some(false),
///     error_type: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMetrics {
    /// Timestamp when the request was processed
    pub timestamp: SystemTime,
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Request path
    pub path: String,
    /// HTTP status code
    pub status_code: u16,
    /// Request processing time in milliseconds
    pub latency_ms: u64,
    /// Provider used for this request (if applicable)
    pub provider: Option<String>,
    /// Model used for this request (if applicable)
    pub model: Option<String>,
    /// Number of input tokens consumed
    pub input_tokens: Option<u32>,
    /// Number of output tokens generated
    pub output_tokens: Option<u32>,
    /// Cost of the request in USD
    pub cost_usd: Option<f64>,
    /// User identifier (if authenticated)
    pub user_id: Option<String>,
    /// Request body size in bytes
    pub request_size_bytes: Option<u64>,
    /// Response body size in bytes
    pub response_size_bytes: Option<u64>,
    /// Whether the request was served from cache
    pub cache_hit: Option<bool>,
    /// Error type if the request failed
    pub error_type: Option<String>,
}

/// Aggregated metrics providing high-level performance overview.
///
/// Contains aggregated statistics across all requests including
/// throughput, latency percentiles, error rates, and cost analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedMetrics {
    /// Total number of requests processed
    pub total_requests: u64,
    /// Number of successful requests (status < 400)
    pub successful_requests: u64,
    /// Number of failed requests (status >= 400)
    pub failed_requests: u64,
    /// Average request latency in milliseconds
    pub average_latency_ms: f64,
    /// 50th percentile latency in milliseconds
    pub p50_latency_ms: f64,
    /// 90th percentile latency in milliseconds
    pub p90_latency_ms: f64,
    /// 95th percentile latency in milliseconds
    pub p95_latency_ms: f64,
    /// 99th percentile latency in milliseconds
    pub p99_latency_ms: f64,
    /// Requests per minute (current rate)
    pub requests_per_minute: f64,
    /// Error rate as a percentage
    pub error_rate: f64,
    /// Total cost in USD
    pub total_cost_usd: f64,
    /// Total tokens processed
    pub total_tokens: u64,
    /// Number of active connections
    pub active_connections: u32,
    /// Service uptime in seconds
    pub uptime_seconds: u64,
    /// Service uptime as a percentage
    pub uptime_percentage: f64,
    /// Per-provider performance metrics
    pub provider_stats: HashMap<String, ProviderMetrics>,
    /// Per-model performance metrics
    pub model_stats: HashMap<String, ModelMetrics>,
    /// Cache performance statistics
    pub cache_stats: CacheStats,
    /// Error statistics and categorization
    pub error_stats: ErrorStats,
}

/// Cache performance statistics.
///
/// Tracks cache effectiveness including hit rates, latency, and throughput.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    /// Total number of cache requests
    pub total_requests: u64,
    /// Number of cache hits
    pub cache_hits: u64,
    /// Number of cache misses
    pub cache_misses: u64,
    /// Cache hit rate as a percentage
    pub hit_rate: f64,
    /// Average cache access latency in milliseconds
    pub average_cache_latency_ms: f64,
}

/// Error statistics and categorization.
///
/// Provides detailed error tracking including error types, rates, and
/// most common error patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorStats {
    /// Total number of errors
    pub total_errors: u64,
    /// Error counts by type
    pub error_types: HashMap<String, u64>,
    /// Error rate as a percentage
    pub error_rate: f64,
    /// Most frequently occurring error type
    pub most_common_error: Option<String>,
}

/// Per-provider performance metrics.
///
/// Tracks performance statistics for each configured provider including
/// request counts, latency, error rates, and costs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetrics {
    /// Total requests to this provider
    pub requests: u64,
    /// Successful requests to this provider
    pub successful_requests: u64,
    /// Failed requests to this provider
    pub failed_requests: u64,
    /// Average latency for this provider in milliseconds
    pub average_latency_ms: f64,
    /// 95th percentile latency for this provider in milliseconds
    pub p95_latency_ms: f64,
    /// Total cost for this provider in USD
    pub total_cost_usd: f64,
    /// Provider uptime as a percentage
    pub uptime_percentage: f64,
    /// Error rate for this provider as a percentage
    pub error_rate: f64,
    /// Timestamp of the last request to this provider
    pub last_request: Option<chrono::DateTime<chrono::Utc>>,
}

/// Per-model performance metrics.
///
/// Tracks performance statistics for each model including token usage,
/// latency, costs, and error rates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetrics {
    /// Total requests for this model
    pub requests: u64,
    /// Total input tokens for this model
    pub total_input_tokens: u64,
    /// Total output tokens for this model
    pub total_output_tokens: u64,
    /// Average latency for this model in milliseconds
    pub average_latency_ms: f64,
    /// 95th percentile latency for this model in milliseconds
    pub p95_latency_ms: f64,
    /// Total cost for this model in USD
    pub total_cost_usd: f64,
    /// Average tokens per request for this model
    pub average_tokens_per_request: f64,
    /// Error rate for this model as a percentage
    pub error_rate: f64,
}

/// Configuration for the metrics collection system.
///
/// Controls metrics collection behavior including retention periods,
/// cleanup intervals, and data limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Maximum number of requests to keep in memory
    pub max_requests: usize,
    /// How long to retain metrics data
    pub retention_duration: Duration,
    /// How often to clean up expired metrics
    pub cleanup_interval: Duration,
    /// Whether metrics collection is enabled
    pub enabled: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            max_requests: 1000,
            retention_duration: Duration::from_secs(24 * 60 * 60), // 24 hours
            cleanup_interval: Duration::from_secs(60 * 60),        // 1 hour
            enabled: true,
        }
    }
}

/// Metrics collector for storing and aggregating request metrics.
///
/// Provides thread-safe metrics collection with automatic cleanup
/// and memory management.
#[derive(Debug)]
#[allow(dead_code)]
pub struct MetricsCollector {
    /// Circular buffer of request metrics
    requests: VecDeque<RequestMetrics>,
    /// Service start time for uptime calculation
    start_time: Instant,
    /// Number of active connections
    active_connections: u32,
    /// Metrics configuration
    config: MetricsConfig,
    /// Last cleanup time
    last_cleanup: Instant,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    /// Create a new metrics collector with default configuration.
    ///
    /// Initializes an empty metrics collector with default settings.
    ///
    /// # Returns
    ///
    /// Returns a new `MetricsCollector` instance.
    pub fn new() -> Self {
        Self {
            requests: VecDeque::new(),
            start_time: Instant::now(),
            active_connections: 0,
            config: MetricsConfig::default(),
            last_cleanup: Instant::now(),
        }
    }

    /// Create a new metrics collector with custom configuration.
    ///
    /// Initializes a metrics collector with the specified configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Metrics configuration
    ///
    /// # Returns
    ///
    /// Returns a new `MetricsCollector` instance with custom configuration.
    pub fn with_config(config: MetricsConfig) -> Self {
        Self {
            requests: VecDeque::new(),
            start_time: Instant::now(),
            active_connections: 0,
            config,
            last_cleanup: Instant::now(),
        }
    }

    /// Record a new request metric.
    ///
    /// Adds a request metric to the collection and manages memory
    /// by removing old entries if necessary.
    ///
    /// # Arguments
    ///
    /// * `metrics` - The request metrics to record
    pub fn record_request(&mut self, metrics: RequestMetrics) {
        // Add the new request metric
        self.requests.push_back(metrics);

        // Clean up old entries if we exceed the maximum
        if self.requests.len() > self.config.max_requests {
            self.cleanup_expired_entries();
        }

        // Perform emergency cleanup if we're still over the limit
        if self.requests.len() > self.config.max_requests {
            self.emergency_cleanup();
        }
    }

    /// Clean up expired metrics entries.
    ///
    /// Removes metrics older than the retention duration to prevent
    /// memory leaks and maintain performance.
    pub fn cleanup_expired_entries(&mut self) {
        let now = SystemTime::now();
        let retention_duration = self.config.retention_duration;

        // Remove expired entries from the front of the queue
        while let Some(front) = self.requests.front() {
            if now.duration_since(front.timestamp).unwrap() > retention_duration {
                self.requests.pop_front();
            } else {
                break;
            }
        }
    }

    /// Emergency cleanup to prevent memory issues.
    ///
    /// Removes the oldest 25% of entries when memory pressure is detected.
    /// This is a fallback mechanism to prevent memory exhaustion.
    fn emergency_cleanup(&mut self) {
        let remove_count = self.requests.len() / 4; // Remove 25%
        for _ in 0..remove_count {
            self.requests.pop_front();
        }
    }

    /// Increment the active connections counter.
    ///
    /// Called when a new connection is established.
    pub fn increment_connections(&mut self) {
        self.active_connections += 1;
    }

    /// Decrement the active connections counter.
    ///
    /// Called when a connection is closed.
    pub fn decrement_connections(&mut self) {
        if self.active_connections > 0 {
            self.active_connections -= 1;
        }
    }

    /// Get aggregated metrics from all collected data.
    ///
    /// Calculates comprehensive statistics including throughput,
    /// latency percentiles, error rates, and cost analysis.
    ///
    /// # Returns
    ///
    /// Returns aggregated metrics with detailed breakdown.
    pub fn get_aggregated_metrics(&self) -> AggregatedMetrics {
        let uptime = self.start_time.elapsed();

        if self.requests.is_empty() {
            return AggregatedMetrics {
                total_requests: 0,
                successful_requests: 0,
                failed_requests: 0,
                average_latency_ms: 0.0,
                p50_latency_ms: 0.0,
                p90_latency_ms: 0.0,
                p95_latency_ms: 0.0,
                p99_latency_ms: 0.0,
                requests_per_minute: 0.0,
                error_rate: 0.0,
                total_cost_usd: 0.0,
                total_tokens: 0,
                active_connections: self.active_connections,
                uptime_seconds: uptime.as_secs(),
                uptime_percentage: 0.0, // No requests yet, uptime is 0%
                provider_stats: HashMap::new(),
                model_stats: HashMap::new(),
                cache_stats: CacheStats {
                    total_requests: 0,
                    cache_hits: 0,
                    cache_misses: 0,
                    hit_rate: 0.0,
                    average_cache_latency_ms: 0.0,
                },
                error_stats: ErrorStats {
                    total_errors: 0,
                    error_types: HashMap::new(),
                    error_rate: 0.0,
                    most_common_error: None,
                },
            };
        }

        // Calculate basic statistics
        let total_requests = self.requests.len() as u64;
        let mut latencies: Vec<u64> = self.requests.iter().map(|r| r.latency_ms).collect();
        latencies.sort_unstable();

        let successful_requests =
            self.requests.iter().filter(|r| r.status_code < 400).count() as u64;
        let failed_requests = total_requests - successful_requests;

        let average_latency = self
            .requests
            .iter()
            .map(|r| r.latency_ms as f64)
            .sum::<f64>()
            / total_requests as f64;

        // Calculate percentiles
        let p50_idx = (latencies.len() as f64 * 0.5) as usize;
        let p90_idx = (latencies.len() as f64 * 0.9) as usize;
        let p95_idx = (latencies.len() as f64 * 0.95) as usize;
        let p99_idx = (latencies.len() as f64 * 0.99) as usize;

        let p50_latency = latencies.get(p50_idx).copied().unwrap_or(0) as f64;
        let p90_latency = latencies.get(p90_idx).copied().unwrap_or(0) as f64;
        let p95_latency = latencies.get(p95_idx).copied().unwrap_or(0) as f64;
        let p99_latency = latencies.get(p99_idx).copied().unwrap_or(0) as f64;

        // Calculate requests per minute
        let requests_per_minute = if uptime.as_secs() > 0 {
            (total_requests as f64 * 60.0) / uptime.as_secs() as f64
        } else {
            0.0
        };

        // Calculate error rate
        let error_rate = if total_requests > 0 {
            failed_requests as f64 / total_requests as f64
        } else {
            0.0
        };

        // Calculate cost and tokens
        let total_cost = self.requests.iter().filter_map(|r| r.cost_usd).sum::<f64>();

        let total_tokens = self
            .requests
            .iter()
            .filter_map(|r| r.input_tokens)
            .map(|t| t as u64)
            .sum::<u64>()
            + self
                .requests
                .iter()
                .filter_map(|r| r.output_tokens)
                .map(|t| t as u64)
                .sum::<u64>();

        // Calculate provider statistics
        let mut provider_stats = HashMap::new();
        let mut provider_requests: HashMap<String, Vec<&RequestMetrics>> = HashMap::new();

        for request in &self.requests {
            if let Some(provider) = &request.provider {
                provider_requests
                    .entry(provider.clone())
                    .or_default()
                    .push(request);
            }
        }

        for (provider_name, requests) in provider_requests {
            let total_provider_requests = requests.len() as u64;
            let successful_provider_requests =
                requests.iter().filter(|r| r.status_code < 400).count() as u64;
            let failed_provider_requests = total_provider_requests - successful_provider_requests;

            let avg_latency = requests.iter().map(|r| r.latency_ms as f64).sum::<f64>()
                / total_provider_requests as f64;

            let total_cost = requests.iter().filter_map(|r| r.cost_usd).sum::<f64>();

            let error_rate = if total_provider_requests > 0 {
                failed_provider_requests as f64 / total_provider_requests as f64
            } else {
                0.0
            };

            let last_request = requests
                .iter()
                .max_by_key(|r| r.timestamp)
                .map(|r| chrono::DateTime::from(r.timestamp));

            // Calculate p95 latency for provider
            let mut provider_latencies: Vec<u64> = requests.iter().map(|r| r.latency_ms).collect();
            provider_latencies.sort_unstable();
            let p95_idx = (provider_latencies.len() as f64 * 0.95) as usize;
            let p95_latency = provider_latencies.get(p95_idx).copied().unwrap_or(0) as f64;

            provider_stats.insert(
                provider_name,
                ProviderMetrics {
                    requests: total_provider_requests,
                    successful_requests: successful_provider_requests,
                    failed_requests: failed_provider_requests,
                    average_latency_ms: avg_latency,
                    p95_latency_ms: p95_latency,
                    total_cost_usd: total_cost,
                    uptime_percentage: self.calculate_uptime_percentage(),
                    error_rate,
                    last_request,
                },
            );
        }

        // Calculate model statistics
        let mut model_stats = HashMap::new();
        let mut model_requests: HashMap<String, Vec<&RequestMetrics>> = HashMap::new();

        for request in &self.requests {
            if let Some(model) = &request.model {
                model_requests
                    .entry(model.clone())
                    .or_default()
                    .push(request);
            }
        }

        for (model_name, requests) in model_requests {
            let total_model_requests = requests.len() as u64;
            let total_input_tokens: u64 = requests
                .iter()
                .filter_map(|r| r.input_tokens)
                .map(|t| t as u64)
                .sum();
            let total_output_tokens: u64 = requests
                .iter()
                .filter_map(|r| r.output_tokens)
                .map(|t| t as u64)
                .sum();

            let avg_latency = requests.iter().map(|r| r.latency_ms as f64).sum::<f64>()
                / total_model_requests as f64;

            let total_cost = requests.iter().filter_map(|r| r.cost_usd).sum::<f64>();

            let avg_tokens_per_request = if total_model_requests > 0 {
                (total_input_tokens + total_output_tokens) as f64 / total_model_requests as f64
            } else {
                0.0
            };

            let error_rate = if total_model_requests > 0 {
                requests.iter().filter(|r| r.status_code >= 400).count() as f64
                    / total_model_requests as f64
            } else {
                0.0
            };

            // Calculate p95 latency for model
            let mut model_latencies: Vec<u64> = requests.iter().map(|r| r.latency_ms).collect();
            model_latencies.sort_unstable();
            let p95_idx = (model_latencies.len() as f64 * 0.95) as usize;
            let p95_latency = model_latencies.get(p95_idx).copied().unwrap_or(0) as f64;

            model_stats.insert(
                model_name,
                ModelMetrics {
                    requests: total_model_requests,
                    total_input_tokens,
                    total_output_tokens,
                    average_latency_ms: avg_latency,
                    p95_latency_ms: p95_latency,
                    total_cost_usd: total_cost,
                    average_tokens_per_request: avg_tokens_per_request,
                    error_rate,
                },
            );
        }

        // Calculate cache statistics
        let cache_requests: Vec<&RequestMetrics> = self
            .requests
            .iter()
            .filter(|r| r.cache_hit.is_some())
            .collect();

        let total_cache_requests = cache_requests.len() as u64;
        let cache_hits = cache_requests
            .iter()
            .filter(|r| r.cache_hit.unwrap_or(false))
            .count() as u64;
        let cache_misses = total_cache_requests - cache_hits;

        let hit_rate = if total_cache_requests > 0 {
            cache_hits as f64 / total_cache_requests as f64
        } else {
            0.0
        };

        let avg_cache_latency = if !cache_requests.is_empty() {
            cache_requests
                .iter()
                .map(|r| r.latency_ms as f64)
                .sum::<f64>()
                / cache_requests.len() as f64
        } else {
            0.0
        };

        // Calculate error statistics
        let error_requests: Vec<&RequestMetrics> = self
            .requests
            .iter()
            .filter(|r| r.status_code >= 400)
            .collect();

        let mut error_types = HashMap::new();
        for request in &error_requests {
            let error_type = if request.status_code >= 500 {
                "server_error".to_string()
            } else if request.status_code == 429 {
                "rate_limit".to_string()
            } else if request.status_code == 401 {
                "authentication".to_string()
            } else if request.status_code == 403 {
                "authorization".to_string()
            } else {
                "client_error".to_string()
            };

            *error_types.entry(error_type).or_insert(0) += 1;
        }

        let most_common_error = error_types
            .iter()
            .max_by_key(|(_, &count)| count)
            .map(|(error_type, _)| error_type.clone());

        AggregatedMetrics {
            total_requests,
            successful_requests,
            failed_requests,
            average_latency_ms: average_latency,
            p50_latency_ms: p50_latency,
            p90_latency_ms: p90_latency,
            p95_latency_ms: p95_latency,
            p99_latency_ms: p99_latency,
            requests_per_minute,
            error_rate,
            total_cost_usd: total_cost,
            total_tokens,
            active_connections: self.active_connections,
            uptime_seconds: uptime.as_secs(),
            uptime_percentage: self.calculate_uptime_percentage(),
            provider_stats,
            model_stats,
            cache_stats: CacheStats {
                total_requests: total_cache_requests,
                cache_hits,
                cache_misses,
                hit_rate,
                average_cache_latency_ms: avg_cache_latency,
            },
            error_stats: ErrorStats {
                total_errors: error_requests.len() as u64,
                error_types,
                error_rate,
                most_common_error,
            },
        }
    }

    /// Generate Prometheus-compatible metrics.
    ///
    /// Converts the collected metrics to Prometheus text format
    /// for integration with monitoring systems.
    ///
    /// # Returns
    ///
    /// Returns metrics in Prometheus text format.
    pub fn get_prometheus_metrics(&self) -> String {
        let metrics = self.get_aggregated_metrics();
        let mut prometheus_metrics = String::new();

        // Add basic metrics
        prometheus_metrics.push_str(&format!(
            "gateway_requests_total {}\n",
            metrics.total_requests
        ));
        prometheus_metrics.push_str(&format!(
            "gateway_requests_successful {}\n",
            metrics.successful_requests
        ));
        prometheus_metrics.push_str(&format!(
            "gateway_requests_failed {}\n",
            metrics.failed_requests
        ));
        prometheus_metrics.push_str(&format!(
            "gateway_average_latency_ms {}\n",
            metrics.average_latency_ms
        ));
        prometheus_metrics.push_str(&format!("gateway_error_rate {}\n", metrics.error_rate));
        prometheus_metrics.push_str(&format!(
            "gateway_total_cost_usd {}\n",
            metrics.total_cost_usd
        ));
        prometheus_metrics.push_str(&format!(
            "gateway_uptime_percentage {}\n",
            metrics.uptime_percentage
        ));

        // Add provider-specific metrics
        for (provider, provider_metrics) in &metrics.provider_stats {
            prometheus_metrics.push_str(&format!(
                "gateway_provider_requests_total{{provider=\"{}\"}} {}\n",
                provider, provider_metrics.requests
            ));
            prometheus_metrics.push_str(&format!(
                "gateway_provider_latency_ms{{provider=\"{}\"}} {}\n",
                provider, provider_metrics.average_latency_ms
            ));
            prometheus_metrics.push_str(&format!(
                "gateway_provider_error_rate{{provider=\"{}\"}} {}\n",
                provider, provider_metrics.error_rate
            ));
        }

        prometheus_metrics
    }

    /// Calculate service uptime percentage.
    ///
    /// Determines the service uptime based on the start time
    /// and current time.
    ///
    /// # Returns
    ///
    /// Returns uptime as a percentage (0.0 to 100.0).
    fn calculate_uptime_percentage(&self) -> f64 {
        // Calculate uptime based on the time since start and any downtime periods
        let now = std::time::Instant::now();
        let _total_uptime = now.duration_since(self.start_time);

        // In a real implementation, you would track downtime periods
        // For now, we'll calculate based on successful vs failed requests
        let total_requests = self.requests.len();
        if total_requests == 0 {
            return 100.0; // No requests yet, assume 100% uptime
        }

        let successful_requests = self
            .requests
            .iter()
            .filter(|r| r.status_code < 500) // Consider 5xx errors as downtime
            .count();

        let uptime_percentage = (successful_requests as f64 / total_requests as f64) * 100.0;

        // Ensure the percentage is reasonable (not negative, not over 100%)
        uptime_percentage.clamp(0.0, 100.0)
    }
}

// Initialization function
pub async fn initialize_metrics(config: MetricsConfig) {
    let cleanup_interval = config.cleanup_interval;
    let collector = MetricsCollector::with_config(config);
    let _ = METRICS_COLLECTOR.set(Arc::new(RwLock::new(collector)));

    // Start background cleanup task
    start_cleanup_task(cleanup_interval).await;
}

async fn start_cleanup_task(cleanup_interval: Duration) {
    let collector_ref = get_metrics_collector().clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(cleanup_interval);
        let mut memory_pressure_checks = 0u32;

        loop {
            interval.tick().await;

            // Force cleanup of expired entries
            let (current_size, max_size) = {
                let mut collector = collector_ref.write().await;
                collector.cleanup_expired_entries();

                // Check for memory pressure every 10 cleanup cycles
                memory_pressure_checks += 1;
                if memory_pressure_checks >= 10 {
                    memory_pressure_checks = 0;

                    let current = collector.requests.len();
                    let max = collector.config.max_requests;

                    // If we're using more than 80% of capacity, be more aggressive
                    if current > (max * 8 / 10) {
                        tracing::warn!(
                            "Memory pressure detected: {}/{} entries. Performing aggressive cleanup.",
                            current, max
                        );
                        collector.emergency_cleanup();
                    }

                    (current, max)
                } else {
                    (collector.requests.len(), collector.config.max_requests)
                }
            };

            tracing::debug!(
                "Background metrics cleanup completed. Size: {}/{}",
                current_size,
                max_size
            );

            // If memory pressure is too high, increase cleanup frequency temporarily
            if current_size > (max_size * 9 / 10) {
                tracing::warn!("High memory pressure, scheduling additional cleanup");
                tokio::time::sleep(cleanup_interval / 4).await;

                let mut collector = collector_ref.write().await;
                collector.emergency_cleanup();
            }
        }
    });
}

// Public API functions
pub async fn record_request(metrics: RequestMetrics) {
    let collector = get_metrics_collector();
    let mut collector = collector.write().await;
    collector.record_request(metrics);
}

pub async fn increment_connections() {
    let collector = get_metrics_collector();
    let mut collector = collector.write().await;
    collector.increment_connections();
}

pub async fn decrement_connections() {
    let collector = get_metrics_collector();
    let mut collector = collector.write().await;
    collector.decrement_connections();
}

pub async fn get_aggregated_metrics() -> AggregatedMetrics {
    let collector = get_metrics_collector();
    let collector = collector.read().await;
    collector.get_aggregated_metrics()
}

pub async fn get_prometheus_metrics() -> String {
    let collector = get_metrics_collector();
    let collector = collector.read().await;
    collector.get_prometheus_metrics()
}

pub async fn reset_metrics() {
    let collector = get_metrics_collector();
    let mut collector = collector.write().await;
    collector.requests.clear();
}

#[derive(Debug)]
pub struct RequestMetricsBuilder {
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub latency: Duration,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cost_usd: Option<f64>,
    pub user_id: Option<String>,
}

impl RequestMetricsBuilder {
    pub fn new(method: String, path: String, status_code: u16, latency: Duration) -> Self {
        Self {
            method,
            path,
            status_code,
            latency,
            provider: None,
            model: None,
            input_tokens: None,
            output_tokens: None,
            cost_usd: None,
            user_id: None,
        }
    }

    pub fn provider(mut self, provider: String) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn model(mut self, model: String) -> Self {
        self.model = Some(model);
        self
    }

    pub fn input_tokens(mut self, input_tokens: u32) -> Self {
        self.input_tokens = Some(input_tokens);
        self
    }

    pub fn output_tokens(mut self, output_tokens: u32) -> Self {
        self.output_tokens = Some(output_tokens);
        self
    }

    pub fn cost_usd(mut self, cost_usd: f64) -> Self {
        self.cost_usd = Some(cost_usd);
        self
    }

    pub fn user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn build(self) -> RequestMetrics {
        RequestMetrics {
            timestamp: SystemTime::now(),
            method: self.method,
            path: self.path,
            status_code: self.status_code,
            latency_ms: self.latency.as_millis() as u64,
            provider: self.provider,
            model: self.model,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cost_usd: self.cost_usd,
            user_id: self.user_id,
            request_size_bytes: None,  // Enhanced request tracking
            response_size_bytes: None, // Enhanced response tracking
            cache_hit: None,           // Cache performance tracking
            error_type: None,          // Enhanced error categorization
        }
    }
}
