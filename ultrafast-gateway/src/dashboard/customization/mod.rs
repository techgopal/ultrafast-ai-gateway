// User Customization System
// Advanced user customization features including custom dashboards, themes, layouts, and widgets

use crate::dashboard::architecture::{DashboardContext, WidgetType, Position, Size};
use crate::gateway_error::GatewayError;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use std::time::{Duration, Instant};

pub mod layout_engine;
pub mod widget_builder;
pub mod theme_manager;
pub mod preferences;
pub mod permissions;
pub mod storage;

/// User customization manager
pub struct CustomizationManager {
    storage: Arc<dyn storage::CustomizationStorage>,
    layout_engine: Arc<layout_engine::LayoutEngine>,
    widget_builder: Arc<widget_builder::WidgetBuilder>,
    theme_manager: Arc<theme_manager::ThemeManager>,
    permissions: Arc<permissions::PermissionManager>,
    cache: Arc<RwLock<HashMap<String, CachedCustomization>>>,
    config: CustomizationConfig,
}

impl CustomizationManager {
    pub fn new(
        storage: Arc<dyn storage::CustomizationStorage>,
        config: CustomizationConfig,
    ) -> Self {
        Self {
            storage,
            layout_engine: Arc::new(layout_engine::LayoutEngine::new()),
            widget_builder: Arc::new(widget_builder::WidgetBuilder::new()),
            theme_manager: Arc::new(theme_manager::ThemeManager::new()),
            permissions: Arc::new(permissions::PermissionManager::new()),
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }
    
    /// Get user's complete customization settings
    pub async fn get_user_customization(&self, user_id: &str) -> Result<UserCustomization, GatewayError> {
        // Check cache first
        let cache_key = format!("user:{}", user_id);
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&cache_key) {
                if !cached.is_expired() {
                    return Ok(cached.customization.clone());
                }
            }
        }
        
        // Load from storage
        let customization = self.storage.load_user_customization(user_id).await
            .unwrap_or_else(|_| self.create_default_customization(user_id));
        
        // Cache the result
        {
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, CachedCustomization {
                customization: customization.clone(),
                cached_at: Instant::now(),
                ttl: Duration::from_secs(300), // 5 minutes
            });
        }
        
        Ok(customization)
    }
    
    /// Save user customization settings
    pub async fn save_user_customization(&self, user_id: &str, customization: UserCustomization) -> Result<(), GatewayError> {
        // Validate customization
        self.validate_customization(&customization).await?;
        
        // Check permissions
        self.permissions.check_customization_permissions(user_id, &customization).await?;
        
        // Save to storage
        self.storage.save_user_customization(user_id, &customization).await?;
        
        // Update cache
        let cache_key = format!("user:{}", user_id);
        {
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, CachedCustomization {
                customization: customization.clone(),
                cached_at: Instant::now(),
                ttl: Duration::from_secs(300),
            });
        }
        
        tracing::info!("Saved customization for user: {}", user_id);
        Ok(())
    }
    
    /// Create a custom dashboard
    pub async fn create_custom_dashboard(&self, user_id: &str, dashboard: CustomDashboard) -> Result<String, GatewayError> {
        // Check if user can create more dashboards
        let user_customization = self.get_user_customization(user_id).await?;
        if user_customization.custom_dashboards.len() >= self.config.max_custom_dashboards {
            return Err(GatewayError::InvalidRequest {
                message: format!("Maximum number of custom dashboards reached ({})", self.config.max_custom_dashboards)
            });
        }
        
        // Validate dashboard
        self.validate_dashboard(&dashboard).await?;
        
        // Generate dashboard ID
        let dashboard_id = Uuid::new_v4().to_string();
        
        // Save dashboard
        self.storage.save_custom_dashboard(user_id, &dashboard_id, &dashboard).await?;
        
        // Update user customization
        let mut updated_customization = user_customization;
        updated_customization.custom_dashboards.insert(dashboard_id.clone(), dashboard);
        self.save_user_customization(user_id, updated_customization).await?;
        
        Ok(dashboard_id)
    }
    
    /// Update custom dashboard
    pub async fn update_custom_dashboard(&self, user_id: &str, dashboard_id: &str, dashboard: CustomDashboard) -> Result<(), GatewayError> {
        // Validate dashboard
        self.validate_dashboard(&dashboard).await?;
        
        // Check ownership
        let user_customization = self.get_user_customization(user_id).await?;
        if !user_customization.custom_dashboards.contains_key(dashboard_id) {
            return Err(GatewayError::Authentication {
                message: "Dashboard not found or access denied".to_string()
            });
        }
        
        // Save dashboard
        self.storage.save_custom_dashboard(user_id, dashboard_id, &dashboard).await?;
        
        // Update user customization
        let mut updated_customization = user_customization;
        updated_customization.custom_dashboards.insert(dashboard_id.to_string(), dashboard);
        self.save_user_customization(user_id, updated_customization).await?;
        
        Ok(())
    }
    
    /// Delete custom dashboard
    pub async fn delete_custom_dashboard(&self, user_id: &str, dashboard_id: &str) -> Result<(), GatewayError> {
        // Check ownership
        let user_customization = self.get_user_customization(user_id).await?;
        if !user_customization.custom_dashboards.contains_key(dashboard_id) {
            return Err(GatewayError::Authentication {
                message: "Dashboard not found or access denied".to_string()
            });
        }
        
        // Delete from storage
        self.storage.delete_custom_dashboard(user_id, dashboard_id).await?;
        
        // Update user customization
        let mut updated_customization = user_customization;
        updated_customization.custom_dashboards.remove(dashboard_id);
        self.save_user_customization(user_id, updated_customization).await?;
        
        Ok(())
    }
    
    /// Create custom widget
    pub async fn create_custom_widget(&self, user_id: &str, widget: CustomWidget) -> Result<String, GatewayError> {
        // Validate widget
        self.widget_builder.validate(&widget).await?;
        
        // Check limits
        let user_customization = self.get_user_customization(user_id).await?;
        if user_customization.custom_widgets.len() >= self.config.max_custom_widgets {
            return Err(GatewayError::InvalidRequest {
                message: format!("Maximum number of custom widgets reached ({})", self.config.max_custom_widgets)
            });
        }
        
        // Generate widget ID
        let widget_id = Uuid::new_v4().to_string();
        
        // Save widget
        self.storage.save_custom_widget(user_id, &widget_id, &widget).await?;
        
        // Update user customization
        let mut updated_customization = user_customization;
        updated_customization.custom_widgets.insert(widget_id.clone(), widget);
        self.save_user_customization(user_id, updated_customization).await?;
        
        Ok(widget_id)
    }
    
    /// Apply custom theme
    pub async fn apply_custom_theme(&self, user_id: &str, theme: CustomTheme) -> Result<(), GatewayError> {
        // Validate theme
        self.theme_manager.validate(&theme).await?;
        
        // Get user customization
        let mut user_customization = self.get_user_customization(user_id).await?;
        
        // Update theme
        user_customization.theme = Some(theme);
        
        // Save
        self.save_user_customization(user_id, user_customization).await?;
        
        Ok(())
    }
    
    /// Get available layout templates
    pub async fn get_layout_templates(&self) -> Vec<LayoutTemplate> {
        self.layout_engine.get_templates().await
    }
    
    /// Apply layout template
    pub async fn apply_layout_template(&self, user_id: &str, template_id: &str) -> Result<(), GatewayError> {
        let template = self.layout_engine.get_template(template_id).await
            .ok_or_else(|| GatewayError::InvalidRequest {
                message: format!("Layout template not found: {}", template_id)
            })?;
        
        let mut user_customization = self.get_user_customization(user_id).await?;
        user_customization.layout = template.layout;
        
        self.save_user_customization(user_id, user_customization).await?;
        
        Ok(())
    }
    
    /// Export user customization
    pub async fn export_customization(&self, user_id: &str) -> Result<CustomizationExport, GatewayError> {
        let customization = self.get_user_customization(user_id).await?;
        
        Ok(CustomizationExport {
            version: "1.0".to_string(),
            exported_at: chrono::Utc::now().timestamp(),
            user_id: user_id.to_string(),
            customization,
        })
    }
    
    /// Import user customization
    pub async fn import_customization(&self, user_id: &str, import: CustomizationExport) -> Result<(), GatewayError> {
        // Validate import
        if import.version != "1.0" {
            return Err(GatewayError::InvalidRequest {
                message: format!("Unsupported import version: {}", import.version)
            });
        }
        
        // Validate customization
        self.validate_customization(&import.customization).await?;
        
        // Check permissions
        self.permissions.check_import_permissions(user_id, &import).await?;
        
        // Save customization
        self.save_user_customization(user_id, import.customization).await?;
        
        Ok(())
    }
    
    /// Get customization analytics for user
    pub async fn get_user_analytics(&self, user_id: &str) -> Result<CustomizationAnalytics, GatewayError> {
        let customization = self.get_user_customization(user_id).await?;
        
        Ok(CustomizationAnalytics {
            dashboard_count: customization.custom_dashboards.len(),
            widget_count: customization.custom_widgets.len(),
            theme_customized: customization.theme.is_some(),
            last_modified: customization.last_modified,
            most_used_widgets: self.calculate_most_used_widgets(&customization).await,
            customization_score: self.calculate_customization_score(&customization).await,
        })
    }
    
    fn create_default_customization(&self, user_id: &str) -> UserCustomization {
        UserCustomization {
            user_id: user_id.to_string(),
            preferences: UserPreferences::default(),
            layout: DashboardLayout::default(),
            theme: None,
            custom_dashboards: HashMap::new(),
            custom_widgets: HashMap::new(),
            widget_preferences: HashMap::new(),
            shortcuts: HashMap::new(),
            favorites: Vec::new(),
            recent_views: Vec::new(),
            created_at: chrono::Utc::now().timestamp(),
            last_modified: chrono::Utc::now().timestamp(),
        }
    }
    
    async fn validate_customization(&self, customization: &UserCustomization) -> Result<(), GatewayError> {
        // Validate layout
        self.layout_engine.validate(&customization.layout).await?;
        
        // Validate custom widgets
        for (widget_id, widget) in &customization.custom_widgets {
            self.widget_builder.validate(widget).await
                .map_err(|e| GatewayError::InvalidRequest {
                    message: format!("Invalid widget {}: {}", widget_id, e)
                })?;
        }
        
        // Validate custom dashboards
        for (dashboard_id, dashboard) in &customization.custom_dashboards {
            self.validate_dashboard(dashboard).await
                .map_err(|e| GatewayError::InvalidRequest {
                    message: format!("Invalid dashboard {}: {}", dashboard_id, e)
                })?;
        }
        
        // Validate theme
        if let Some(theme) = &customization.theme {
            self.theme_manager.validate(theme).await?;
        }
        
        Ok(())
    }
    
    async fn validate_dashboard(&self, dashboard: &CustomDashboard) -> Result<(), GatewayError> {
        // Check widget limits
        if dashboard.widgets.len() > self.config.max_widgets_per_dashboard {
            return Err(GatewayError::InvalidRequest {
                message: format!("Too many widgets in dashboard (max: {})", self.config.max_widgets_per_dashboard)
            });
        }
        
        // Validate layout
        self.layout_engine.validate(&dashboard.layout).await?;
        
        // Validate widgets
        for widget in &dashboard.widgets {
            self.widget_builder.validate_widget_config(widget).await?;
        }
        
        Ok(())
    }
    
    async fn calculate_most_used_widgets(&self, customization: &UserCustomization) -> Vec<String> {
        // In a real implementation, this would analyze usage patterns
        customization.custom_widgets.keys().take(5).cloned().collect()
    }
    
    async fn calculate_customization_score(&self, customization: &UserCustomization) -> f64 {
        let mut score = 0.0;
        
        // Points for custom dashboards
        score += customization.custom_dashboards.len() as f64 * 10.0;
        
        // Points for custom widgets
        score += customization.custom_widgets.len() as f64 * 5.0;
        
        // Points for theme customization
        if customization.theme.is_some() {
            score += 20.0;
        }
        
        // Points for preferences
        if customization.preferences.notifications_enabled {
            score += 5.0;
        }
        
        // Cap at 100
        score.min(100.0)
    }
}

/// User customization data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCustomization {
    pub user_id: String,
    pub preferences: UserPreferences,
    pub layout: DashboardLayout,
    pub theme: Option<CustomTheme>,
    pub custom_dashboards: HashMap<String, CustomDashboard>,
    pub custom_widgets: HashMap<String, CustomWidget>,
    pub widget_preferences: HashMap<String, WidgetPreferences>,
    pub shortcuts: HashMap<String, KeyboardShortcut>,
    pub favorites: Vec<String>,
    pub recent_views: Vec<RecentView>,
    pub created_at: i64,
    pub last_modified: i64,
}

/// User preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub default_refresh_interval: Duration,
    pub notifications_enabled: bool,
    pub sound_enabled: bool,
    pub dark_mode_preference: ThemePreference,
    pub timezone: String,
    pub date_format: String,
    pub number_format: String,
    pub language: String,
    pub accessibility: AccessibilityPreferences,
    pub privacy: PrivacyPreferences,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            default_refresh_interval: Duration::from_secs(30),
            notifications_enabled: true,
            sound_enabled: false,
            dark_mode_preference: ThemePreference::Auto,
            timezone: "UTC".to_string(),
            date_format: "YYYY-MM-DD HH:mm:ss".to_string(),
            number_format: "en-US".to_string(),
            language: "en".to_string(),
            accessibility: AccessibilityPreferences::default(),
            privacy: PrivacyPreferences::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThemePreference {
    Light,
    Dark,
    Auto,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityPreferences {
    pub high_contrast: bool,
    pub reduced_motion: bool,
    pub large_text: bool,
    pub screen_reader: bool,
    pub keyboard_navigation: bool,
}

impl Default for AccessibilityPreferences {
    fn default() -> Self {
        Self {
            high_contrast: false,
            reduced_motion: false,
            large_text: false,
            screen_reader: false,
            keyboard_navigation: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyPreferences {
    pub analytics_enabled: bool,
    pub error_reporting: bool,
    pub usage_tracking: bool,
    pub data_sharing: bool,
}

impl Default for PrivacyPreferences {
    fn default() -> Self {
        Self {
            analytics_enabled: true,
            error_reporting: true,
            usage_tracking: true,
            data_sharing: false,
        }
    }
}

/// Dashboard layout configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardLayout {
    pub layout_type: LayoutType,
    pub grid_columns: u8,
    pub grid_rows: u8,
    pub gap_size: String,
    pub padding: String,
    pub responsive: bool,
    pub breakpoints: HashMap<String, GridConfig>,
}

impl Default for DashboardLayout {
    fn default() -> Self {
        let mut breakpoints = HashMap::new();
        breakpoints.insert("mobile".to_string(), GridConfig { columns: 1, rows: 0 });
        breakpoints.insert("tablet".to_string(), GridConfig { columns: 2, rows: 0 });
        breakpoints.insert("desktop".to_string(), GridConfig { columns: 4, rows: 0 });
        
        Self {
            layout_type: LayoutType::Grid,
            grid_columns: 4,
            grid_rows: 0, // Auto rows
            gap_size: "24px".to_string(),
            padding: "24px".to_string(),
            responsive: true,
            breakpoints,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayoutType {
    Grid,
    Masonry,
    Flexbox,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridConfig {
    pub columns: u8,
    pub rows: u8,
}

/// Custom dashboard definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomDashboard {
    pub id: String,
    pub name: String,
    pub description: String,
    pub layout: DashboardLayout,
    pub widgets: Vec<DashboardWidgetConfig>,
    pub filters: Vec<DashboardFilter>,
    pub permissions: DashboardPermissions,
    pub settings: DashboardSettings,
    pub created_at: i64,
    pub last_modified: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardWidgetConfig {
    pub widget_id: String,
    pub widget_type: WidgetType,
    pub position: Position,
    pub size: Size,
    pub config: serde_json::Value,
    pub title: Option<String>,
    pub refresh_interval: Option<Duration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardFilter {
    pub name: String,
    pub filter_type: FilterType,
    pub options: Vec<String>,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterType {
    Select,
    MultiSelect,
    DateRange,
    TextInput,
    NumberRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardPermissions {
    pub public: bool,
    pub shared_with: Vec<String>,
    pub allow_edit: bool,
    pub allow_copy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSettings {
    pub auto_refresh: bool,
    pub refresh_interval: Duration,
    pub full_screen_mode: bool,
    pub export_enabled: bool,
}

/// Custom widget definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomWidget {
    pub id: String,
    pub name: String,
    pub description: String,
    pub widget_type: WidgetType,
    pub data_source: DataSource,
    pub visualization: VisualizationConfig,
    pub interactions: InteractionConfig,
    pub styling: WidgetStyling,
    pub created_at: i64,
    pub last_modified: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSource {
    pub source_type: DataSourceType,
    pub query: String,
    pub refresh_interval: Duration,
    pub cache_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataSourceType {
    Api,
    Database,
    Static,
    Computed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizationConfig {
    pub chart_type: String,
    pub color_scheme: String,
    pub show_legend: bool,
    pub show_axes: bool,
    pub animation_enabled: bool,
    pub options: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionConfig {
    pub clickable: bool,
    pub hoverable: bool,
    pub drilldown_enabled: bool,
    pub export_enabled: bool,
    pub actions: Vec<WidgetAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetAction {
    pub name: String,
    pub action_type: ActionType,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionType {
    Navigate,
    Filter,
    Export,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetStyling {
    pub background_color: Option<String>,
    pub text_color: Option<String>,
    pub border_color: Option<String>,
    pub border_width: Option<String>,
    pub border_radius: Option<String>,
    pub padding: Option<String>,
    pub margin: Option<String>,
    pub custom_css: Option<String>,
}

/// Custom theme definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomTheme {
    pub id: String,
    pub name: String,
    pub description: String,
    pub colors: ThemeColors,
    pub fonts: ThemeFonts,
    pub spacing: ThemeSpacing,
    pub shadows: ThemeShadows,
    pub borders: ThemeBorders,
    pub animations: ThemeAnimations,
    pub custom_css: Option<String>,
    pub created_at: i64,
    pub last_modified: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeColors {
    pub primary: String,
    pub secondary: String,
    pub accent: String,
    pub background: String,
    pub surface: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub success: String,
    pub warning: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeFonts {
    pub primary: String,
    pub secondary: String,
    pub monospace: String,
    pub sizes: HashMap<String, String>,
    pub weights: HashMap<String, u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeSpacing {
    pub xs: String,
    pub sm: String,
    pub md: String,
    pub lg: String,
    pub xl: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeShadows {
    pub small: String,
    pub medium: String,
    pub large: String,
    pub extra_large: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeBorders {
    pub width: String,
    pub radius: String,
    pub style: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeAnimations {
    pub duration: String,
    pub easing: String,
    pub enabled: bool,
}

/// Widget preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetPreferences {
    pub auto_refresh: bool,
    pub refresh_interval: Duration,
    pub show_title: bool,
    pub show_border: bool,
    pub collapse_on_error: bool,
    pub notifications_enabled: bool,
}

/// Keyboard shortcut definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardShortcut {
    pub key_combination: String,
    pub action: String,
    pub description: String,
    pub enabled: bool,
}

/// Recent view tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentView {
    pub item_type: String,
    pub item_id: String,
    pub title: String,
    pub viewed_at: i64,
    pub view_count: u32,
}

/// Layout template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub preview_image: Option<String>,
    pub layout: DashboardLayout,
    pub default_widgets: Vec<DashboardWidgetConfig>,
    pub tags: Vec<String>,
    pub category: String,
}

/// Customization export format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomizationExport {
    pub version: String,
    pub exported_at: i64,
    pub user_id: String,
    pub customization: UserCustomization,
}

/// Customization analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomizationAnalytics {
    pub dashboard_count: usize,
    pub widget_count: usize,
    pub theme_customized: bool,
    pub last_modified: i64,
    pub most_used_widgets: Vec<String>,
    pub customization_score: f64,
}

/// Customization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomizationConfig {
    pub enabled: bool,
    pub max_custom_dashboards: usize,
    pub max_custom_widgets: usize,
    pub max_widgets_per_dashboard: usize,
    pub allow_custom_themes: bool,
    pub allow_layout_changes: bool,
    pub allow_widget_creation: bool,
    pub allow_sharing: bool,
}

impl Default for CustomizationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_custom_dashboards: 10,
            max_custom_widgets: 50,
            max_widgets_per_dashboard: 20,
            allow_custom_themes: true,
            allow_layout_changes: true,
            allow_widget_creation: true,
            allow_sharing: true,
        }
    }
}

/// Cached customization for performance
#[derive(Debug, Clone)]
struct CachedCustomization {
    customization: UserCustomization,
    cached_at: Instant,
    ttl: Duration,
}

impl CachedCustomization {
    fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }
}