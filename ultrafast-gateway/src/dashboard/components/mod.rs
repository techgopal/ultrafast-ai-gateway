// Dashboard Component System
// Modular, reusable components for the dashboard

use crate::dashboard::architecture::{DashboardContext, ComponentType};
use crate::gateway_error::GatewayError;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod metrics_card;
pub mod performance_chart;
pub mod provider_health;
pub mod cost_analysis;
pub mod error_analytics;
pub mod user_activity;
pub mod system_health;
pub mod alerts_panel;

/// Trait that all dashboard components must implement
#[async_trait]
pub trait DashboardComponent: Send + Sync {
    /// Unique identifier for this component type
    fn component_id(&self) -> &'static str;
    
    /// Human-readable name for this component
    fn display_name(&self) -> &'static str;
    
    /// Load data required for this component
    async fn load_data(&self, context: &DashboardContext) -> Result<ComponentData, GatewayError>;
    
    /// Render the component with the provided data
    async fn render(&self, data: ComponentData) -> Result<String, GatewayError>;
    
    /// Get component configuration schema
    fn config_schema(&self) -> ComponentConfigSchema;
    
    /// Validate component configuration
    fn validate_config(&self, config: &serde_json::Value) -> Result<(), GatewayError>;
    
    /// Check if user has permission to view this component
    fn check_permissions(&self, context: &DashboardContext) -> bool {
        // Default implementation - can be overridden
        context.permissions.contains(&format!("dashboard:view:{}", self.component_id()))
            || context.permissions.contains(&"dashboard:admin".to_string())
    }
    
    /// Get component dependencies (other components this one needs)
    fn dependencies(&self) -> Vec<ComponentType> {
        vec![]
    }
    
    /// Whether this component supports real-time updates
    fn supports_realtime(&self) -> bool {
        false
    }
    
    /// Handle real-time data updates
    async fn handle_realtime_update(&self, _update: ComponentUpdate) -> Result<Option<String>, GatewayError> {
        Ok(None)
    }
}

/// Component registry that manages all available dashboard components
pub struct ComponentRegistry {
    components: RwLock<HashMap<ComponentType, Arc<dyn DashboardComponent>>>,
    templates: RwLock<HashMap<String, String>>,
}

impl ComponentRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            components: RwLock::new(HashMap::new()),
            templates: RwLock::new(HashMap::new()),
        };
        
        // Register built-in components
        tokio::spawn(async move {
            registry.register_builtin_components().await;
        });
        
        registry
    }
    
    async fn register_builtin_components(&self) {
        self.register_component(ComponentType::MetricsOverview, Arc::new(metrics_card::MetricsCardComponent::new())).await;
        self.register_component(ComponentType::PerformanceChart, Arc::new(performance_chart::PerformanceChartComponent::new())).await;
        self.register_component(ComponentType::ProviderHealth, Arc::new(provider_health::ProviderHealthComponent::new())).await;
        self.register_component(ComponentType::CostAnalysis, Arc::new(cost_analysis::CostAnalysisComponent::new())).await;
        self.register_component(ComponentType::ErrorAnalytics, Arc::new(error_analytics::ErrorAnalyticsComponent::new())).await;
        self.register_component(ComponentType::UserActivity, Arc::new(user_activity::UserActivityComponent::new())).await;
        self.register_component(ComponentType::SystemHealth, Arc::new(system_health::SystemHealthComponent::new())).await;
        self.register_component(ComponentType::AlertsPanel, Arc::new(alerts_panel::AlertsPanelComponent::new())).await;
    }
    
    pub async fn register_component(&self, component_type: ComponentType, component: Arc<dyn DashboardComponent>) {
        let mut components = self.components.write().await;
        components.insert(component_type, component);
    }
    
    pub async fn get_component(&self, component_type: ComponentType) -> Result<Arc<dyn DashboardComponent>, GatewayError> {
        let components = self.components.read().await;
        components.get(&component_type)
            .cloned()
            .ok_or_else(|| GatewayError::InvalidRequest {
                message: format!("Component type {:?} not found", component_type)
            })
    }
    
    pub async fn list_components(&self) -> Vec<ComponentInfo> {
        let components = self.components.read().await;
        let mut info = Vec::new();
        
        for (component_type, component) in components.iter() {
            info.push(ComponentInfo {
                component_type: component_type.clone(),
                id: component.component_id().to_string(),
                display_name: component.display_name().to_string(),
                config_schema: component.config_schema(),
                supports_realtime: component.supports_realtime(),
                dependencies: component.dependencies(),
            });
        }
        
        info
    }
    
    pub async fn render_component(&self, component_type: ComponentType, context: &DashboardContext, config: Option<serde_json::Value>) -> Result<String, GatewayError> {
        let component = self.get_component(component_type).await?;
        
        // Check permissions
        if !component.check_permissions(context) {
            return Err(GatewayError::Authentication {
                message: "Insufficient permissions to view this component".to_string()
            });
        }
        
        // Validate configuration if provided
        if let Some(config) = &config {
            component.validate_config(config)?;
        }
        
        // Load component data
        let data = component.load_data(context).await?;
        
        // Render component
        component.render(data).await
    }
    
    pub async fn load_template(&self, template_name: &str) -> Option<String> {
        let templates = self.templates.read().await;
        templates.get(template_name).cloned()
    }
    
    pub async fn register_template(&self, name: String, template: String) {
        let mut templates = self.templates.write().await;
        templates.insert(name, template);
    }
}

/// Data structure that components use to pass data to templates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentData {
    pub component_id: String,
    pub title: String,
    pub data: serde_json::Value,
    pub metadata: ComponentMetadata,
    pub error: Option<String>,
}

impl ComponentData {
    pub fn new(component_id: String, title: String) -> Self {
        Self {
            component_id,
            title,
            data: serde_json::Value::Null,
            metadata: ComponentMetadata::default(),
            error: None,
        }
    }
    
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = data;
        self
    }
    
    pub fn with_error(mut self, error: String) -> Self {
        self.error = Some(error);
        self
    }
    
    pub fn with_metadata(mut self, metadata: ComponentMetadata) -> Self {
        self.metadata = metadata;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentMetadata {
    pub last_updated: Option<i64>,
    pub refresh_interval: Option<u64>,
    pub cache_key: Option<String>,
    pub loading_state: bool,
    pub error_count: u32,
    pub performance_metrics: PerformanceMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PerformanceMetrics {
    pub load_time_ms: u64,
    pub render_time_ms: u64,
    pub data_size_bytes: u64,
}

/// Configuration schema for components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfigSchema {
    pub schema_version: String,
    pub properties: HashMap<String, PropertySchema>,
    pub required: Vec<String>,
}

impl Default for ComponentConfigSchema {
    fn default() -> Self {
        Self {
            schema_version: "1.0".to_string(),
            properties: HashMap::new(),
            required: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertySchema {
    pub property_type: PropertyType,
    pub description: String,
    pub default_value: Option<serde_json::Value>,
    pub validation: PropertyValidation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyType {
    String,
    Number,
    Boolean,
    Array,
    Object,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PropertyValidation {
    pub min_length: Option<u32>,
    pub max_length: Option<u32>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub pattern: Option<String>,
    pub allowed_values: Option<Vec<serde_json::Value>>,
}

/// Information about a registered component
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentInfo {
    pub component_type: ComponentType,
    pub id: String,
    pub display_name: String,
    pub config_schema: ComponentConfigSchema,
    pub supports_realtime: bool,
    pub dependencies: Vec<ComponentType>,
}

/// Real-time component updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentUpdate {
    pub component_id: String,
    pub update_type: UpdateType,
    pub data: serde_json::Value,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpdateType {
    DataRefresh,
    ConfigChange,
    StatusChange,
    Error,
}

/// Base component implementation helper
pub struct BaseComponent {
    pub id: &'static str,
    pub display_name: &'static str,
    pub template_name: String,
    pub required_permissions: Vec<String>,
}

impl BaseComponent {
    pub fn new(id: &'static str, display_name: &'static str) -> Self {
        Self {
            id,
            display_name,
            template_name: format!("components/{}", id),
            required_permissions: vec![format!("dashboard:view:{}", id)],
        }
    }
    
    pub fn with_template(mut self, template_name: String) -> Self {
        self.template_name = template_name;
        self
    }
    
    pub fn with_permissions(mut self, permissions: Vec<String>) -> Self {
        self.required_permissions = permissions;
        self
    }
}

/// Component factory for creating components dynamically
pub struct ComponentFactory;

impl ComponentFactory {
    pub fn create_component(component_type: ComponentType) -> Result<Arc<dyn DashboardComponent>, GatewayError> {
        match component_type {
            ComponentType::MetricsOverview => Ok(Arc::new(metrics_card::MetricsCardComponent::new())),
            ComponentType::PerformanceChart => Ok(Arc::new(performance_chart::PerformanceChartComponent::new())),
            ComponentType::ProviderHealth => Ok(Arc::new(provider_health::ProviderHealthComponent::new())),
            ComponentType::CostAnalysis => Ok(Arc::new(cost_analysis::CostAnalysisComponent::new())),
            ComponentType::ErrorAnalytics => Ok(Arc::new(error_analytics::ErrorAnalyticsComponent::new())),
            ComponentType::UserActivity => Ok(Arc::new(user_activity::UserActivityComponent::new())),
            ComponentType::SystemHealth => Ok(Arc::new(system_health::SystemHealthComponent::new())),
            ComponentType::AlertsPanel => Ok(Arc::new(alerts_panel::AlertsPanelComponent::new())),
            _ => Err(GatewayError::InvalidRequest {
                message: format!("Unknown component type: {:?}", component_type)
            })
        }
    }
}

/// Helper macros for component development
#[macro_export]
macro_rules! impl_basic_component {
    ($struct_name:ident, $id:expr, $display_name:expr) => {
        impl $struct_name {
            pub fn new() -> Self {
                Self {
                    base: BaseComponent::new($id, $display_name),
                }
            }
        }
        
        #[async_trait]
        impl DashboardComponent for $struct_name {
            fn component_id(&self) -> &'static str {
                self.base.id
            }
            
            fn display_name(&self) -> &'static str {
                self.base.display_name
            }
            
            fn config_schema(&self) -> ComponentConfigSchema {
                ComponentConfigSchema::default()
            }
            
            fn validate_config(&self, _config: &serde_json::Value) -> Result<(), GatewayError> {
                Ok(())
            }
        }
    };
}

pub use impl_basic_component;