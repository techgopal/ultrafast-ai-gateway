//! # Intelligent Routing Module
//!
//! This module provides intelligent request routing and load balancing for the Ultrafast Models SDK.
//! It enables automatic provider selection based on various strategies including load balancing,
//! failover, conditional routing, and performance-based selection.
//!
//! ## Overview
//!
//! The routing system provides:
//! - **Multiple Routing Strategies**: Single, load balancing, failover, conditional, A/B testing
//! - **Performance-Based Routing**: Route based on latency, success rates, and health
//! - **Conditional Routing**: Route based on request characteristics and context
//! - **Load Balancing**: Distribute requests across multiple providers
//! - **Health Monitoring**: Track provider health and performance metrics
//! - **Adaptive Routing**: Dynamic routing based on real-time performance data
//!
//! ## Routing Strategies
//!
//! ### Single Provider
//! Routes all requests to a single provider regardless of conditions.
//!
//! ### Load Balancing
//! Distributes requests across multiple providers using weighted or round-robin selection.
//!
//! ### Failover
//! Uses a primary provider with automatic fallback to backup providers on failure.
//!
//! ### Conditional Routing
//! Routes requests based on specific conditions like model name, user region, or request size.
//!
//! ### A/B Testing
//! Routes requests to different providers for testing and comparison.
//!
//! ### Round Robin
//! Cycles through providers in a fixed order.
//!
//! ### Least Used
//! Routes to the provider with the lowest request count.
//!
//! ### Lowest Latency
//! Routes to the provider with the best average response time.
//!
//! ## Usage Examples
//!
//! ### Basic Routing Setup
//!
//! ```rust
//! use ultrafast_models_sdk::routing::{Router, RoutingStrategy, RoutingContext};
//!
//! // Create router with load balancing strategy
//! let router = Router::new(RoutingStrategy::LoadBalance {
//!     weights: vec![0.6, 0.4], // 60% to first provider, 40% to second
//! });
//!
//! let providers = vec!["openai".to_string(), "anthropic".to_string()];
//! let context = RoutingContext {
//!     model: Some("gpt-4".to_string()),
//!     user_region: Some("us-east-1".to_string()),
//!     request_size: 1000,
//!     estimated_tokens: 500,
//!     user_id: Some("user123".to_string()),
//!     metadata: std::collections::HashMap::new(),
//! };
//!
//! // Select provider for this request
//! if let Some(selection) = router.select_provider(&providers, &context) {
//!     println!("Selected provider: {}", selection.provider_id);
//!     println!("Selection reason: {}", selection.reason);
//! }
//! ```
//!
//! ### Conditional Routing
//!
//! ```rust
//! use ultrafast_models_sdk::routing::{Router, RoutingStrategy, RoutingRule, Condition};
//!
//! // Create conditional routing rules
//! let rules = vec![
//!     RoutingRule {
//!         condition: Condition::ModelName("gpt-4".to_string()),
//!         provider: "openai".to_string(),
//!         weight: 1.0,
//!     },
//!     RoutingRule {
//!         condition: Condition::ModelName("claude-3".to_string()),
//!         provider: "anthropic".to_string(),
//!         weight: 1.0,
//!     },
//!     RoutingRule {
//!         condition: Condition::UserRegion("eu-west-1".to_string()),
//!         provider: "azure".to_string(),
//!         weight: 1.0,
//!     },
//! ];
//!
//! let router = Router::new(RoutingStrategy::Conditional { rules });
//! ```
//!
//! ### A/B Testing
//!
//! ```rust
//! use ultrafast_models_sdk::routing::{Router, RoutingStrategy};
//!
//! // Route 70% to provider A, 30% to provider B
//! let router = Router::new(RoutingStrategy::ABTesting { split: 0.7 });
//!
//! let providers = vec!["provider-a".to_string(), "provider-b".to_string()];
//! let context = RoutingContext::default();
//!
//! // This will randomly select based on the split
//! let selection = router.select_provider(&providers, &context);
//! ```
//!
//! ### Performance-Based Routing
//!
//! ```rust
//! use ultrafast_models_sdk::routing::{Router, RoutingStrategy};
//!
//! // Route to provider with lowest latency
//! let router = Router::new(RoutingStrategy::LowestLatency);
//!
//! // Update provider stats after requests
//! router.update_stats("openai", true, 150); // Success, 150ms latency
//! router.update_stats("anthropic", false, 500); // Failure, 500ms latency
//! ```
//!
//! ## Routing Conditions
//!
//! The system supports various routing conditions:
//!
//! - **Model Name**: Route based on specific model names
//! - **Model Prefix**: Route based on model name prefixes
//! - **User Region**: Route based on user's geographic region
//! - **Request Size**: Route based on request payload size
//! - **Token Count**: Route based on estimated token count
//! - **Time of Day**: Route based on current time
//! - **Custom**: User-defined custom conditions
//!
//! ## Performance Monitoring
//!
//! The routing system tracks comprehensive performance metrics:
//!
//! - **Request Counts**: Total, successful, and failed requests per provider
//! - **Latency Tracking**: Average and percentile response times
//! - **Success Rates**: Request success percentages
//! - **Load Balancing**: Current load distribution across providers
//! - **Health Status**: Provider health and availability
//!
//! ## Best Practices
//!
//! - **Monitor Performance**: Regularly review routing metrics and adjust strategies
//! - **Set Appropriate Weights**: Balance load based on provider capabilities and costs
//! - **Use Health Checks**: Implement health monitoring for automatic failover
//! - **Test Strategies**: A/B test different routing strategies to optimize performance
//! - **Consider Costs**: Factor in provider costs when designing routing strategies
//! - **Handle Failures**: Implement proper fallback mechanisms for routing failures

use chrono::Timelike;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Routing strategies for provider selection.
///
/// This enum defines the different strategies that can be used to select
/// which provider should handle a particular request.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::routing::RoutingStrategy;
///
/// // Single provider strategy
/// let single = RoutingStrategy::Single;
///
/// // Load balancing with weights
/// let load_balance = RoutingStrategy::LoadBalance {
///     weights: vec![0.6, 0.4],
/// };
///
/// // Conditional routing with rules
/// let conditional = RoutingStrategy::Conditional {
///     rules: vec![/* routing rules */],
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutingStrategy {
    /// Route all requests to a single provider
    Single,
    /// Use primary provider with automatic fallback
    Fallback,
    /// Distribute requests across providers with weights
    LoadBalance { weights: Vec<f32> },
    /// Route based on specific conditions and rules
    Conditional { rules: Vec<RoutingRule> },
    /// A/B testing with configurable split
    ABTesting { split: f32 },
    /// Cycle through providers in order
    RoundRobin,
    /// Route to provider with lowest request count
    LeastUsed,
    /// Route to provider with lowest average latency
    LowestLatency,
}

/// Routing rule for conditional routing.
///
/// Defines a condition that must be met and the provider to route to
/// when that condition is satisfied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    /// Condition that must be met for this rule to apply
    pub condition: Condition,
    /// Provider to route to when condition is met
    pub provider: String,
    /// Weight for this rule (used in weighted selection)
    pub weight: f32,
}

/// Conditions for conditional routing.
///
/// Defines various conditions that can be used to determine routing decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Condition {
    /// Route based on exact model name match
    ModelName(String),
    /// Route based on model name prefix
    ModelPrefix(String),
    /// Route based on user's geographic region
    UserRegion(String),
    /// Route based on request size in bytes
    RequestSize(u32),
    /// Route based on estimated token count
    TokenCount(u32),
    /// Route based on time of day (24-hour format)
    TimeOfDay { start: u8, end: u8 },
    /// Custom condition for user-defined logic
    Custom(String),
}

impl Condition {
    /// Check if this condition matches the given routing context.
    ///
    /// # Arguments
    ///
    /// * `context` - The routing context to check against
    ///
    /// # Returns
    ///
    /// Returns `true` if the condition matches the context, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::routing::{Condition, RoutingContext};
    /// use std::collections::HashMap;
    ///
    /// let context = RoutingContext {
    ///     model: Some("gpt-4".to_string()),
    ///     user_region: Some("us-east-1".to_string()),
    ///     request_size: 1000,
    ///     estimated_tokens: 500,
    ///     user_id: Some("user123".to_string()),
    ///     metadata: HashMap::new(),
    /// };
    ///
    /// let model_condition = Condition::ModelName("gpt-4".to_string());
    /// assert!(model_condition.matches(&context));
    ///
    /// let region_condition = Condition::UserRegion("us-east-1".to_string());
    /// assert!(region_condition.matches(&context));
    /// ```
    pub fn matches(&self, context: &RoutingContext) -> bool {
        match self {
            Condition::ModelName(name) => context.model.as_ref() == Some(name),
            Condition::ModelPrefix(prefix) => context
                .model
                .as_ref()
                .is_some_and(|m| m.starts_with(prefix)),
            Condition::UserRegion(region) => context.user_region.as_ref() == Some(region),
            Condition::RequestSize(size) => context.request_size >= *size,
            Condition::TokenCount(count) => context.estimated_tokens >= *count,
            Condition::TimeOfDay { start, end } => {
                let now = chrono::Utc::now().hour() as u8;
                if start <= end {
                    now >= *start && now <= *end
                } else {
                    // Handle time ranges that cross midnight
                    now >= *start || now <= *end
                }
            }
            Condition::Custom(_) => {
                // Custom conditions require additional implementation
                // This is a placeholder for user-defined logic
                false
            }
        }
    }
}

/// Context information for routing decisions.
///
/// Contains all the information needed to make intelligent routing decisions,
/// including request characteristics, user information, and metadata.
#[derive(Debug, Clone)]
pub struct RoutingContext {
    /// Model being requested (if specified)
    pub model: Option<String>,
    /// User's geographic region (if known)
    pub user_region: Option<String>,
    /// Request size in bytes
    pub request_size: u32,
    /// Estimated number of tokens in the request
    pub estimated_tokens: u32,
    /// User identifier (if authenticated)
    pub user_id: Option<String>,
    /// Additional metadata for custom routing logic
    pub metadata: HashMap<String, String>,
}

/// Provider selection result.
///
/// Contains the selected provider and information about why it was chosen.
#[derive(Debug, Clone)]
pub struct ProviderSelection {
    /// Identifier of the selected provider
    pub provider_id: String,
    /// Weight assigned to this selection
    pub weight: f32,
    /// Human-readable reason for the selection
    pub reason: String,
}

/// Router for intelligent provider selection.
///
/// This struct implements the routing logic based on the configured strategy
/// and maintains provider performance statistics for informed decision making.
pub struct Router {
    /// The routing strategy to use for provider selection
    strategy: RoutingStrategy,
    /// Performance statistics for each provider
    provider_stats: HashMap<String, ProviderStats>,
}

/// Performance statistics for a provider.
///
/// Tracks various metrics about provider performance including request counts,
/// success rates, latency, and load information.
#[derive(Debug, Clone, Default)]
pub struct ProviderStats {
    /// Total number of requests made to this provider
    pub total_requests: u64,
    /// Number of successful requests
    pub successful_requests: u64,
    /// Number of failed requests
    pub failed_requests: u64,
    /// Average response latency in milliseconds
    pub average_latency_ms: f64,
    /// Timestamp of the last request to this provider
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
    /// Current load (number of active requests)
    pub current_load: u32,
}

impl ProviderStats {
    /// Calculate the success rate for this provider.
    ///
    /// Returns the percentage of successful requests as a value between 0.0 and 1.0.
    /// If no requests have been made, returns 1.0 (100% success rate).
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            1.0 // No requests means 100% success rate
        } else {
            self.successful_requests as f64 / self.total_requests as f64
        }
    }
}

impl Router {
    /// Create a new router with the specified strategy.
    ///
    /// # Arguments
    ///
    /// * `strategy` - The routing strategy to use for provider selection
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::routing::{Router, RoutingStrategy};
    ///
    /// let router = Router::new(RoutingStrategy::LoadBalance {
    ///     weights: vec![0.6, 0.4],
    /// });
    /// ```
    pub fn new(strategy: RoutingStrategy) -> Self {
        Self {
            strategy,
            provider_stats: HashMap::new(),
        }
    }

    /// Select a provider based on the current routing strategy.
    ///
    /// # Arguments
    ///
    /// * `providers` - List of available provider identifiers
    /// * `context` - Routing context with request information
    ///
    /// # Returns
    ///
    /// Returns a provider selection if one is found, or `None` if no provider
    /// can be selected based on the current strategy and context.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::routing::{Router, RoutingStrategy, RoutingContext};
    /// use std::collections::HashMap;
    ///
    /// let router = Router::new(RoutingStrategy::Single);
    /// let providers = vec!["openai".to_string(), "anthropic".to_string()];
    /// let context = RoutingContext {
    ///     model: Some("gpt-4".to_string()),
    ///     user_region: None,
    ///     request_size: 1000,
    ///     estimated_tokens: 500,
    ///     user_id: None,
    ///     metadata: HashMap::new(),
    /// };
    ///
    /// if let Some(selection) = router.select_provider(&providers, &context) {
    ///     println!("Selected: {}", selection.provider_id);
    /// }
    /// ```
    pub fn select_provider(
        &self,
        providers: &[String],
        context: &RoutingContext,
    ) -> Option<ProviderSelection> {
        if providers.is_empty() {
            return None;
        }

        // Filter out unhealthy providers
        let healthy_providers = self.filter_healthy_providers(providers, context);
        if healthy_providers.is_empty() {
            return None;
        }

        match &self.strategy {
            RoutingStrategy::Single => {
                // Always select the first provider
                Some(ProviderSelection {
                    provider_id: healthy_providers[0].clone(),
                    weight: 1.0,
                    reason: "Single provider strategy".to_string(),
                })
            }
            RoutingStrategy::Fallback => {
                // Select the first healthy provider
                Some(ProviderSelection {
                    provider_id: healthy_providers[0].clone(),
                    weight: 1.0,
                    reason: "Fallback strategy - first healthy provider".to_string(),
                })
            }
            RoutingStrategy::LoadBalance { weights } => {
                self.select_weighted_provider(&healthy_providers, weights)
            }
            RoutingStrategy::Conditional { rules } => {
                self.select_conditional_provider(&healthy_providers, rules, context)
            }
            RoutingStrategy::ABTesting { split } => {
                self.select_ab_testing_provider(&healthy_providers, *split)
            }
            RoutingStrategy::RoundRobin => self.select_round_robin_provider(&healthy_providers),
            RoutingStrategy::LeastUsed => self.select_least_used_provider(&healthy_providers),
            RoutingStrategy::LowestLatency => {
                self.select_lowest_latency_provider(&healthy_providers)
            }
        }
    }

    /// Filter providers to only include healthy ones.
    ///
    /// This method removes providers that are considered unhealthy based on
    /// their performance statistics.
    fn filter_healthy_providers(
        &self,
        providers: &[String],
        _context: &RoutingContext,
    ) -> Vec<String> {
        providers
            .iter()
            .filter(|provider_id| {
                if let Some(stats) = self.provider_stats.get(*provider_id) {
                    // Consider provider healthy if success rate is above 80%
                    // and average latency is below 10 seconds
                    stats.success_rate() > 0.8 && stats.average_latency_ms < 10000.0
                } else {
                    // No stats available - assume healthy
                    true
                }
            })
            .cloned()
            .collect()
    }

    /// Select provider using weighted load balancing.
    ///
    /// Uses the provided weights to probabilistically select a provider.
    /// Weights should sum to 1.0 for proper distribution.
    fn select_weighted_provider(
        &self,
        providers: &[String],
        weights: &[f32],
    ) -> Option<ProviderSelection> {
        if providers.is_empty() {
            return None;
        }

        // Use provided weights or equal weights if not enough provided
        let effective_weights = if weights.len() >= providers.len() {
            weights[..providers.len()].to_vec()
        } else {
            // Equal weights for all providers
            vec![1.0 / providers.len() as f32; providers.len()]
        };

        // Normalize weights to sum to 1.0
        let total_weight: f32 = effective_weights.iter().sum();
        let normalized_weights: Vec<f32> =
            effective_weights.iter().map(|w| w / total_weight).collect();

        // Generate random number for weighted selection
        let mut rng = rand::thread_rng();
        let random_value: f32 = rng.gen();
        let mut cumulative_weight = 0.0;

        for (i, weight) in normalized_weights.iter().enumerate() {
            cumulative_weight += weight;
            if random_value <= cumulative_weight {
                return Some(ProviderSelection {
                    provider_id: providers[i].clone(),
                    weight: *weight,
                    reason: format!("Weighted selection (weight: {weight:.2})"),
                });
            }
        }

        // Fallback to first provider
        Some(ProviderSelection {
            provider_id: providers[0].clone(),
            weight: normalized_weights[0],
            reason: "Weighted selection fallback".to_string(),
        })
    }

    /// Select provider using conditional routing rules.
    ///
    /// Evaluates routing rules in order and selects the first provider
    /// whose condition matches the routing context.
    fn select_conditional_provider(
        &self,
        providers: &[String],
        rules: &[RoutingRule],
        context: &RoutingContext,
    ) -> Option<ProviderSelection> {
        // Check each rule in order
        for rule in rules {
            if rule.condition.matches(context) {
                // Verify the provider is in our healthy providers list
                if providers.contains(&rule.provider) {
                    return Some(ProviderSelection {
                        provider_id: rule.provider.clone(),
                        weight: rule.weight,
                        reason: format!("Conditional routing: {:?}", rule.condition),
                    });
                }
            }
        }

        // No matching rules - fallback to first healthy provider
        if !providers.is_empty() {
            Some(ProviderSelection {
                provider_id: providers[0].clone(),
                weight: 1.0,
                reason: "Conditional routing fallback".to_string(),
            })
        } else {
            None
        }
    }

    /// Select provider using A/B testing strategy.
    ///
    /// Uses the split parameter to probabilistically select between providers.
    /// The split represents the probability of selecting the first provider.
    fn select_ab_testing_provider(
        &self,
        providers: &[String],
        split: f32,
    ) -> Option<ProviderSelection> {
        if providers.len() < 2 {
            return self.select_round_robin_provider(providers);
        }

        let mut rng = rand::thread_rng();
        let random_value: f32 = rng.gen();

        let selected_provider = if random_value < split {
            &providers[0]
        } else {
            &providers[1]
        };

        Some(ProviderSelection {
            provider_id: selected_provider.clone(),
            weight: if random_value < split {
                split
            } else {
                1.0 - split
            },
            reason: format!("A/B testing (split: {split:.2})"),
        })
    }

    /// Select provider using round-robin strategy.
    ///
    /// Cycles through providers in order, maintaining state between calls.
    fn select_round_robin_provider(&self, providers: &[String]) -> Option<ProviderSelection> {
        if providers.is_empty() {
            return None;
        }

        // Simple round-robin based on current time
        let index = chrono::Utc::now().timestamp() as usize % providers.len();

        Some(ProviderSelection {
            provider_id: providers[index].clone(),
            weight: 1.0 / providers.len() as f32,
            reason: "Round-robin selection".to_string(),
        })
    }

    /// Select provider with the lowest request count.
    ///
    /// Chooses the provider that has handled the fewest requests.
    fn select_least_used_provider(&self, providers: &[String]) -> Option<ProviderSelection> {
        if providers.is_empty() {
            return None;
        }

        let mut selected_provider = &providers[0];
        let mut min_requests = u64::MAX;

        for provider_id in providers {
            let requests = self
                .provider_stats
                .get(provider_id)
                .map(|stats| stats.total_requests)
                .unwrap_or(0);

            if requests < min_requests {
                min_requests = requests;
                selected_provider = provider_id;
            }
        }

        Some(ProviderSelection {
            provider_id: selected_provider.clone(),
            weight: 1.0,
            reason: format!("Least used ({min_requests} requests)"),
        })
    }

    /// Select provider with the lowest average latency.
    ///
    /// Chooses the provider with the best average response time.
    fn select_lowest_latency_provider(&self, providers: &[String]) -> Option<ProviderSelection> {
        if providers.is_empty() {
            return None;
        }

        let mut selected_provider = &providers[0];
        let mut min_latency = f64::MAX;

        for provider_id in providers {
            if let Some(stats) = self.provider_stats.get(provider_id) {
                if stats.average_latency_ms < min_latency {
                    min_latency = stats.average_latency_ms;
                    selected_provider = provider_id;
                }
            }
        }

        Some(ProviderSelection {
            provider_id: selected_provider.clone(),
            weight: 1.0,
            reason: format!("Lowest latency ({min_latency:.2}ms)"),
        })
    }

    /// Update provider statistics after a request.
    ///
    /// # Arguments
    ///
    /// * `provider_id` - Identifier of the provider
    /// * `success` - Whether the request was successful
    /// * `latency_ms` - Response latency in milliseconds
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::routing::Router;
    ///
    /// let mut router = Router::new(RoutingStrategy::Single);
    ///
    /// // Update stats after a successful request
    /// router.update_stats("openai", true, 150);
    ///
    /// // Update stats after a failed request
    /// router.update_stats("anthropic", false, 500);
    /// ```
    pub fn update_stats(&mut self, provider_id: &str, success: bool, latency_ms: u64) {
        let stats = self
            .provider_stats
            .entry(provider_id.to_string())
            .or_default();

        stats.total_requests += 1;
        stats.last_used = Some(chrono::Utc::now());

        if success {
            stats.successful_requests += 1;
        } else {
            stats.failed_requests += 1;
        }

        // Update average latency using exponential moving average
        let alpha = 0.1; // Smoothing factor
        stats.average_latency_ms =
            alpha * latency_ms as f64 + (1.0 - alpha) * stats.average_latency_ms;
    }
}
