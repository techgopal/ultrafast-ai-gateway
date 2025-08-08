//! # Authentication and Authorization Module
//!
//! This module provides comprehensive authentication, authorization, and rate limiting
//! functionality for the Ultrafast Gateway. It supports multiple authentication methods,
//! session management, and sophisticated rate limiting with sliding windows.
//!
//! ## Overview
//!
//! The authentication system supports:
//! - **API Key Authentication**: Virtual API keys with user isolation
//! - **JWT Token Authentication**: Stateless authentication with token validation
//! - **Session Management**: Stateful sessions with automatic cleanup
//! - **Rate Limiting**: Per-user and per-provider rate limiting with sliding windows
//! - **Permission System**: Model-level access control and permissions
//!
//! ## Authentication Methods
//!
//! ### API Key Authentication
//!
//! Virtual API keys that map to internal user contexts with specific permissions
//! and rate limits. Keys can be scoped to specific models or providers.
//!
//! ### JWT Token Authentication
//!
//! JSON Web Tokens for stateless authentication. Tokens contain user information,
//! permissions, and rate limit data. Useful for distributed deployments.
//!
//! ### Session Management
//!
//! Stateful sessions stored in Redis or memory with automatic expiration
//! and cleanup. Sessions can be invalidated manually or automatically.
//!
//! ## Rate Limiting
//!
//! The rate limiting system uses sliding windows for accurate tracking:
//!
//! - **Per-Minute Limits**: Track requests and tokens per minute
//! - **Per-Hour Limits**: Track requests and tokens per hour
//! - **Sliding Windows**: Accurate rate limiting without fixed windows
//! - **Distributed Rate Limiting**: Redis-backed rate limiting for multi-instance deployments
//!
//! ## Usage
//!
//! ```rust
//! use ultrafast_gateway::auth::{AuthService, RateLimits};
//!
//! // Initialize authentication service
//! let auth_service = AuthService::new(config);
//!
//! // Validate API key
//! let auth_context = auth_service.validate_api_key("sk-...")?;
//!
//! // Check rate limits
//! let limits = RateLimits::new(100, 1000, 10000);
//! auth_service.check_rate_limits(&auth_context.user_id, limits).await?;
//! ```
//!
//! ## Security Features
//!
//! - **Virtual API Keys**: Internal mapping to user contexts
//! - **Rate Limiting**: Per-user and per-provider limits
//! - **Permission Scoping**: Model and provider-level access control
//! - **Session Security**: Secure session management with expiration
//! - **JWT Security**: Signed tokens with expiration validation
//!
//! ## Rate Limiting Algorithms
//!
//! ### Sliding Window Rate Limiting
//!
//! Uses a sliding window approach to provide accurate rate limiting:
//!
//! 1. **Window Tracking**: Maintains multiple time windows
//! 2. **Request Counting**: Counts requests within each window
//! 3. **Token Tracking**: Tracks token usage across windows
//! 4. **Automatic Cleanup**: Removes expired windows
//!
//! ### Distributed Rate Limiting
//!
//! For multi-instance deployments, rate limiting data is stored in Redis:
//!
//! - **Shared State**: Rate limits shared across instances
//! - **Atomic Operations**: Thread-safe rate limit updates
//! - **Automatic Cleanup**: Expired data automatically removed
//! - **Fallback**: In-memory rate limiting if Redis unavailable

use crate::config::AuthConfig;
use crate::gateway_caching::CacheManager;
use crate::gateway_error::GatewayError;
use dashmap::DashMap;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Authentication context containing user information and permissions.
///
/// This struct represents an authenticated user session with their
/// permissions, rate limits, and session metadata.
///
/// # Example
///
/// ```rust
/// let auth_context = AuthContext {
///     api_key: "sk-user-123".to_string(),
///     user_id: "user-456".to_string(),
///     permissions: vec!["chat:read".to_string(), "embedding:write".to_string()],
///     rate_limits: RateLimits::new(100, 1000, 10000),
///     metadata: HashMap::new(),
///     jwt_token: None,
///     session_expires_at: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    /// The API key used for authentication
    pub api_key: String,
    /// Unique user identifier
    pub user_id: String,
    /// List of permissions granted to this user
    pub permissions: Vec<String>,
    /// Rate limiting configuration and current state
    pub rate_limits: RateLimits,
    /// Additional metadata for the user session
    pub metadata: HashMap<String, String>,
    /// JWT token for stateless authentication (if applicable)
    pub jwt_token: Option<String>,
    /// Session expiration time (for stateful sessions)
    pub session_expires_at: Option<SystemTime>,
}

/// Rate limiting configuration and current state.
///
/// Tracks rate limits for requests and tokens with sliding window
/// implementation for accurate rate limiting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimits {
    /// Maximum requests allowed per minute
    pub requests_per_minute: u32,
    /// Maximum requests allowed per hour
    pub requests_per_hour: u32,
    /// Maximum tokens allowed per minute
    pub tokens_per_minute: u32,
    /// Current requests in the current minute window
    pub current_minute_requests: u32,
    /// Current requests in the current hour window
    pub current_hour_requests: u32,
    /// Current tokens in the current minute window
    pub current_minute_tokens: u32,
    /// Current minute window identifier
    pub minute_window: u64,
    /// Current hour window identifier
    pub hour_window: u64,
    /// Sliding windows for accurate rate limiting
    pub sliding_windows: Vec<SlidingWindow>,
}

/// Represents a single sliding window for rate limiting.
///
/// Each window tracks requests and tokens within a specific time period
/// to provide accurate rate limiting without fixed window boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlidingWindow {
    /// Start time of this window (Unix timestamp)
    pub window_start: u64,
    /// Number of requests in this window
    pub request_count: u32,
    /// Number of tokens consumed in this window
    pub token_count: u32,
}

/// Rate limiting state for a specific user.
///
/// Contains the current rate limiting state for a user, including
/// their limits, current usage, and last update time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitState {
    /// User identifier
    pub user_id: String,
    /// Current rate limiting configuration and state
    pub limits: RateLimits,
    /// Last time the rate limit state was updated
    pub last_updated: SystemTime,
    /// JWT secret for token validation (if applicable)
    pub jwt_secret: Option<String>,
}

/// JWT claims structure for token-based authentication.
///
/// Contains user information and permissions encoded in JWT tokens
/// for stateless authentication.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// Token expiration time (Unix timestamp)
    pub exp: u64,
    /// Token issued at time (Unix timestamp)
    pub iat: u64,
    /// User permissions
    pub permissions: Vec<String>,
    /// User rate limits
    pub rate_limits: RateLimits,
}

/// Global rate limiter storage for thread-safe access.
///
/// Uses `OnceLock` to ensure the rate limiter is initialized only once
/// and shared across all threads.
static RATE_LIMITER: OnceLock<Arc<RwLock<RateLimiter>>> = OnceLock::new();

/// Get the global rate limiter instance.
///
/// Returns a reference to the global rate limiter, initializing it
/// if it hasn't been initialized yet.
fn get_rate_limiter() -> &'static Arc<RwLock<RateLimiter>> {
    RATE_LIMITER.get_or_init(|| Arc::new(RwLock::new(RateLimiter::new())))
}

/// Rate limiter for managing user rate limits.
///
/// Provides thread-safe rate limiting with support for both in-memory
/// and Redis-backed storage for distributed deployments.
pub struct RateLimiter {
    /// Optional cache manager for Redis-backed rate limiting
    cache_manager: Option<Arc<CacheManager>>,
    /// In-memory rate limit state storage
    in_memory_state: DashMap<String, RateLimitState>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    /// Create a new rate limiter instance.
    ///
    /// Initializes an empty rate limiter with no cache manager.
    /// Use `initialize()` to set up Redis-backed rate limiting.
    pub fn new() -> Self {
        Self {
            cache_manager: None,
            in_memory_state: DashMap::new(),
        }
    }

    /// Initialize the rate limiter with a cache manager.
    ///
    /// Sets up Redis-backed rate limiting for distributed deployments.
    /// If no cache manager is provided, rate limiting will use in-memory storage only.
    ///
    /// # Arguments
    ///
    /// * `cache_manager` - Cache manager for Redis-backed rate limiting
    ///
    /// # Errors
    ///
    /// Returns an error if the rate limiter cannot be initialized.
    pub async fn initialize(cache_manager: Arc<CacheManager>) -> Result<(), GatewayError> {
        let rate_limiter = get_rate_limiter();
        let mut limiter = rate_limiter.write().await;
        limiter.cache_manager = Some(cache_manager);
        Ok(())
    }

    /// Get the current rate limit state for a user.
    ///
    /// Attempts to retrieve rate limit state from cache first, then falls back
    /// to in-memory storage. Returns `None` if no state exists.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user identifier
    ///
    /// # Returns
    ///
    /// Returns the rate limit state if found, `None` otherwise.
    async fn get_rate_limit_state(&mut self, user_id: &str) -> Option<RateLimitState> {
        let cache_key = format!("rate_limit:{user_id}");

        // Try cache first (Redis or memory)
        if let Some(cache_manager) = &self.cache_manager {
            if let Some(cached_value) = cache_manager.get(&cache_key).await {
                // Parse cached rate limit state
                if let Ok(state) = serde_json::from_value::<RateLimitState>(cached_value) {
                    return Some(state);
                }
            }
        }

        // Fall back to in-memory storage
        self.in_memory_state.get(user_id).map(|entry| entry.clone())
    }

    /// Set the rate limit state for a user.
    ///
    /// Attempts to set the rate limit state in cache first, then falls back
    /// to in-memory storage.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user identifier
    /// * `state` - The rate limit state to set
    async fn set_rate_limit_state(&mut self, user_id: &str, state: &RateLimitState) {
        let cache_key = format!("rate_limit:{user_id}");
        let ttl = Duration::from_secs(3600); // 1 hour TTL

        // Try cache first (Redis or memory)
        if let Some(cache_manager) = &self.cache_manager {
            if let Ok(state_value) = serde_json::to_value(state) {
                cache_manager.set(&cache_key, state_value, Some(ttl)).await;
                return;
            }
        }

        // Fallback to in-memory state
        self.in_memory_state
            .insert(user_id.to_string(), state.clone());
    }

    /// Cleanup old rate limit states to prevent memory bloat
    fn cleanup_old_rate_limit_states(&mut self) {
        let cutoff_time = SystemTime::now() - Duration::from_secs(7200); // 2 hours
        let initial_count = self.in_memory_state.len();

        // Remove entries older than cutoff time
        self.in_memory_state
            .retain(|_user_id, state| state.last_updated > cutoff_time);

        let cleaned_count = initial_count - self.in_memory_state.len();
        if cleaned_count > 0 {
            tracing::info!(
                "Cleaned up {} old rate limit states. Current count: {}",
                cleaned_count,
                self.in_memory_state.len()
            );
        }

        // If still too many entries, keep only the most recently updated ones
        if self.in_memory_state.len() > 5000 {
            let mut entries: Vec<(String, RateLimitState)> = self
                .in_memory_state
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().clone()))
                .collect();
            entries.sort_by(|a, b| b.1.last_updated.cmp(&a.1.last_updated));

            // Keep only the top 5000 most recent entries
            let to_keep: Vec<(String, RateLimitState)> = entries.into_iter().take(5000).collect();

            let removed_count = self.in_memory_state.len() - to_keep.len();
            self.in_memory_state.clear();
            for (k, v) in to_keep {
                self.in_memory_state.insert(k, v);
            }

            tracing::warn!(
                "Emergency cleanup removed {} rate limit states to prevent memory exhaustion. Current count: {}",
                removed_count,
                self.in_memory_state.len()
            );
        }
    }

    /// Check and update rate limits for a user.
    ///
    /// This function handles both in-memory and distributed rate limiting.
    /// It attempts to use Redis atomic counters if available, otherwise falls back
    /// to in-memory sliding window logic.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user identifier
    /// * `limits` - The current rate limits for the user
    ///
    /// # Returns
    ///
    /// Returns `Ok(updated_limits)` if within limits, `Err(GatewayError::RateLimit)` otherwise.
    pub async fn check_and_update_rate_limits(
        &mut self,
        user_id: &str,
        limits: RateLimits,
    ) -> Result<RateLimits, GatewayError> {
        // Distributed mode using Redis atomic counters if available
        if let Some(cache_manager) = &self.cache_manager {
            if cache_manager.has_redis() {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_secs();
                let current_minute = (now / 60) as usize;
                let current_hour = (now / 3600) as usize;

                let minute_key = format!("rl:req:m:{user_id}:{current_minute}");
                let hour_key = format!("rl:req:h:{user_id}:{current_hour}");

                // INCR with expiry atomically
                let minute_count = cache_manager
                    .incr_with_expiry(&minute_key, 60)
                    .await
                    .map_err(|e| GatewayError::RateLimit {
                        message: format!("Rate limit backend error: {e}"),
                    })?;
                if minute_count as u32 > limits.requests_per_minute {
                    return Err(GatewayError::RateLimit {
                        message: format!(
                            "Rate limit exceeded: {} requests per minute",
                            limits.requests_per_minute
                        ),
                    });
                }

                let hour_count = cache_manager
                    .incr_with_expiry(&hour_key, 3600)
                    .await
                    .map_err(|e| GatewayError::RateLimit {
                        message: format!("Rate limit backend error: {e}"),
                    })?;
                if hour_count as u32 > limits.requests_per_hour {
                    return Err(GatewayError::RateLimit {
                        message: format!(
                            "Rate limit exceeded: {} requests per hour",
                            limits.requests_per_hour
                        ),
                    });
                }

                // Return an updated snapshot-only struct
                let mut updated = limits.clone();
                updated.current_minute_requests = minute_count as u32;
                updated.current_hour_requests = hour_count as u32;
                updated.minute_window = now / 60;
                updated.hour_window = now / 3600;
                return Ok(updated);
            }
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        let current_minute = now / 60;
        let current_hour = now / 3600;

        // Get existing state or create new
        let mut state =
            self.get_rate_limit_state(user_id)
                .await
                .unwrap_or_else(|| RateLimitState {
                    user_id: user_id.to_string(),
                    limits: limits.clone(),
                    last_updated: SystemTime::now(),
                    jwt_secret: None,
                });

        // Reset counters if window has changed
        if state.limits.minute_window != current_minute {
            state.limits.current_minute_requests = 0;
            state.limits.current_minute_tokens = 0;
            state.limits.minute_window = current_minute;
        }
        if state.limits.hour_window != current_hour {
            state.limits.current_hour_requests = 0;
            state.limits.hour_window = current_hour;
        }

        // Check limits
        if state.limits.current_minute_requests >= state.limits.requests_per_minute {
            return Err(GatewayError::RateLimit {
                message: format!(
                    "Rate limit exceeded: {} requests per minute",
                    state.limits.requests_per_minute
                ),
            });
        }
        if state.limits.current_hour_requests >= state.limits.requests_per_hour {
            return Err(GatewayError::RateLimit {
                message: format!(
                    "Rate limit exceeded: {} requests per hour",
                    state.limits.requests_per_hour
                ),
            });
        }

        // Update counters
        state.limits.current_minute_requests += 1;
        state.limits.current_hour_requests += 1;
        state.last_updated = SystemTime::now();

        // Save updated state
        self.set_rate_limit_state(user_id, &state).await;

        Ok(state.limits)
    }

    /// Check and update token limits for a user.
    ///
    /// This function handles both in-memory and distributed token limiting.
    /// It attempts to use Redis atomic counters if available, otherwise falls back
    /// to in-memory sliding window logic.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user identifier
    /// * `tokens` - The number of tokens to add
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if within limits, `Err(GatewayError::RateLimit)` otherwise.
    pub async fn check_and_update_token_limits(
        &mut self,
        user_id: &str,
        tokens: u32,
    ) -> Result<(), GatewayError> {
        // Snapshot current per-minute token limit without overlapping borrows
        let current_limit = {
            let state = self.get_rate_limit_state(user_id).await;
            state
                .as_ref()
                .map(|s| s.limits.tokens_per_minute)
                .unwrap_or(60_000)
        };

        // Distributed mode using Redis atomic counters if available
        if let Some(cache_manager) = self.cache_manager.clone() {
            if cache_manager.has_redis() {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_secs();
                let current_minute = (now / 60) as usize;
                let key = format!("rl:tok:m:{user_id}:{current_minute}");

                let new_total = cache_manager
                    .incr_by_with_expiry(&key, tokens as i64, 60)
                    .await
                    .map_err(|e| GatewayError::RateLimit {
                        message: format!("Rate limit backend error: {e}"),
                    })?;

                if new_total as u32 > current_limit {
                    return Err(GatewayError::RateLimit {
                        message: format!(
                            "Token rate limit exceeded: {current_limit} tokens per minute"
                        ),
                    });
                }
                return Ok(());
            }
        }

        // Get existing state
        if let Some(mut state) = self.get_rate_limit_state(user_id).await {
            if state.limits.current_minute_tokens + tokens > { state.limits.tokens_per_minute } {
                return Err(GatewayError::RateLimit {
                    message: format!("Token rate limit exceeded: {} tokens per minute", {
                        state.limits.tokens_per_minute
                    }),
                });
            }

            state.limits.current_minute_tokens += tokens;
            state.last_updated = SystemTime::now();

            // Save updated state
            self.set_rate_limit_state(user_id, &state).await;
        }

        Ok(())
    }
}

impl RateLimits {
    /// Create a new rate limits configuration.
    ///
    /// Initializes rate limits with default values.
    ///
    /// # Arguments
    ///
    /// * `requests_per_minute` - Maximum requests per minute
    /// * `requests_per_hour` - Maximum requests per hour
    /// * `tokens_per_minute` - Maximum tokens per minute
    ///
    /// # Returns
    ///
    /// Returns a new `RateLimits` struct.
    pub fn new(requests_per_minute: u32, requests_per_hour: u32, tokens_per_minute: u32) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            requests_per_minute,
            requests_per_hour,
            tokens_per_minute,
            current_minute_requests: 0,
            current_hour_requests: 0,
            current_minute_tokens: 0,
            minute_window: now / 60,
            hour_window: now / 3600,
            sliding_windows: vec![SlidingWindow {
                window_start: now,
                request_count: 0,
                token_count: 0,
            }],
        }
    }

    /// Enhanced rate limiting with sliding windows.
    ///
    /// This method checks if the current request and token counts exceed
    /// the configured limits for the sliding window. It also tracks
    /// hourly limits across all sliding windows.
    ///
    /// # Arguments
    ///
    /// * `tokens` - The number of tokens consumed in the current request
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if within limits, `Err(String)` otherwise.
    pub fn check_sliding_window_limits(&mut self, tokens: u32) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Clean up old sliding windows
        self.sliding_windows.retain(|window| {
            now - window.window_start < 3600 // Keep only last hour
        });

        // Add new window if needed
        if self.sliding_windows.is_empty()
            || now - self.sliding_windows.last().unwrap().window_start >= 60
        {
            self.sliding_windows.push(SlidingWindow {
                window_start: now,
                request_count: 0,
                token_count: 0,
            });
        }

        // Update current window
        if let Some(current_window) = self.sliding_windows.last_mut() {
            current_window.request_count += 1;
            current_window.token_count += tokens;

            // Check limits
            if current_window.request_count > self.requests_per_minute {
                return Err("Rate limit exceeded: too many requests per minute".to_string());
            }

            if current_window.token_count > self.tokens_per_minute {
                return Err("Rate limit exceeded: too many tokens per minute".to_string());
            }
        }

        // Check hourly limits across all windows
        let total_hourly_requests: u32 = self
            .sliding_windows
            .iter()
            .filter(|w| now - w.window_start < 3600)
            .map(|w| w.request_count)
            .sum();

        if total_hourly_requests > self.requests_per_hour {
            return Err("Rate limit exceeded: too many requests per hour".to_string());
        }

        Ok(())
    }
}

/// Auth service for managing user sessions and API keys.
///
/// Manages user sessions, API key validation, and permission checking.
/// Uses a global instance for thread-safe access.
pub struct AuthService {
    /// Authentication configuration
    config: AuthConfig,
    /// Concurrent HashMap for user sessions
    sessions: DashMap<String, AuthContext>,
    /// Optional cache manager for Redis-backed sessions
    cache_manager: Option<Arc<CacheManager>>,
    /// JWT signing secret
    jwt_secret: String,
}

/// Global auth service instance for thread-safe access.
///
/// Uses `OnceLock` to ensure the auth service is initialized only once
/// and shared across all threads.
static AUTH_SERVICE: OnceLock<Arc<RwLock<AuthService>>> = OnceLock::new();

/// Get the global auth service instance.
///
/// Returns a reference to the global auth service, initializing it
/// if it hasn't been initialized yet.
fn get_auth_service() -> &'static Arc<RwLock<AuthService>> {
    AUTH_SERVICE.get_or_init(|| Arc::new(RwLock::new(AuthService::new_empty())))
}

impl AuthService {
    /// Create a new auth service instance.
    ///
    /// Initializes an empty auth service with default configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Authentication configuration
    ///
    /// # Returns
    ///
    /// Returns a new `AuthService` struct.
    pub fn new(config: AuthConfig) -> Self {
        let jwt_secret = std::env::var("GATEWAY_JWT_SECRET")
            .unwrap_or_else(|_| "ultrafast-gateway-secret-key".to_string());

        Self {
            config,
            sessions: DashMap::new(),
            cache_manager: None,
            jwt_secret,
        }
    }

    /// Create a new auth service instance with default empty configuration.
    ///
    /// Used for initializing the global instance.
    ///
    /// # Returns
    ///
    /// Returns an `AuthService` struct with default empty configuration.
    pub fn new_empty() -> Self {
        Self {
            config: AuthConfig {
                enabled: false,
                api_keys: vec![],
                rate_limiting: crate::config::RateLimitConfig {
                    requests_per_minute: 60,
                    requests_per_hour: 1000,
                    tokens_per_minute: 10000,
                },
            },
            sessions: DashMap::new(),
            cache_manager: None,
            jwt_secret: "ultrafast-gateway-secret-key".to_string(),
        }
    }

    /// Initialize the global auth service with configuration and cache manager.
    ///
    /// Sets up the global auth service with the provided configuration
    /// and cache manager for distributed deployments.
    ///
    /// # Arguments
    ///
    /// * `config` - Authentication configuration
    /// * `cache_manager` - Cache manager for Redis-backed sessions
    ///
    /// # Errors
    ///
    /// Returns an error if the auth service cannot be initialized.
    pub async fn initialize_global(config: AuthConfig, cache_manager: Arc<CacheManager>) {
        let auth_service = get_auth_service();
        let mut service = auth_service.write().await;
        service.config = config;
        service.cache_manager = Some(cache_manager);
    }

    /// Enhanced API key validation with JWT support.
    ///
    /// Attempts to validate an API key as a JWT token first, then falls back
    /// to traditional API key validation.
    ///
    /// # Arguments
    ///
    /// * `api_key` - The API key or JWT token to validate
    ///
    /// # Returns
    ///
    /// Returns `Ok(AuthContext)` if valid, `Err(GatewayError::Auth)` otherwise.
    pub fn validate_api_key(&self, api_key: &str) -> Result<AuthContext, GatewayError> {
        // Try parsing as JWT first; avoid brittle prefix heuristics
        if let Ok(ctx) = self.validate_jwt_token(api_key) {
            return Ok(ctx);
        }

        // Traditional API key validation
        for api_key_config in &self.config.api_keys {
            if api_key_config.key == api_key && api_key_config.enabled {
                let user_id = api_key
                    .split('-')
                    .next_back()
                    .unwrap_or("unknown")
                    .to_string();

                let rate_limits = api_key_config
                    .rate_limit
                    .clone()
                    .map(|rl| {
                        RateLimits::new(
                            rl.requests_per_minute,
                            rl.requests_per_hour,
                            rl.tokens_per_minute,
                        )
                    })
                    .unwrap_or_else(|| RateLimits::new(100, 1000, 10000));

                return Ok(AuthContext {
                    api_key: api_key.to_string(),
                    user_id,
                    permissions: vec!["read".to_string(), "write".to_string()],
                    rate_limits,
                    metadata: api_key_config.metadata.clone(),
                    jwt_token: None,
                    session_expires_at: None,
                });
            }
        }

        Err(GatewayError::Auth {
            message: "Invalid API key".to_string(),
        })
    }

    /// JWT token validation.
    ///
    /// Validates a JWT token and extracts user information and permissions.
    ///
    /// # Arguments
    ///
    /// * `token` - The JWT token to validate
    ///
    /// # Returns
    ///
    /// Returns `Ok(AuthContext)` if valid, `Err(GatewayError::Auth)` otherwise.
    pub fn validate_jwt_token(&self, token: &str) -> Result<AuthContext, GatewayError> {
        // Harden validation: default to HS256, disable "none" algs
        let mut validation = Validation::default();
        validation.validate_exp = true;
        validation.algorithms = vec![jsonwebtoken::Algorithm::HS256];

        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_ref()),
            &validation,
        )
        .map_err(|e| GatewayError::Auth {
            message: format!("Invalid JWT token: {e}"),
        })?;

        let claims = token_data.claims;

        // Check if token is expired
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if claims.exp < now {
            return Err(GatewayError::Auth {
                message: "JWT token expired".to_string(),
            });
        }

        Ok(AuthContext {
            api_key: token.to_string(),
            user_id: claims.sub,
            permissions: claims.permissions,
            rate_limits: claims.rate_limits,
            metadata: HashMap::new(),
            jwt_token: Some(token.to_string()),
            session_expires_at: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(claims.exp)),
        })
    }

    /// Validate auth configuration sanity (e.g., secret strength) and warn/fail in risky setups
    ///
    /// Checks if the JWT secret is default and warns if it is.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if configuration is sane, `Err(GatewayError::Auth)` otherwise.
    pub fn sanity_check(&self) -> Result<(), GatewayError> {
        if self.config.enabled {
            // Disallow default secret in enabled auth mode
            if self.jwt_secret == "ultrafast-gateway-secret-key" {
                return Err(GatewayError::Auth {
                    message:
                        "Insecure GATEWAY_JWT_SECRET; set a strong secret when auth is enabled"
                            .to_string(),
                });
            }
        }
        Ok(())
    }

    /// Generate a JWT token for stateless authentication.
    ///
    /// Creates a new JWT token with a 1-hour expiration.
    ///
    /// # Arguments
    ///
    /// * `auth_context` - The authentication context for which to generate the token
    ///
    /// # Returns
    ///
    /// Returns `Ok(String)` if successful, `Err(GatewayError::Auth)` otherwise.
    pub fn generate_jwt_token(&self, auth_context: &AuthContext) -> Result<String, GatewayError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let exp = now + 3600; // 1 hour expiration

        let claims = Claims {
            sub: auth_context.user_id.clone(),
            exp,
            iat: now,
            permissions: auth_context.permissions.clone(),
            rate_limits: auth_context.rate_limits.clone(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_ref()),
        )
        .map_err(|e| GatewayError::Auth {
            message: format!("Failed to generate JWT token: {e}"),
        })
    }

    /// Enhanced session management with expiration.
    ///
    /// Creates a new session ID, sets its expiration, and stores it in memory
    /// and optionally in cache.
    ///
    /// # Arguments
    ///
    /// * `auth_context` - The authentication context for the session
    ///
    /// # Returns
    ///
    /// Returns the session ID as a string.
    pub async fn create_session(&mut self, auth_context: AuthContext) -> String {
        let session_id = Uuid::new_v4().to_string();
        let expires_at = SystemTime::now() + Duration::from_secs(3600); // 1 hour session

        let mut session = auth_context;
        session.session_expires_at = Some(expires_at);

        self.sessions.insert(session_id.clone(), session.clone());

        // Store in cache if available
        if let Some(cache_manager) = &self.cache_manager {
            let cache_key = format!("session:{session_id}");
            if let Ok(session_value) = serde_json::to_value(&session) {
                cache_manager
                    .set(&cache_key, session_value, Some(Duration::from_secs(3600)))
                    .await;
            }
        }

        session_id
    }

    /// Enhanced session retrieval with expiration check.
    ///
    /// Retrieves a session from memory and cache, checking for expiration.
    ///
    /// # Arguments
    ///
    /// * `session_id` - The ID of the session to retrieve
    ///
    /// # Returns
    ///
    /// Returns the session if found and not expired, `None` otherwise.
    pub async fn get_session(&self, session_id: &str) -> Option<AuthContext> {
        // Check cache first
        if let Some(cache_manager) = &self.cache_manager {
            let cache_key = format!("session:{session_id}");
            if let Some(cached_value) = cache_manager.get(&cache_key).await {
                if let Ok(session) = serde_json::from_value::<AuthContext>(cached_value) {
                    // Check if session is expired
                    if let Some(expires_at) = session.session_expires_at {
                        if SystemTime::now() < expires_at {
                            return Some(session);
                        } else {
                            // Session expired, remove from cache
                            cache_manager.invalidate(&cache_key).await;
                        }
                    }
                }
            }
        }

        // Fallback to in-memory sessions
        if let Some(session) = self.sessions.get(session_id) {
            let session = session.clone();

            // Check if session is expired
            if let Some(expires_at) = session.session_expires_at {
                if SystemTime::now() < expires_at {
                    return Some(session);
                } else {
                    // Session expired, remove from memory
                    self.sessions.remove(session_id);
                }
            }
        }

        None
    }

    /// Enhanced session invalidation.
    ///
    /// Invalidates a session by removing it from memory and cache.
    ///
    /// # Arguments
    ///
    /// * `session_id` - The ID of the session to invalidate
    pub async fn invalidate_session(&mut self, session_id: &str) {
        // Remove from memory
        self.sessions.remove(session_id);

        // Remove from cache
        if let Some(cache_manager) = &self.cache_manager {
            let cache_key = format!("session:{session_id}");
            cache_manager.invalidate(&cache_key).await;
        }
    }

    /// Clean up expired sessions.
    ///
    /// Iterates through sessions and removes those that have expired.
    fn cleanup_expired_sessions(&mut self) {
        let now = SystemTime::now();
        let expired_sessions: Vec<String> = self
            .sessions
            .iter()
            .filter_map(|entry| {
                if let Some(expires_at) = entry.session_expires_at {
                    if now >= expires_at {
                        Some(entry.key().clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        for session_id in &expired_sessions {
            self.sessions.remove(session_id);
        }

        if !expired_sessions.is_empty() {
            tracing::info!("Cleaned up {} expired sessions", expired_sessions.len());
        }
    }

    /// Enhanced model permission checking.
    ///
    /// Checks if the user has specific model permissions in their metadata
    /// or if they have a default "write" permission.
    ///
    /// # Arguments
    ///
    /// * `auth_context` - The authentication context
    /// * `model` - The model name to check
    ///
    /// # Returns
    ///
    /// Returns `true` if the user has permission, `false` otherwise.
    pub fn check_model_permission(&self, auth_context: &AuthContext, model: &str) -> bool {
        // Check if user has specific model permissions
        if let Some(allowed_models) = &auth_context.metadata.get("allowed_models") {
            if let Ok(models) = serde_json::from_str::<Vec<String>>(allowed_models) {
                return models.contains(&model.to_string());
            }
        }

        // Default permission check
        auth_context.permissions.contains(&"write".to_string())
    }

    /// Enhanced API key extraction with JWT support.
    ///
    /// Extracts the API key from an HTTP authorization header.
    /// Supports "Bearer" and "sk-" prefixes.
    ///
    /// # Arguments
    ///
    /// * `auth_header` - The optional HTTP authorization header
    ///
    /// # Returns
    ///
    /// Returns the extracted API key if found, `None` otherwise.
    pub fn extract_api_key_from_header(auth_header: Option<&str>) -> Option<String> {
        auth_header.and_then(|header| {
            if let Some(stripped) = header.strip_prefix("Bearer ") {
                Some(stripped.to_string())
            } else if header.starts_with("sk-") {
                Some(header.to_string())
            } else {
                None
            }
        })
    }

    /// Check if authentication is enabled.
    ///
    /// # Returns
    ///
    /// Returns `true` if authentication is enabled, `false` otherwise.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

// Public API functions for rate limiting
/// Initialize the global rate limiter.
///
/// Sets up Redis-backed rate limiting for distributed deployments.
///
/// # Arguments
///
/// * `cache_manager` - Cache manager for Redis-backed rate limiting
///
/// # Errors
///
/// Returns an error if the rate limiter cannot be initialized.
pub async fn initialize_rate_limiter(cache_manager: Arc<CacheManager>) -> Result<(), GatewayError> {
    RateLimiter::initialize(cache_manager).await
}

/// Check and update rate limits for a user.
///
/// This function handles both in-memory and distributed rate limiting.
///
/// # Arguments
///
/// * `user_id` - The user identifier
/// * `limits` - The current rate limits for the user
///
/// # Returns
///
/// Returns `Ok(updated_limits)` if within limits, `Err(GatewayError::RateLimit)` otherwise.
pub async fn check_rate_limits(
    user_id: &str,
    limits: RateLimits,
) -> Result<RateLimits, GatewayError> {
    let rate_limiter = get_rate_limiter();
    let mut limiter = rate_limiter.write().await;
    limiter.check_and_update_rate_limits(user_id, limits).await
}

/// Check and update token limits for a user.
///
/// This function handles both in-memory and distributed token limiting.
///
/// # Arguments
///
/// * `user_id` - The user identifier
/// * `tokens` - The number of tokens to add
///
/// # Returns
///
/// Returns `Ok(())` if within limits, `Err(GatewayError::RateLimit)` otherwise.
pub async fn check_token_limits(user_id: &str, tokens: u32) -> Result<(), GatewayError> {
    let rate_limiter = get_rate_limiter();
    let mut limiter = rate_limiter.write().await;
    limiter.check_and_update_token_limits(user_id, tokens).await
}

// Global auth service API
/// Initialize the global auth service.
///
/// Sets up the global auth service with the provided configuration
/// and cache manager for distributed deployments.
///
/// # Arguments
///
/// * `config` - Authentication configuration
/// * `cache_manager` - Cache manager for Redis-backed sessions
///
/// # Errors
///
/// Returns an error if the auth service cannot be initialized.
pub async fn initialize_auth_service(config: AuthConfig, cache_manager: Arc<CacheManager>) {
    AuthService::initialize_global(config, cache_manager).await;

    // Start background cleanup task for auth service
    start_auth_cleanup_task().await;
}

async fn start_auth_cleanup_task() {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1800)); // 30 minutes

        loop {
            interval.tick().await;

            // Cleanup auth service sessions
            {
                let auth_service = get_auth_service();
                let mut service = auth_service.write().await;
                if service.sessions.len() > 10000 {
                    service.cleanup_expired_sessions();
                }
            }

            // Cleanup rate limiter states
            {
                let rate_limiter = get_rate_limiter();
                let mut limiter = rate_limiter.write().await;
                if limiter.in_memory_state.len() > 5000 {
                    limiter.cleanup_old_rate_limit_states();
                }
            }

            tracing::debug!("Auth background cleanup completed");
        }
    });
}

/// Validate an API key globally.
///
/// Attempts to validate an API key using the global auth service.
///
/// # Arguments
///
/// * `api_key` - The API key or JWT token to validate
///
/// # Returns
///
/// Returns `Ok(AuthContext)` if valid, `Err(GatewayError::Auth)` otherwise.
pub async fn validate_api_key_global(api_key: &str) -> Result<AuthContext, GatewayError> {
    let auth_service = get_auth_service();
    let service = auth_service.read().await;
    service.validate_api_key(api_key)
}

/// Create a new session globally.
///
/// Creates a new session using the global auth service.
///
/// # Arguments
///
/// * `auth_context` - The authentication context for the session
///
/// # Returns
///
/// Returns the session ID as a string.
pub async fn create_session_global(auth_context: AuthContext) -> String {
    let auth_service = get_auth_service();
    let mut service = auth_service.write().await;
    service.create_session(auth_context).await
}

/// Retrieve a session globally.
///
/// Retrieves a session from the global auth service.
///
/// # Arguments
///
/// * `session_id` - The ID of the session to retrieve
///
/// # Returns
///
/// Returns the session if found and not expired, `None` otherwise.
pub async fn get_session_global(session_id: &str) -> Option<AuthContext> {
    let auth_service = get_auth_service();
    let service = auth_service.read().await;
    service.get_session(session_id).await
}

/// Invalidate a session globally.
///
/// Invalidates a session by removing it from the global auth service.
///
/// # Arguments
///
/// * `session_id` - The ID of the session to invalidate
pub async fn invalidate_session_global(session_id: &str) {
    let auth_service = get_auth_service();
    let mut service = auth_service.write().await;
    service.invalidate_session(session_id).await
}
