// Advanced Dashboard Configuration System
// Comprehensive configuration management with validation, hot-reload, and multi-environment support

use crate::gateway_error::GatewayError;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};

pub mod loader;
pub mod validator;
pub mod hot_reload;
pub mod environment;
pub mod schema;

/// Advanced dashboard configuration with full feature support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfiguration {
    /// Basic configuration
    pub basic: BasicConfig,
    
    /// Layout and theming
    pub appearance: AppearanceConfig,
    
    /// Feature toggles
    pub features: FeatureConfig,
    
    /// Security settings
    pub security: SecurityConfig,
    
    /// Performance optimization
    pub performance: PerformanceConfig,
    
    /// Integration settings
    pub integrations: IntegrationConfig,
    
    /// User customization options
    pub customization: CustomizationConfig,
    
    /// Advanced analytics
    pub analytics: AnalyticsConfig,
    
    /// Notification system
    pub notifications: NotificationConfig,
    
    /// Environment-specific overrides
    pub environment: EnvironmentConfig,
}

impl Default for DashboardConfiguration {
    fn default() -> Self {
        Self {
            basic: BasicConfig::default(),
            appearance: AppearanceConfig::default(),
            features: FeatureConfig::all_enabled(),
            security: SecurityConfig::secure_defaults(),
            performance: PerformanceConfig::optimized(),
            integrations: IntegrationConfig::default(),
            customization: CustomizationConfig::default(),
            analytics: AnalyticsConfig::default(),
            notifications: NotificationConfig::default(),
            environment: EnvironmentConfig::default(),
        }
    }
}

/// Basic dashboard configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicConfig {
    pub title: String,
    pub description: String,
    pub version: String,
    pub contact_info: ContactInfo,
    pub timezone: String,
    pub locale: String,
    pub default_refresh_interval: Duration,
}

impl Default for BasicConfig {
    fn default() -> Self {
        Self {
            title: "Ultrafast Gateway Dashboard".to_string(),
            description: "High-performance AI gateway monitoring and management".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            contact_info: ContactInfo::default(),
            timezone: "UTC".to_string(),
            locale: "en-US".to_string(),
            default_refresh_interval: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactInfo {
    pub support_email: Option<String>,
    pub admin_email: Option<String>,
    pub documentation_url: Option<String>,
    pub support_url: Option<String>,
}

impl Default for ContactInfo {
    fn default() -> Self {
        Self {
            support_email: None,
            admin_email: None,
            documentation_url: Some("https://docs.ultrafast.ai".to_string()),
            support_url: None,
        }
    }
}

/// Appearance and theming configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    pub theme: ThemeConfig,
    pub layout: LayoutConfig,
    pub branding: BrandingConfig,
    pub fonts: FontConfig,
    pub colors: ColorPalette,
    pub animations: AnimationConfig,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: ThemeConfig::default(),
            layout: LayoutConfig::default(),
            branding: BrandingConfig::default(),
            fonts: FontConfig::default(),
            colors: ColorPalette::default(),
            animations: AnimationConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub default_mode: ThemeMode,
    pub allow_user_override: bool,
    pub custom_themes: HashMap<String, CustomTheme>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            default_mode: ThemeMode::Auto,
            allow_user_override: true,
            custom_themes: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThemeMode {
    Light,
    Dark,
    Auto,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomTheme {
    pub name: String,
    pub description: String,
    pub colors: ColorPalette,
    pub css_overrides: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    pub sidebar: SidebarConfig,
    pub header: HeaderConfig,
    pub footer: FooterConfig,
    pub content: ContentConfig,
    pub responsive_breakpoints: ResponsiveBreakpoints,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            sidebar: SidebarConfig::default(),
            header: HeaderConfig::default(),
            footer: FooterConfig::default(),
            content: ContentConfig::default(),
            responsive_breakpoints: ResponsiveBreakpoints::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarConfig {
    pub enabled: bool,
    pub collapsible: bool,
    pub default_collapsed: bool,
    pub width: String,
    pub position: SidebarPosition,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            collapsible: true,
            default_collapsed: false,
            width: "280px".to_string(),
            position: SidebarPosition::Left,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SidebarPosition {
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderConfig {
    pub enabled: bool,
    pub height: String,
    pub show_logo: bool,
    pub show_title: bool,
    pub show_user_menu: bool,
    pub show_notifications: bool,
    pub custom_content: Option<String>,
}

impl Default for HeaderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            height: "64px".to_string(),
            show_logo: true,
            show_title: true,
            show_user_menu: true,
            show_notifications: true,
            custom_content: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FooterConfig {
    pub enabled: bool,
    pub height: String,
    pub content: String,
    pub show_version: bool,
    pub show_links: bool,
}

impl Default for FooterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            height: "40px".to_string(),
            content: "© 2024 Ultrafast Gateway".to_string(),
            show_version: true,
            show_links: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentConfig {
    pub max_width: Option<String>,
    pub padding: String,
    pub spacing: String,
}

impl Default for ContentConfig {
    fn default() -> Self {
        Self {
            max_width: Some("1400px".to_string()),
            padding: "24px".to_string(),
            spacing: "24px".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsiveBreakpoints {
    pub mobile: String,
    pub tablet: String,
    pub desktop: String,
    pub wide: String,
}

impl Default for ResponsiveBreakpoints {
    fn default() -> Self {
        Self {
            mobile: "640px".to_string(),
            tablet: "768px".to_string(),
            desktop: "1024px".to_string(),
            wide: "1280px".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrandingConfig {
    pub logo_url: Option<String>,
    pub favicon_url: Option<String>,
    pub company_name: String,
    pub company_url: Option<String>,
    pub custom_css: Option<String>,
    pub custom_js: Option<String>,
}

impl Default for BrandingConfig {
    fn default() -> Self {
        Self {
            logo_url: None,
            favicon_url: None,
            company_name: "Ultrafast AI".to_string(),
            company_url: Some("https://ultrafast.ai".to_string()),
            custom_css: None,
            custom_js: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontConfig {
    pub primary_font: String,
    pub secondary_font: String,
    pub monospace_font: String,
    pub font_sizes: FontSizes,
    pub font_weights: FontWeights,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            primary_font: "Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif".to_string(),
            secondary_font: "Inter, sans-serif".to_string(),
            monospace_font: "'JetBrains Mono', 'Monaco', 'Consolas', monospace".to_string(),
            font_sizes: FontSizes::default(),
            font_weights: FontWeights::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontSizes {
    pub xs: String,
    pub sm: String,
    pub base: String,
    pub lg: String,
    pub xl: String,
    pub xxl: String,
}

impl Default for FontSizes {
    fn default() -> Self {
        Self {
            xs: "12px".to_string(),
            sm: "14px".to_string(),
            base: "16px".to_string(),
            lg: "18px".to_string(),
            xl: "20px".to_string(),
            xxl: "24px".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontWeights {
    pub light: u16,
    pub normal: u16,
    pub medium: u16,
    pub semibold: u16,
    pub bold: u16,
}

impl Default for FontWeights {
    fn default() -> Self {
        Self {
            light: 300,
            normal: 400,
            medium: 500,
            semibold: 600,
            bold: 700,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorPalette {
    pub primary: ColorScale,
    pub secondary: ColorScale,
    pub accent: ColorScale,
    pub success: ColorScale,
    pub warning: ColorScale,
    pub error: ColorScale,
    pub neutral: ColorScale,
    pub background: BackgroundColors,
    pub text: TextColors,
}

impl Default for ColorPalette {
    fn default() -> Self {
        Self {
            primary: ColorScale::blue(),
            secondary: ColorScale::slate(),
            accent: ColorScale::purple(),
            success: ColorScale::green(),
            warning: ColorScale::yellow(),
            error: ColorScale::red(),
            neutral: ColorScale::gray(),
            background: BackgroundColors::default(),
            text: TextColors::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorScale {
    pub c50: String,
    pub c100: String,
    pub c200: String,
    pub c300: String,
    pub c400: String,
    pub c500: String,
    pub c600: String,
    pub c700: String,
    pub c800: String,
    pub c900: String,
}

impl ColorScale {
    pub fn blue() -> Self {
        Self {
            c50: "#eff6ff".to_string(),
            c100: "#dbeafe".to_string(),
            c200: "#bfdbfe".to_string(),
            c300: "#93c5fd".to_string(),
            c400: "#60a5fa".to_string(),
            c500: "#3b82f6".to_string(),
            c600: "#2563eb".to_string(),
            c700: "#1d4ed8".to_string(),
            c800: "#1e40af".to_string(),
            c900: "#1e3a8a".to_string(),
        }
    }
    
    pub fn slate() -> Self {
        Self {
            c50: "#f8fafc".to_string(),
            c100: "#f1f5f9".to_string(),
            c200: "#e2e8f0".to_string(),
            c300: "#cbd5e1".to_string(),
            c400: "#94a3b8".to_string(),
            c500: "#64748b".to_string(),
            c600: "#475569".to_string(),
            c700: "#334155".to_string(),
            c800: "#1e293b".to_string(),
            c900: "#0f172a".to_string(),
        }
    }
    
    pub fn purple() -> Self {
        Self {
            c50: "#faf5ff".to_string(),
            c100: "#f3e8ff".to_string(),
            c200: "#e9d5ff".to_string(),
            c300: "#d8b4fe".to_string(),
            c400: "#c084fc".to_string(),
            c500: "#a855f7".to_string(),
            c600: "#9333ea".to_string(),
            c700: "#7c3aed".to_string(),
            c800: "#6d28d9".to_string(),
            c900: "#581c87".to_string(),
        }
    }
    
    pub fn green() -> Self {
        Self {
            c50: "#f0fdf4".to_string(),
            c100: "#dcfce7".to_string(),
            c200: "#bbf7d0".to_string(),
            c300: "#86efac".to_string(),
            c400: "#4ade80".to_string(),
            c500: "#22c55e".to_string(),
            c600: "#16a34a".to_string(),
            c700: "#15803d".to_string(),
            c800: "#166534".to_string(),
            c900: "#14532d".to_string(),
        }
    }
    
    pub fn yellow() -> Self {
        Self {
            c50: "#fefce8".to_string(),
            c100: "#fef3c7".to_string(),
            c200: "#fde68a".to_string(),
            c300: "#fcd34d".to_string(),
            c400: "#fbbf24".to_string(),
            c500: "#f59e0b".to_string(),
            c600: "#d97706".to_string(),
            c700: "#b45309".to_string(),
            c800: "#92400e".to_string(),
            c900: "#78350f".to_string(),
        }
    }
    
    pub fn red() -> Self {
        Self {
            c50: "#fef2f2".to_string(),
            c100: "#fee2e2".to_string(),
            c200: "#fecaca".to_string(),
            c300: "#fca5a5".to_string(),
            c400: "#f87171".to_string(),
            c500: "#ef4444".to_string(),
            c600: "#dc2626".to_string(),
            c700: "#b91c1c".to_string(),
            c800: "#991b1b".to_string(),
            c900: "#7f1d1d".to_string(),
        }
    }
    
    pub fn gray() -> Self {
        Self {
            c50: "#f9fafb".to_string(),
            c100: "#f3f4f6".to_string(),
            c200: "#e5e7eb".to_string(),
            c300: "#d1d5db".to_string(),
            c400: "#9ca3af".to_string(),
            c500: "#6b7280".to_string(),
            c600: "#4b5563".to_string(),
            c700: "#374151".to_string(),
            c800: "#1f2937".to_string(),
            c900: "#111827".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundColors {
    pub primary: String,
    pub secondary: String,
    pub surface: String,
    pub card: String,
}

impl Default for BackgroundColors {
    fn default() -> Self {
        Self {
            primary: "#ffffff".to_string(),
            secondary: "#f8fafc".to_string(),
            surface: "#ffffff".to_string(),
            card: "#ffffff".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextColors {
    pub primary: String,
    pub secondary: String,
    pub tertiary: String,
    pub inverse: String,
}

impl Default for TextColors {
    fn default() -> Self {
        Self {
            primary: "#111827".to_string(),
            secondary: "#6b7280".to_string(),
            tertiary: "#9ca3af".to_string(),
            inverse: "#ffffff".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationConfig {
    pub enabled: bool,
    pub duration: AnimationDurations,
    pub easing: AnimationEasing,
    pub reduce_motion: bool,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            duration: AnimationDurations::default(),
            easing: AnimationEasing::default(),
            reduce_motion: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationDurations {
    pub fast: String,
    pub normal: String,
    pub slow: String,
}

impl Default for AnimationDurations {
    fn default() -> Self {
        Self {
            fast: "150ms".to_string(),
            normal: "300ms".to_string(),
            slow: "500ms".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationEasing {
    pub ease_in: String,
    pub ease_out: String,
    pub ease_in_out: String,
    pub bounce: String,
}

impl Default for AnimationEasing {
    fn default() -> Self {
        Self {
            ease_in: "cubic-bezier(0.4, 0, 1, 1)".to_string(),
            ease_out: "cubic-bezier(0, 0, 0.2, 1)".to_string(),
            ease_in_out: "cubic-bezier(0.4, 0, 0.2, 1)".to_string(),
            bounce: "cubic-bezier(0.68, -0.55, 0.265, 1.55)".to_string(),
        }
    }
}

/// Feature configuration with granular control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureConfig {
    pub real_time_updates: bool,
    pub websocket_enabled: bool,
    pub advanced_analytics: bool,
    pub custom_dashboards: bool,
    pub user_customization: bool,
    pub export_functionality: bool,
    pub alert_management: bool,
    pub user_management: bool,
    pub api_keys_management: bool,
    pub cost_tracking: bool,
    pub provider_health: bool,
    pub error_analytics: bool,
    pub performance_monitoring: bool,
    pub audit_logging: bool,
    pub dark_mode: bool,
    pub multi_language: bool,
    pub responsive_design: bool,
    pub keyboard_shortcuts: bool,
    pub contextual_help: bool,
    pub data_export: ExportFeatures,
    pub integrations: IntegrationFeatures,
}

impl FeatureConfig {
    pub fn all_enabled() -> Self {
        Self {
            real_time_updates: true,
            websocket_enabled: true,
            advanced_analytics: true,
            custom_dashboards: true,
            user_customization: true,
            export_functionality: true,
            alert_management: true,
            user_management: true,
            api_keys_management: true,
            cost_tracking: true,
            provider_health: true,
            error_analytics: true,
            performance_monitoring: true,
            audit_logging: true,
            dark_mode: true,
            multi_language: true,
            responsive_design: true,
            keyboard_shortcuts: true,
            contextual_help: true,
            data_export: ExportFeatures::all_enabled(),
            integrations: IntegrationFeatures::all_enabled(),
        }
    }
    
    pub fn minimal() -> Self {
        Self {
            real_time_updates: true,
            websocket_enabled: true,
            advanced_analytics: false,
            custom_dashboards: false,
            user_customization: false,
            export_functionality: false,
            alert_management: false,
            user_management: false,
            api_keys_management: false,
            cost_tracking: true,
            provider_health: true,
            error_analytics: true,
            performance_monitoring: true,
            audit_logging: false,
            dark_mode: true,
            multi_language: false,
            responsive_design: true,
            keyboard_shortcuts: false,
            contextual_help: false,
            data_export: ExportFeatures::minimal(),
            integrations: IntegrationFeatures::minimal(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportFeatures {
    pub csv_export: bool,
    pub json_export: bool,
    pub pdf_reports: bool,
    pub scheduled_reports: bool,
}

impl ExportFeatures {
    pub fn all_enabled() -> Self {
        Self {
            csv_export: true,
            json_export: true,
            pdf_reports: true,
            scheduled_reports: true,
        }
    }
    
    pub fn minimal() -> Self {
        Self {
            csv_export: true,
            json_export: true,
            pdf_reports: false,
            scheduled_reports: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationFeatures {
    pub prometheus: bool,
    pub grafana: bool,
    pub slack: bool,
    pub discord: bool,
    pub email: bool,
    pub webhooks: bool,
    pub api_access: bool,
}

impl IntegrationFeatures {
    pub fn all_enabled() -> Self {
        Self {
            prometheus: true,
            grafana: true,
            slack: true,
            discord: true,
            email: true,
            webhooks: true,
            api_access: true,
        }
    }
    
    pub fn minimal() -> Self {
        Self {
            prometheus: true,
            grafana: false,
            slack: false,
            discord: false,
            email: false,
            webhooks: false,
            api_access: true,
        }
    }
}

// Forward declarations for other configuration sections
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityConfig {
    pub enabled: bool,
    pub csp_enabled: bool,
    pub secure_headers: bool,
    pub session_timeout: Duration,
    pub max_login_attempts: u32,
    pub require_https: bool,
    pub api_rate_limiting: bool,
}

impl SecurityConfig {
    pub fn secure_defaults() -> Self {
        Self {
            enabled: true,
            csp_enabled: true,
            secure_headers: true,
            session_timeout: Duration::from_secs(3600), // 1 hour
            max_login_attempts: 5,
            require_https: true,
            api_rate_limiting: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PerformanceConfig {
    pub cache_enabled: bool,
    pub cache_ttl: Duration,
    pub compression_enabled: bool,
    pub lazy_loading: bool,
    pub prefetch_enabled: bool,
    pub resource_hints: bool,
    pub service_worker: bool,
}

impl PerformanceConfig {
    pub fn optimized() -> Self {
        Self {
            cache_enabled: true,
            cache_ttl: Duration::from_secs(300),
            compression_enabled: true,
            lazy_loading: true,
            prefetch_enabled: true,
            resource_hints: true,
            service_worker: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrationConfig {
    pub prometheus_enabled: bool,
    pub grafana_url: Option<String>,
    pub slack_webhook: Option<String>,
    pub email_settings: Option<EmailSettings>,
    pub webhook_endpoints: Vec<WebhookEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailSettings {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub from_address: String,
    pub use_tls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEndpoint {
    pub name: String,
    pub url: String,
    pub events: Vec<String>,
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomizationConfig {
    pub allow_user_themes: bool,
    pub allow_layout_changes: bool,
    pub allow_widget_customization: bool,
    pub max_custom_widgets: u32,
    pub preset_layouts: Vec<PresetLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetLayout {
    pub name: String,
    pub description: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalyticsConfig {
    pub enabled: bool,
    pub retention_days: u32,
    pub predictive_analytics: bool,
    pub anomaly_detection: bool,
    pub custom_metrics: Vec<CustomMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomMetric {
    pub name: String,
    pub description: String,
    pub query: String,
    pub chart_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationConfig {
    pub enabled: bool,
    pub channels: Vec<NotificationChannel>,
    pub alert_rules: Vec<AlertRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationChannel {
    pub name: String,
    pub channel_type: NotificationChannelType,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationChannelType {
    Email,
    Slack,
    Discord,
    Webhook,
    SMS,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub name: String,
    pub condition: String,
    pub severity: AlertSeverity,
    pub channels: Vec<String>,
    pub cooldown: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvironmentConfig {
    pub environment: Environment,
    pub debug_mode: bool,
    pub log_level: String,
    pub feature_flags: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Environment {
    Development,
    Staging,
    Production,
    Testing,
}

impl Default for Environment {
    fn default() -> Self {
        Environment::Development
    }
}

/// Configuration manager for loading, validating, and managing dashboard configuration
pub struct ConfigurationManager {
    config: Arc<RwLock<DashboardConfiguration>>,
    config_path: PathBuf,
    hot_reload: Option<hot_reload::HotReloader>,
    validator: validator::ConfigValidator,
    environment: environment::EnvironmentManager,
}

impl ConfigurationManager {
    pub async fn new(config_path: PathBuf) -> Result<Self, GatewayError> {
        let config = loader::ConfigLoader::load_from_file(&config_path).await?;
        let validator = validator::ConfigValidator::new();
        let environment = environment::EnvironmentManager::new();
        
        // Validate configuration
        validator.validate(&config)?;
        
        let manager = Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
            hot_reload: None,
            validator,
            environment,
        };
        
        Ok(manager)
    }
    
    pub async fn with_hot_reload(mut self) -> Result<Self, GatewayError> {
        let hot_reloader = hot_reload::HotReloader::new(
            self.config_path.clone(),
            self.config.clone(),
        ).await?;
        
        self.hot_reload = Some(hot_reloader);
        Ok(self)
    }
    
    pub async fn get_config(&self) -> DashboardConfiguration {
        let config = self.config.read().await;
        config.clone()
    }
    
    pub async fn update_config(&self, new_config: DashboardConfiguration) -> Result<(), GatewayError> {
        // Validate new configuration
        self.validator.validate(&new_config)?;
        
        // Apply environment overrides
        let final_config = self.environment.apply_overrides(new_config).await?;
        
        // Update configuration
        {
            let mut config = self.config.write().await;
            *config = final_config;
        }
        
        // Save to file
        loader::ConfigLoader::save_to_file(&self.config_path, &self.get_config().await).await?;
        
        Ok(())
    }
    
    pub async fn reload_config(&self) -> Result<(), GatewayError> {
        let new_config = loader::ConfigLoader::load_from_file(&self.config_path).await?;
        self.validator.validate(&new_config)?;
        
        let final_config = self.environment.apply_overrides(new_config).await?;
        
        {
            let mut config = self.config.write().await;
            *config = final_config;
        }
        
        Ok(())
    }
    
    pub async fn start_hot_reload(&self) -> Result<(), GatewayError> {
        if let Some(hot_reloader) = &self.hot_reload {
            hot_reloader.start().await?;
        }
        Ok(())
    }
    
    pub async fn stop_hot_reload(&self) -> Result<(), GatewayError> {
        if let Some(hot_reloader) = &self.hot_reload {
            hot_reloader.stop().await?;
        }
        Ok(())
    }
    
    pub async fn validate_config(&self) -> Result<Vec<String>, GatewayError> {
        let config = self.get_config().await;
        self.validator.validate_detailed(&config)
    }
    
    pub async fn get_feature_flag(&self, flag_name: &str) -> bool {
        let config = self.config.read().await;
        config.environment.feature_flags
            .get(flag_name)
            .copied()
            .unwrap_or(false)
    }
    
    pub async fn set_feature_flag(&self, flag_name: String, enabled: bool) -> Result<(), GatewayError> {
        {
            let mut config = self.config.write().await;
            config.environment.feature_flags.insert(flag_name, enabled);
        }
        
        // Save configuration
        loader::ConfigLoader::save_to_file(&self.config_path, &self.get_config().await).await?;
        
        Ok(())
    }
}