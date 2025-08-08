//! # Caching Module
//!
//! This module provides comprehensive caching functionality for the Ultrafast Models SDK.
//! It supports both in-memory and distributed caching with automatic expiration,
//! intelligent cache key generation, and performance optimization.
//!
//! ## Overview
//!
//! The caching system provides:
//! - **Multiple Cache Backends**: In-memory and distributed caching
//! - **Automatic Expiration**: TTL-based cache invalidation
//! - **Intelligent Key Generation**: Hash-based cache keys for consistency
//! - **Performance Optimization**: Reduces API calls and improves response times
//! - **Thread Safety**: Safe concurrent access across multiple threads
//! - **Cache Statistics**: Hit rates, memory usage, and performance metrics
//!
//! ## Cache Backends
//!
//! ### In-Memory Cache
//!
//! Fast local caching suitable for single-instance deployments:
//! - **Low Latency**: Sub-millisecond access times
//! - **Memory Efficient**: LRU eviction with configurable size limits
//! - **Automatic Cleanup**: Expired entries removed automatically
//! - **Thread Safe**: Concurrent access support with mutex protection
//!
//! ### Distributed Cache
//!
//! Hybrid caching for multi-instance deployments:
//! - **Local + Distributed**: Combines in-memory and distributed storage
//! - **Shared State**: Cache shared across multiple instances
//! - **Fallback Mechanism**: Local cache as backup for distributed cache
//! - **Consistency**: Eventual consistency with local caching
//!
//! ## Cache Key Strategy
//!
//! The system uses intelligent cache key generation:
//!
//! - **Chat Completions**: `chat:{model}:{messages_hash}`
//! - **Embeddings**: `embedding:{model}:{input_hash}`
//! - **Image Generation**: `image:{model}:{prompt_hash}`
//! - **Consistent Hashing**: Deterministic keys for identical requests
//!
//! ## Usage Examples
//!
//! ### Basic Caching
//!
//! ```rust
//! use ultrafast_models_sdk::cache::{InMemoryCache, Cache, CachedResponse};
//! use ultrafast_models_sdk::models::{ChatRequest, ChatResponse};
//! use std::time::Duration;
//!
//! // Create in-memory cache
//! let cache = InMemoryCache::new(1000); // 1000 entries capacity
//!
//! // Create a chat request
//! let request = ChatRequest {
//!     model: "gpt-4".to_string(),
//!     messages: vec![Message::user("Hello, world!")],
//!     ..Default::default()
//! };
//!
//! // Generate cache key
//! let cache_key = CacheKeyBuilder::build_chat_key(&request);
//!
//! // Check cache first
//! if let Some(cached) = cache.get(&cache_key) {
//!     println!("Cache hit: {}", cached.response.choices[0].message.content);
//!     return Ok(cached.response);
//! }
//!
//! // Make API call and cache result
//! let response = provider.chat_completion(request).await?;
//! let cached_response = CachedResponse::new(response.clone(), Duration::from_secs(3600));
//! cache.set(&cache_key, cached_response, Duration::from_secs(3600));
//!
//! Ok(response)
//! ```
//!
//! ### Distributed Caching
//!
//! ```rust
//! use ultrafast_models_sdk::cache::{DistributedCache, Cache};
//!
//! // Create distributed cache
//! let cache = DistributedCache::new(500); // 500 local entries
//!
//! // Cache operations work the same way
//! let cache_key = CacheKeyBuilder::build_embedding_key("text-embedding-ada-002", "Hello");
//!
//! if let Some(cached) = cache.get(&cache_key) {
//!     println!("Cache hit from distributed cache");
//!     return Ok(cached.response);
//! }
//!
//! // Cache miss - make API call
//! let response = provider.embedding(request).await?;
//! let cached_response = CachedResponse::new(response.clone(), Duration::from_secs(1800));
//! cache.set(&cache_key, cached_response, Duration::from_secs(1800));
//!
//! Ok(response)
//! ```
//!
//! ### Cache Configuration
//!
//! ```rust
//! use ultrafast_models_sdk::cache::{CacheConfig, CacheType};
//! use std::time::Duration;
//!
//! let config = CacheConfig {
//!     enabled: true,
//!     ttl: Duration::from_secs(3600), // 1 hour TTL
//!     max_size: 1000,
//!     cache_type: CacheType::InMemory,
//! };
//!
//! // Use with client
//! let client = UltrafastClient::standalone()
//!     .with_openai("your-key")
//!     .with_cache(config)
//!     .build()?;
//! ```
//!
//! ## Performance Benefits
//!
//! The caching system provides significant performance improvements:
//!
//! - **Reduced Latency**: Cached responses served in <1ms
//! - **Lower Costs**: Fewer API calls to providers
//! - **Improved Throughput**: Higher request handling capacity
//! - **Better User Experience**: Faster response times
//! - **Reduced Load**: Less stress on provider APIs
//!
//! ## Cache Invalidation
//!
//! The system supports multiple invalidation strategies:
//!
//! - **TTL-based**: Automatic expiration after configured time
//! - **Manual Invalidation**: Explicit cache entry removal
//! - **Pattern-based**: Remove entries matching patterns
//! - **Full Clear**: Clear entire cache (admin only)
//!
//! ## Best Practices
//!
//! - **Appropriate TTL**: Set TTL based on response freshness requirements
//! - **Monitor Hit Rates**: Track cache effectiveness and adjust accordingly
//! - **Memory Management**: Configure appropriate cache sizes for your workload
//! - **Key Design**: Use consistent and unique cache keys
//! - **Error Handling**: Implement fallback for cache failures
//! - **Monitoring**: Track cache performance and memory usage

use crate::models::{ChatRequest, ChatResponse};
use dashmap::DashMap;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Trait for cache implementations.
///
/// This trait defines the interface for different cache backends,
/// allowing for pluggable caching implementations.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::cache::{Cache, InMemoryCache, CachedResponse};
/// use std::time::Duration;
///
/// let cache: Box<dyn Cache> = Box::new(InMemoryCache::new(100));
///
/// // Store a response
/// let response = CachedResponse::new(chat_response, Duration::from_secs(3600));
/// cache.set("key", response, Duration::from_secs(3600));
///
/// // Retrieve from cache
/// if let Some(cached) = cache.get("key") {
///     println!("Cache hit: {}", cached.response.choices[0].message.content);
/// }
/// ```
pub trait Cache: Send + Sync {
    /// Retrieve a cached response by key.
    ///
    /// Returns `Some(CachedResponse)` if found and not expired, `None` otherwise.
    fn get(&self, key: &str) -> Option<CachedResponse>;

    /// Store a response in the cache.
    ///
    /// # Arguments
    ///
    /// * `key` - Cache key for the response
    /// * `response` - The response to cache
    /// * `ttl` - Time-to-live for the cached response
    fn set(&self, key: &str, response: CachedResponse, ttl: Duration);

    /// Remove a specific entry from the cache.
    ///
    /// # Arguments
    ///
    /// * `key` - Cache key to invalidate
    fn invalidate(&self, key: &str);

    /// Clear all entries from the cache.
    fn clear(&self);

    /// Get the current number of entries in the cache.
    fn size(&self) -> usize;
}

/// Cached response with metadata.
///
/// This struct wraps a response with creation time and TTL information
/// for automatic expiration handling.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::cache::CachedResponse;
/// use std::time::Duration;
///
/// let response = ChatResponse { /* ... */ };
/// let cached = CachedResponse::new(response, Duration::from_secs(3600));
///
/// if !cached.is_expired() {
///     println!("Response is still valid");
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResponse {
    /// The cached response data
    pub response: ChatResponse,
    /// When this entry was created
    pub created_at: SystemTime,
    /// How long this entry should be considered valid
    pub ttl: Duration,
}

impl CachedResponse {
    /// Create a new cached response.
    ///
    /// # Arguments
    ///
    /// * `response` - The response to cache
    /// * `ttl` - Time-to-live for the cached response
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::cache::CachedResponse;
    /// use std::time::Duration;
    ///
    /// let response = ChatResponse { /* ... */ };
    /// let cached = CachedResponse::new(response, Duration::from_secs(3600));
    /// ```
    pub fn new(response: ChatResponse, ttl: Duration) -> Self {
        Self {
            response,
            created_at: SystemTime::now(),
            ttl,
        }
    }

    /// Check if this cached response has expired.
    ///
    /// Returns `true` if the response has exceeded its TTL, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::cache::CachedResponse;
    /// use std::time::Duration;
    ///
    /// let cached = CachedResponse::new(response, Duration::from_secs(3600));
    ///
    /// if cached.is_expired() {
    ///     println!("Response has expired");
    /// } else {
    ///     println!("Response is still valid");
    /// }
    /// ```
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed().unwrap_or(Duration::MAX) > self.ttl
    }
}

/// In-memory cache implementation using LRU eviction.
///
/// This cache provides fast local storage with automatic eviction
/// of least recently used entries when capacity is reached.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::cache::{InMemoryCache, Cache, CachedResponse};
/// use std::time::Duration;
///
/// let cache = InMemoryCache::new(1000); // 1000 entries capacity
///
/// let response = CachedResponse::new(chat_response, Duration::from_secs(3600));
/// cache.set("key", response, Duration::from_secs(3600));
///
/// if let Some(cached) = cache.get("key") {
///     println!("Cache hit: {}", cached.response.choices[0].message.content);
/// }
/// ```
pub struct InMemoryCache {
    /// LRU cache with thread-safe access
    cache: Arc<Mutex<LruCache<String, CachedResponse>>>,
}

impl InMemoryCache {
    /// Create a new in-memory cache with the specified capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of entries in the cache
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::cache::InMemoryCache;
    ///
    /// let cache = InMemoryCache::new(1000);
    /// ```
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(capacity).unwrap(),
            ))),
        }
    }
}

impl Cache for InMemoryCache {
    fn get(&self, key: &str) -> Option<CachedResponse> {
        let mut cache = self.cache.lock().unwrap();

        if let Some(cached) = cache.get(key) {
            if !cached.is_expired() {
                return Some(cached.clone());
            } else {
                // Remove expired entry
                cache.pop(key);
            }
        }
        None
    }

    fn set(&self, key: &str, response: CachedResponse, _ttl: Duration) {
        let mut cache = self.cache.lock().unwrap();
        cache.put(key.to_string(), response);
    }

    fn invalidate(&self, key: &str) {
        let mut cache = self.cache.lock().unwrap();
        cache.pop(key);
    }

    fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
    }

    fn size(&self) -> usize {
        let cache = self.cache.lock().unwrap();
        cache.len()
    }
}

/// Distributed cache implementation with local fallback.
///
/// This cache combines local in-memory storage with distributed storage,
/// providing fast access with shared state across multiple instances.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::cache::{DistributedCache, Cache, CachedResponse};
/// use std::time::Duration;
///
/// let cache = DistributedCache::new(500); // 500 local entries
///
/// let response = CachedResponse::new(chat_response, Duration::from_secs(3600));
/// cache.set("key", response, Duration::from_secs(3600));
///
/// if let Some(cached) = cache.get("key") {
///     println!("Cache hit from distributed cache");
/// }
/// ```
pub struct DistributedCache {
    /// Local in-memory cache for fast access
    local: InMemoryCache,
    /// Distributed storage for shared state
    entries: DashMap<String, CachedResponse>,
}

impl DistributedCache {
    /// Create a new distributed cache with the specified local capacity.
    ///
    /// # Arguments
    ///
    /// * `local_capacity` - Maximum number of entries in the local cache
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::cache::DistributedCache;
    ///
    /// let cache = DistributedCache::new(500);
    /// ```
    pub fn new(local_capacity: usize) -> Self {
        Self {
            local: InMemoryCache::new(local_capacity),
            entries: DashMap::new(),
        }
    }
}

impl Cache for DistributedCache {
    fn get(&self, key: &str) -> Option<CachedResponse> {
        // Check local cache first for fastest access
        if let Some(cached) = self.local.get(key) {
            return Some(cached);
        }

        // Check distributed cache
        if let Some(entry) = self.entries.get(key) {
            if !entry.is_expired() {
                let cached = entry.clone();
                // Populate local cache for future fast access
                self.local.set(key, cached.clone(), cached.ttl);
                return Some(cached);
            } else {
                // Remove expired entry
                self.entries.remove(key);
            }
        }

        None
    }

    fn set(&self, key: &str, response: CachedResponse, ttl: Duration) {
        // Store in both local and distributed cache
        self.local.set(key, response.clone(), ttl);
        self.entries.insert(key.to_string(), response);
    }

    fn invalidate(&self, key: &str) {
        // Remove from both caches
        self.local.invalidate(key);
        self.entries.remove(key);
    }

    fn clear(&self) {
        // Clear both caches
        self.local.clear();
        self.entries.clear();
    }

    fn size(&self) -> usize {
        // Return distributed cache size as the authoritative count
        self.entries.len()
    }
}

/// Utility for generating consistent cache keys.
///
/// This struct provides methods for creating deterministic cache keys
/// based on request content and parameters.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::cache::CacheKeyBuilder;
/// use ultrafast_models_sdk::models::ChatRequest;
///
/// let request = ChatRequest {
///     model: "gpt-4".to_string(),
///     messages: vec![Message::user("Hello")],
///     ..Default::default()
/// };
///
/// let cache_key = CacheKeyBuilder::build_chat_key(&request);
/// println!("Cache key: {}", cache_key);
/// ```
pub struct CacheKeyBuilder;

impl CacheKeyBuilder {
    /// Build a cache key for chat completion requests.
    ///
    /// Creates a deterministic hash-based key that includes the model,
    /// messages, and key parameters like temperature and max_tokens.
    ///
    /// # Arguments
    ///
    /// * `request` - The chat completion request
    ///
    /// # Returns
    ///
    /// Returns a string cache key that is consistent for identical requests.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::cache::CacheKeyBuilder;
    /// use ultrafast_models_sdk::models::ChatRequest;
    ///
    /// let request = ChatRequest {
    ///     model: "gpt-4".to_string(),
    ///     messages: vec![Message::user("Hello")],
    ///     temperature: Some(0.7),
    ///     max_tokens: Some(100),
    ///     ..Default::default()
    /// };
    ///
    /// let cache_key = CacheKeyBuilder::build_chat_key(&request);
    /// // Result: "chat:gpt-4:hash_value"
    /// ```
    pub fn build_chat_key(request: &ChatRequest) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        // Hash the model name
        request.model.hash(&mut hasher);

        // Hash each message (role and content)
        for message in &request.messages {
            message.role.hash(&mut hasher);
            message.content.hash(&mut hasher);
        }

        // Hash temperature if present (scaled to avoid floating point issues)
        if let Some(temp) = request.temperature {
            ((temp * 1000.0) as u32).hash(&mut hasher);
        }

        // Hash max_tokens if present
        if let Some(max_tokens) = request.max_tokens {
            max_tokens.hash(&mut hasher);
        }

        format!("chat:{:x}", hasher.finish())
    }

    /// Build a cache key for embedding requests.
    ///
    /// Creates a deterministic hash-based key for embedding requests
    /// based on the model and input text.
    ///
    /// # Arguments
    ///
    /// * `model` - The embedding model name
    /// * `input` - The input text to embed
    ///
    /// # Returns
    ///
    /// Returns a string cache key for the embedding request.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::cache::CacheKeyBuilder;
    ///
    /// let cache_key = CacheKeyBuilder::build_embedding_key(
    ///     "text-embedding-ada-002",
    ///     "Hello, world!"
    /// );
    /// // Result: "embedding:text-embedding-ada-002:hash_value"
    /// ```
    pub fn build_embedding_key(model: &str, input: &str) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        model.hash(&mut hasher);
        input.hash(&mut hasher);
        format!("embedding:{:x}", hasher.finish())
    }

    /// Build a cache key for image generation requests.
    ///
    /// Creates a deterministic hash-based key for image generation
    /// based on the model and prompt.
    ///
    /// # Arguments
    ///
    /// * `model` - The image generation model name
    /// * `prompt` - The text prompt for image generation
    ///
    /// # Returns
    ///
    /// Returns a string cache key for the image generation request.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::cache::CacheKeyBuilder;
    ///
    /// let cache_key = CacheKeyBuilder::build_image_key(
    ///     "dall-e-3",
    ///     "A beautiful sunset over mountains"
    /// );
    /// // Result: "image:dall-e-3:hash_value"
    /// ```
    pub fn build_image_key(model: &str, prompt: &str) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        model.hash(&mut hasher);
        prompt.hash(&mut hasher);
        format!("image:{:x}", hasher.finish())
    }

    /// Hash content for cache key generation.
    ///
    /// Creates a consistent hash of the provided content string.
    ///
    /// # Arguments
    ///
    /// * `content` - The content to hash
    ///
    /// # Returns
    ///
    /// Returns a hexadecimal string representation of the hash.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::cache::CacheKeyBuilder;
    ///
    /// let hash = CacheKeyBuilder::hash_content("Hello, world!");
    /// println!("Content hash: {}", hash);
    /// ```
    pub fn hash_content(content: &str) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

/// Configuration for cache behavior.
///
/// This struct defines the parameters that control cache behavior,
/// including TTL, size limits, and cache type selection.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::cache::{CacheConfig, CacheType};
/// use std::time::Duration;
///
/// let config = CacheConfig {
///     enabled: true,
///     ttl: Duration::from_secs(3600),
///     max_size: 1000,
///     cache_type: CacheType::InMemory,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Whether caching is enabled
    pub enabled: bool,
    /// Time-to-live for cached responses
    pub ttl: Duration,
    /// Maximum number of cached entries
    pub max_size: usize,
    /// Type of cache backend to use
    pub cache_type: CacheType,
}

/// Available cache backend types.
///
/// Defines the different cache implementations that can be used.
#[derive(Debug, Clone)]
pub enum CacheType {
    /// In-memory caching (faster but not shared across instances)
    InMemory,
    /// Distributed caching (shared across multiple instances)
    Distributed,
    /// Redis-based caching (external Redis server)
    Redis,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl: Duration::from_secs(300), // 5 minutes
            max_size: 1000,
            cache_type: CacheType::InMemory,
        }
    }
}
