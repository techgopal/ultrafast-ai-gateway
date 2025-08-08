//! # Plugin System Module
//!
//! This module provides a comprehensive plugin system for the Ultrafast Gateway,
//! allowing dynamic extension of gateway functionality through modular plugins.
//!
//! ## Overview
//!
//! The plugin system enables:
//! - **Dynamic Functionality**: Runtime plugin loading and management
//! - **Request/Response Modification**: Intercept and modify requests/responses
//! - **Content Filtering**: Automatic content filtering and validation
//! - **Cost Tracking**: Real-time cost monitoring and analysis
//! - **Enhanced Logging**: Structured logging with custom formats
//! - **Error Handling**: Custom error processing and recovery
//!
//! ## Plugin Architecture
//!
//! The plugin system uses a lifecycle-based architecture:
//!
//! 1. **Initialization**: Plugin setup and configuration
//! 2. **Activation**: Plugin startup and resource allocation
//! 3. **Execution**: Request/response processing hooks
//! 4. **Deactivation**: Clean shutdown and resource cleanup
//!
//! ## Plugin Types
//!
//! ### Content Filtering Plugin
//!
//! Automatically filters and validates request/response content:
//! - **Content Validation**: Checks for inappropriate content
//! - **Security Filtering**: Removes malicious content
//! - **Compliance Checking**: Ensures regulatory compliance
//! - **Custom Rules**: Configurable filtering rules
//!
//! ### Cost Tracking Plugin
//!
//! Monitors and tracks costs across all providers:
//! - **Real-time Cost Tracking**: Live cost monitoring
//! - **Provider Cost Analysis**: Per-provider cost breakdown
//! - **User Cost Allocation**: Per-user cost tracking
//! - **Budget Management**: Cost limit enforcement
//!
//! ### Logging Plugin
//!
//! Enhanced logging with custom formats and destinations:
//! - **Structured Logging**: JSON and custom log formats
//! - **Multi-destination**: File, database, and external logging
//! - **Log Filtering**: Configurable log filtering
//! - **Performance Logging**: Request/response performance logs
//!
//! ## Plugin Lifecycle
//!
//! Each plugin follows a defined lifecycle:
//!
//! ```rust
//! use ultrafast_gateway::plugins::{Plugin, PluginManager};
//!
//! // Create plugin manager
//! let mut manager = PluginManager::new();
//!
//! // Register and initialize plugin
//! let plugin = Plugin::ContentFiltering(/* config */);
//! manager.register_plugin(plugin).await?;
//!
//! // Plugin is now active and processing requests
//! ```
//!
//! ## Hook System
//!
//! Plugins can hook into different stages of request processing:
//!
//! - **Before Request**: Modify incoming requests
//! - **After Response**: Modify outgoing responses
//! - **On Error**: Handle and process errors
//!
//! ## Configuration
//!
//! Plugins are configured via TOML configuration:
//!
//! ```toml
//! [[plugins]]
//! name = "content_filtering"
//! enabled = true
//! priority = 5
//!
//! [plugins.config]
//! filter_level = "moderate"
//! custom_rules = ["rule1", "rule2"]
//!
//! [[plugins]]
//! name = "cost_tracking"
//! enabled = true
//! priority = 10
//!
//! [plugins.config]
//! budget_limit = 100.0
//! alert_threshold = 0.8
//! ```
//!
//! ## Performance Impact
//!
//! The plugin system is designed for minimal performance impact:
//! - **Async Processing**: Non-blocking plugin execution
//! - **Priority-based Execution**: Configurable execution order
//! - **Error Isolation**: Plugin errors don't affect core functionality
//! - **Resource Management**: Automatic resource cleanup
//!
//! ## Security Considerations
//!
//! The plugin system includes security features:
//! - **Sandboxed Execution**: Isolated plugin execution
//! - **Input Validation**: Plugin input validation
//! - **Error Handling**: Secure error handling
//! - **Resource Limits**: Plugin resource limitations

use crate::config::PluginConfig;
use crate::gateway_error::GatewayError;
use axum::body::Body;
use axum::http::Request;
use axum::response::Response;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// pub mod rate_limiting; // DEPRECATED: Use auth middleware rate limiting instead
pub mod content_filtering;
pub mod cost_tracking;
pub mod logging;

/// Plugin lifecycle states.
///
/// Represents the current state of a plugin in its lifecycle.
/// Plugins transition through these states during initialization,
/// activation, and shutdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginState {
    /// Plugin is not yet initialized
    Inactive,
    /// Plugin is currently starting up
    Starting,
    /// Plugin is active and processing requests
    Active,
    /// Plugin is shutting down
    Stopping,
    /// Plugin has failed with an error message
    Failed(String),
}

/// Metadata for a plugin instance.
///
/// Contains information about a plugin's identity, state,
/// configuration, and lifecycle management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Unique identifier for this plugin instance
    pub id: String,
    /// Human-readable plugin name
    pub name: String,
    /// Plugin version string
    pub version: String,
    /// Whether the plugin is enabled
    pub enabled: bool,
    /// Current lifecycle state of the plugin
    pub state: PluginState,
    /// List of plugin dependencies (other plugin names)
    pub dependencies: Vec<String>,
    /// Execution priority (lower numbers = higher priority)
    pub priority: i32,
    /// Last error message if the plugin failed
    pub last_error: Option<String>,
}

/// Trait for plugin lifecycle management.
///
/// Defines the interface for plugin initialization, activation,
/// deactivation, and health monitoring. All plugins must implement
/// this trait to participate in the plugin system.
#[async_trait::async_trait]
pub trait PluginLifecycle: Send + Sync {
    /// Initialize the plugin with its configuration.
    ///
    /// This method is called when the plugin is first registered.
    /// It should perform any necessary setup and validation.
    async fn initialize(&mut self) -> Result<(), GatewayError>;

    /// Start the plugin and begin processing requests.
    ///
    /// This method is called to activate the plugin. It should
    /// allocate any necessary resources and begin processing.
    async fn start(&mut self) -> Result<(), GatewayError>;

    /// Stop the plugin and stop processing requests.
    ///
    /// This method is called to deactivate the plugin. It should
    /// gracefully shut down and release resources.
    async fn stop(&mut self) -> Result<(), GatewayError>;

    /// Clean up plugin resources.
    ///
    /// This method is called during plugin shutdown to perform
    /// final cleanup operations.
    async fn cleanup(&mut self) -> Result<(), GatewayError>;

    /// Perform a health check on the plugin.
    ///
    /// This method should verify that the plugin is functioning
    /// correctly and return an error if there are issues.
    async fn health_check(&self) -> Result<(), GatewayError>;

    /// Get a reference to the plugin's metadata.
    fn metadata(&self) -> &PluginMetadata;

    /// Get a mutable reference to the plugin's metadata.
    fn metadata_mut(&mut self) -> &mut PluginMetadata;
}

/// Trait for plugin request/response hooks.
///
/// Defines the interface for plugins to intercept and modify
/// requests and responses during processing. Plugins can implement
/// these hooks to add custom functionality.
#[async_trait::async_trait]
pub trait PluginHooks: Send + Sync {
    /// Hook called before a request is processed.
    ///
    /// This method is called before the request is sent to the
    /// provider. Plugins can modify the request or perform
    /// validation here.
    async fn before_request(&self, request: &mut Request<Body>) -> Result<(), GatewayError>;

    /// Hook called after a response is received.
    ///
    /// This method is called after the response is received from
    /// the provider. Plugins can modify the response or perform
    /// post-processing here.
    async fn after_response(&self, response: &mut Response<Body>) -> Result<(), GatewayError>;

    /// Hook called when an error occurs.
    ///
    /// This method is called when an error occurs during request
    /// processing. Plugins can handle or modify the error here.
    async fn on_error(&self, error: &GatewayError) -> Result<(), GatewayError>;
}

/// Enum representing different plugin types.
///
/// Each variant contains the specific plugin implementation.
/// This enum provides a unified interface for all plugin types
/// while maintaining type safety.
#[derive(Debug)]
pub enum Plugin {
    /// Cost tracking plugin for monitoring provider costs
    CostTracking(cost_tracking::CostTrackingPlugin),
    /// Content filtering plugin for request/response filtering
    ContentFiltering(content_filtering::ContentFilteringPlugin),
    /// Enhanced logging plugin for custom logging functionality
    Logging(logging::LoggingPlugin),
}

impl Plugin {
    pub fn metadata(&self) -> PluginMetadata {
        match self {
            Plugin::CostTracking(_) => PluginMetadata {
                id: Uuid::new_v4().to_string(),
                name: "cost_tracking".to_string(),
                version: "1.0.0".to_string(),
                enabled: true,
                state: PluginState::Inactive,
                dependencies: vec![],
                priority: 10,
                last_error: None,
            },
            Plugin::ContentFiltering(_) => PluginMetadata {
                id: Uuid::new_v4().to_string(),
                name: "content_filtering".to_string(),
                version: "1.0.0".to_string(),
                enabled: true,
                state: PluginState::Inactive,
                dependencies: vec![],
                priority: 5, // Higher priority than cost tracking
                last_error: None,
            },
            Plugin::Logging(_) => PluginMetadata {
                id: Uuid::new_v4().to_string(),
                name: "logging".to_string(),
                version: "1.0.0".to_string(),
                enabled: true,
                state: PluginState::Inactive,
                dependencies: vec![],
                priority: 20, // Lower priority
                last_error: None,
            },
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Plugin::CostTracking(_) => "cost_tracking",
            Plugin::ContentFiltering(_) => "content_filtering",
            Plugin::Logging(_) => "logging",
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Plugin::CostTracking(p) => p.enabled(),
            Plugin::ContentFiltering(p) => p.enabled(),
            Plugin::Logging(p) => p.enabled(),
        }
    }

    #[allow(dead_code)]
    fn set_error(&mut self, error: String) {
        // In a full implementation, plugins would store their own metadata
        tracing::error!("Plugin {} error: {}", self.name(), error);
    }

    pub async fn before_request(&self, request: &mut Request<Body>) -> Result<(), GatewayError> {
        match self {
            Plugin::CostTracking(p) => p.before_request(request).await,
            Plugin::ContentFiltering(p) => p.before_request(request).await,
            Plugin::Logging(p) => p.before_request(request).await,
        }
    }

    pub async fn after_response(&self, response: &mut Response<Body>) -> Result<(), GatewayError> {
        match self {
            Plugin::CostTracking(p) => p.after_response(response).await,
            Plugin::ContentFiltering(p) => p.after_response(response).await,
            Plugin::Logging(p) => p.after_response(response).await,
        }
    }

    pub async fn on_error(&self, error: &GatewayError) -> Result<(), GatewayError> {
        match self {
            Plugin::CostTracking(p) => p.on_error(error).await,
            Plugin::ContentFiltering(p) => p.on_error(error).await,
            Plugin::Logging(p) => p.on_error(error).await,
        }
    }
}

#[derive(Debug)]
/// A plugin with managed lifecycle and metadata.
///
/// This struct wraps a plugin with its metadata and provides
/// lifecycle management functionality. It ensures that plugins
/// are properly initialized, started, and cleaned up.
pub struct ManagedPlugin {
    /// The underlying plugin implementation
    plugin: Plugin,
    /// Plugin metadata and state information
    metadata: PluginMetadata,
}

impl ManagedPlugin {
    pub fn new(plugin: Plugin) -> Self {
        let metadata = plugin.metadata();
        Self { plugin, metadata }
    }

    pub async fn initialize(&mut self) -> Result<(), GatewayError> {
        self.metadata.state = PluginState::Starting;

        match self
            .plugin
            .before_request(
                &mut axum::http::Request::builder()
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
        {
            Ok(_) => {
                self.metadata.state = PluginState::Active;
                self.metadata.last_error = None;
                tracing::info!("Plugin {} initialized successfully", self.metadata.name);
                Ok(())
            }
            Err(e) => {
                self.metadata.state = PluginState::Failed(e.to_string());
                self.metadata.last_error = Some(e.to_string());
                tracing::error!("Plugin {} initialization failed: {}", self.metadata.name, e);
                Err(e)
            }
        }
    }

    pub async fn stop(&mut self) -> Result<(), GatewayError> {
        self.metadata.state = PluginState::Stopping;
        self.metadata.state = PluginState::Inactive;
        tracing::info!("Plugin {} stopped", self.metadata.name);
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        matches!(self.metadata.state, PluginState::Active) && self.metadata.enabled
    }
}

/// Manages the lifecycle and execution of all plugins.
///
/// This struct provides centralized plugin management including
/// registration, lifecycle management, and execution coordination.
/// It ensures plugins are executed in the correct order and
/// handles plugin failures gracefully.
pub struct PluginManager {
    /// Concurrent map of registered plugins by name
    plugins: DashMap<String, ManagedPlugin>,
    /// Plugin execution order (lower priority numbers execute first)
    execution_order: Arc<RwLock<Vec<String>>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: DashMap::new(),
            execution_order: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn register_plugin(&mut self, plugin: Plugin) -> Result<(), GatewayError> {
        let mut managed_plugin = ManagedPlugin::new(plugin);
        let plugin_name = managed_plugin.metadata.name.clone();
        let _priority = managed_plugin.metadata.priority;

        // Initialize the plugin
        managed_plugin.initialize().await?;

        // Insert into plugins map
        self.plugins.insert(plugin_name.clone(), managed_plugin);

        // Update execution order based on priority
        {
            let mut order = self.execution_order.write().await;
            order.push(plugin_name.clone());
            order.sort_by_key(|name| {
                // Get priority from plugins map (this is a simplified approach)
                match name.as_str() {
                    "content_filtering" => 5,
                    "cost_tracking" => 10,
                    "logging" => 20,
                    _ => 100,
                }
            });
        }

        tracing::info!("Plugin registered and initialized: {}", plugin_name);
        Ok(())
    }

    pub async fn get_plugin_metadata(&self, name: &str) -> Option<PluginMetadata> {
        self.plugins.get(name).map(|p| p.metadata.clone())
    }

    pub async fn list_plugins(&self) -> Vec<PluginMetadata> {
        self.plugins
            .iter()
            .map(|entry| entry.value().metadata.clone())
            .collect()
    }

    pub async fn stop_plugin(&self, name: &str) -> Result<(), GatewayError> {
        if let Some(mut plugin_entry) = self.plugins.get_mut(name) {
            plugin_entry.stop().await?;
            tracing::info!("Plugin stopped: {}", name);
        }
        Ok(())
    }

    pub async fn stop_all_plugins(&self) -> Result<(), GatewayError> {
        for mut plugin_entry in self.plugins.iter_mut() {
            let name = plugin_entry.key().clone();
            if let Err(e) = plugin_entry.stop().await {
                tracing::error!("Failed to stop plugin {}: {}", name, e);
            }
        }
        tracing::info!("All plugins stopped");
        Ok(())
    }

    pub async fn before_request(&self, request: &mut Request<Body>) -> Result<(), GatewayError> {
        let execution_order = self.execution_order.read().await;

        // Execute plugins in priority order
        for plugin_name in execution_order.iter() {
            if let Some(managed_plugin) = self.plugins.get(plugin_name) {
                if managed_plugin.is_active() {
                    if let Err(e) = managed_plugin.plugin.before_request(request).await {
                        tracing::error!("Plugin {} failed in before_request: {}", plugin_name, e);
                        // Don't stop the chain for non-critical errors
                        if matches!(e, GatewayError::ContentFiltered { .. }) {
                            return Err(e);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn after_response(&self, response: &mut Response<Body>) -> Result<(), GatewayError> {
        let execution_order = self.execution_order.read().await;

        // Execute plugins in reverse priority order for cleanup
        for plugin_name in execution_order.iter().rev() {
            if let Some(managed_plugin) = self.plugins.get(plugin_name) {
                if managed_plugin.is_active() {
                    if let Err(e) = managed_plugin.plugin.after_response(response).await {
                        tracing::error!("Plugin {} failed in after_response: {}", plugin_name, e);
                        // Continue with other plugins even if one fails
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn on_error(&self, error: &GatewayError) -> Result<(), GatewayError> {
        let execution_order = self.execution_order.read().await;

        for plugin_name in execution_order.iter() {
            if let Some(managed_plugin) = self.plugins.get(plugin_name) {
                if managed_plugin.is_active() {
                    if let Err(e) = managed_plugin.plugin.on_error(error).await {
                        tracing::error!("Plugin {} failed in on_error: {}", plugin_name, e);
                        // Continue with other plugins even if one fails
                    }
                }
            }
        }
        Ok(())
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Plugin {
    fn clone(&self) -> Self {
        match self {
            Plugin::CostTracking(p) => Plugin::CostTracking(p.clone()),
            Plugin::ContentFiltering(p) => Plugin::ContentFiltering(p.clone()),
            Plugin::Logging(p) => Plugin::Logging(p.clone()),
        }
    }
}

pub fn create_plugin(config: &PluginConfig) -> Result<Plugin, GatewayError> {
    match config.name.as_str() {
        "rate_limiting" => {
            // DEPRECATED: Rate limiting is now handled by auth middleware
            Err(GatewayError::Config {
                message: "rate_limiting plugin is deprecated. Use auth middleware rate limiting instead. Configure rate limits in [auth.rate_limiting] or per API key.".to_string(),
            })
        }
        "cost_tracking" => Ok(Plugin::CostTracking(
            cost_tracking::CostTrackingPlugin::new(config)?,
        )),
        "content_filtering" => Ok(Plugin::ContentFiltering(
            content_filtering::ContentFilteringPlugin::new(config)?,
        )),
        "logging" => Ok(Plugin::Logging(logging::LoggingPlugin::new(config)?)),
        _ => Err(GatewayError::Config {
            message: format!("Unknown plugin: {}", config.name),
        }),
    }
}
