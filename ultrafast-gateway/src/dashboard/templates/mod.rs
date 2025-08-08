// Template Engine for Dashboard
// Modern templating system with caching, compilation, and security features

use crate::gateway_error::GatewayError;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};

pub mod handlebars_engine;
pub mod minijinja_engine;
pub mod security;
pub mod cache;

/// Main template engine interface
#[async_trait::async_trait]
pub trait TemplateRenderer: Send + Sync {
    /// Render a template with the given context
    async fn render(&self, template_name: &str, context: &serde_json::Value) -> Result<String, GatewayError>;
    
    /// Render a template string directly
    async fn render_string(&self, template: &str, context: &serde_json::Value) -> Result<String, GatewayError>;
    
    /// Check if a template exists
    async fn template_exists(&self, template_name: &str) -> bool;
    
    /// Register a template
    async fn register_template(&self, name: String, template: String) -> Result<(), GatewayError>;
    
    /// Register a helper function
    async fn register_helper(&self, name: String, helper: Box<dyn TemplateHelper>) -> Result<(), GatewayError>;
    
    /// Clear template cache
    async fn clear_cache(&self) -> Result<(), GatewayError>;
    
    /// Get template compilation statistics
    async fn get_stats(&self) -> TemplateStats;
}

/// Template helper trait for custom functions
#[async_trait::async_trait]
pub trait TemplateHelper: Send + Sync {
    async fn call(&self, args: &[serde_json::Value]) -> Result<serde_json::Value, GatewayError>;
}

/// Main template engine that orchestrates different rendering backends
pub struct TemplateEngine {
    renderer: Arc<dyn TemplateRenderer>,
    cache: Arc<cache::TemplateCache>,
    security: Arc<security::TemplateSecurity>,
    config: TemplateConfig,
    stats: Arc<RwLock<TemplateEngineStats>>,
}

impl TemplateEngine {
    pub fn new() -> Self {
        let config = TemplateConfig::default();
        let renderer = Self::create_renderer(&config);
        let cache = Arc::new(cache::TemplateCache::new(config.cache.clone()));
        let security = Arc::new(security::TemplateSecurity::new(config.security.clone()));
        
        Self {
            renderer,
            cache,
            security,
            config,
            stats: Arc::new(RwLock::new(TemplateEngineStats::default())),
        }
    }
    
    pub fn with_config(config: TemplateConfig) -> Self {
        let renderer = Self::create_renderer(&config);
        let cache = Arc::new(cache::TemplateCache::new(config.cache.clone()));
        let security = Arc::new(security::TemplateSecurity::new(config.security.clone()));
        
        Self {
            renderer,
            cache,
            security,
            config,
            stats: Arc::new(RwLock::new(TemplateEngineStats::default())),
        }
    }
    
    fn create_renderer(config: &TemplateConfig) -> Arc<dyn TemplateRenderer> {
        match config.engine_type {
            TemplateEngineType::Handlebars => Arc::new(handlebars_engine::HandlebarsRenderer::new(config.clone())),
            TemplateEngineType::MiniJinja => Arc::new(minijinja_engine::MiniJinjaRenderer::new(config.clone())),
        }
    }
    
    /// Render a template with full security and caching
    pub async fn render(&self, template_name: &str, context: &serde_json::Value) -> Result<String, GatewayError> {
        let start_time = Instant::now();
        
        // Security validation
        if !self.security.validate_template_name(template_name) {
            return Err(GatewayError::InvalidRequest {
                message: format!("Invalid template name: {}", template_name)
            });
        }
        
        if !self.security.validate_context(context) {
            return Err(GatewayError::InvalidRequest {
                message: "Template context contains unsafe content".to_string()
            });
        }
        
        // Check cache first
        let cache_key = self.generate_cache_key(template_name, context);
        if let Some(cached_result) = self.cache.get(&cache_key).await {
            self.update_stats(template_name, start_time.elapsed(), true).await;
            return Ok(cached_result);
        }
        
        // Render template
        let result = self.renderer.render(template_name, context).await?;
        
        // Apply security post-processing
        let secure_result = self.security.sanitize_output(&result)?;
        
        // Cache the result
        self.cache.set(cache_key, secure_result.clone()).await;
        
        // Update statistics
        self.update_stats(template_name, start_time.elapsed(), false).await;
        
        Ok(secure_result)
    }
    
    /// Render template string directly
    pub async fn render_string(&self, template: &str, context: &serde_json::Value) -> Result<String, GatewayError> {
        let start_time = Instant::now();
        
        // Security validation
        if !self.security.validate_template_content(template) {
            return Err(GatewayError::InvalidRequest {
                message: "Template content contains unsafe elements".to_string()
            });
        }
        
        if !self.security.validate_context(context) {
            return Err(GatewayError::InvalidRequest {
                message: "Template context contains unsafe content".to_string()
            });
        }
        
        // Check cache for inline templates
        let cache_key = self.generate_inline_cache_key(template, context);
        if let Some(cached_result) = self.cache.get(&cache_key).await {
            self.update_stats("inline", start_time.elapsed(), true).await;
            return Ok(cached_result);
        }
        
        // Render template
        let result = self.renderer.render_string(template, context).await?;
        
        // Apply security post-processing
        let secure_result = self.security.sanitize_output(&result)?;
        
        // Cache the result
        self.cache.set(cache_key, secure_result.clone()).await;
        
        // Update statistics
        self.update_stats("inline", start_time.elapsed(), false).await;
        
        Ok(secure_result)
    }
    
    /// Register a new template
    pub async fn register_template(&self, name: String, template: String) -> Result<(), GatewayError> {
        // Security validation
        if !self.security.validate_template_name(&name) {
            return Err(GatewayError::InvalidRequest {
                message: format!("Invalid template name: {}", name)
            });
        }
        
        if !self.security.validate_template_content(&template) {
            return Err(GatewayError::InvalidRequest {
                message: "Template content contains unsafe elements".to_string()
            });
        }
        
        // Register with renderer
        self.renderer.register_template(name.clone(), template).await?;
        
        // Clear related cache entries
        self.cache.invalidate_prefix(&name).await;
        
        tracing::info!("Registered template: {}", name);
        Ok(())
    }
    
    /// Register a template helper function
    pub async fn register_helper(&self, name: String, helper: Box<dyn TemplateHelper>) -> Result<(), GatewayError> {
        self.renderer.register_helper(name, helper).await
    }
    
    /// Load templates from directory
    pub async fn load_templates_from_directory(&self, dir_path: &str) -> Result<(), GatewayError> {
        use std::fs;
        use std::path::Path;
        
        let dir = Path::new(dir_path);
        if !dir.exists() || !dir.is_dir() {
            return Err(GatewayError::Configuration {
                message: format!("Template directory does not exist: {}", dir_path)
            });
        }
        
        self.load_templates_recursive(dir, "").await
    }
    
    async fn load_templates_recursive(&self, dir: &std::path::Path, prefix: &str) -> Result<(), GatewayError> {
        use std::fs;
        
        let entries = fs::read_dir(dir)
            .map_err(|e| GatewayError::Configuration {
                message: format!("Failed to read template directory: {}", e)
            })?;
        
        for entry in entries {
            let entry = entry.map_err(|e| GatewayError::Configuration {
                message: format!("Failed to read directory entry: {}", e)
            })?;
            
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy();
            
            if path.is_dir() {
                // Recursively load subdirectories
                let new_prefix = if prefix.is_empty() {
                    file_name.to_string()
                } else {
                    format!("{}/{}", prefix, file_name)
                };
                self.load_templates_recursive(&path, &new_prefix).await?;
            } else if let Some(extension) = path.extension() {
                if extension == "html" || extension == "hbs" || extension == "j2" {
                    // Load template file
                    let template_content = fs::read_to_string(&path)
                        .map_err(|e| GatewayError::Configuration {
                            message: format!("Failed to read template file {:?}: {}", path, e)
                        })?;
                    
                    let template_name = if prefix.is_empty() {
                        file_name.trim_end_matches(".html")
                               .trim_end_matches(".hbs")
                               .trim_end_matches(".j2")
                               .to_string()
                    } else {
                        format!("{}/{}",
                            prefix,
                            file_name.trim_end_matches(".html")
                                   .trim_end_matches(".hbs")
                                   .trim_end_matches(".j2")
                        )
                    };
                    
                    self.register_template(template_name, template_content).await?;
                }
            }
        }
        
        Ok(())
    }
    
    /// Get template engine statistics
    pub async fn get_stats(&self) -> TemplateEngineStats {
        let stats = self.stats.read().await;
        stats.clone()
    }
    
    /// Clear all caches
    pub async fn clear_cache(&self) -> Result<(), GatewayError> {
        self.cache.clear().await;
        self.renderer.clear_cache().await
    }
    
    fn generate_cache_key(&self, template_name: &str, context: &serde_json::Value) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        template_name.hash(&mut hasher);
        context.to_string().hash(&mut hasher);
        format!("template:{}:{}", template_name, hasher.finish())
    }
    
    fn generate_inline_cache_key(&self, template: &str, context: &serde_json::Value) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        template.hash(&mut hasher);
        context.to_string().hash(&mut hasher);
        format!("inline:{}", hasher.finish())
    }
    
    async fn update_stats(&self, template_name: &str, duration: Duration, cache_hit: bool) {
        let mut stats = self.stats.write().await;
        stats.total_renders += 1;
        stats.total_render_time += duration;
        
        if cache_hit {
            stats.cache_hits += 1;
        } else {
            stats.cache_misses += 1;
        }
        
        let template_stats = stats.template_stats.entry(template_name.to_string()).or_default();
        template_stats.render_count += 1;
        template_stats.total_time += duration;
        template_stats.average_time = template_stats.total_time / template_stats.render_count as u32;
        
        if cache_hit {
            template_stats.cache_hits += 1;
        }
    }
}

/// Template engine configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    pub engine_type: TemplateEngineType,
    pub template_directory: String,
    pub auto_reload: bool,
    pub strict_mode: bool,
    pub cache: cache::CacheConfig,
    pub security: security::SecurityConfig,
    pub performance: PerformanceConfig,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            engine_type: TemplateEngineType::Handlebars,
            template_directory: "templates".to_string(),
            auto_reload: true,
            strict_mode: true,
            cache: cache::CacheConfig::default(),
            security: security::SecurityConfig::default(),
            performance: PerformanceConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TemplateEngineType {
    Handlebars,
    MiniJinja,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub compile_timeout: Duration,
    pub render_timeout: Duration,
    pub max_template_size: usize,
    pub max_context_size: usize,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            compile_timeout: Duration::from_secs(5),
            render_timeout: Duration::from_secs(10),
            max_template_size: 1024 * 1024, // 1MB
            max_context_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

/// Template rendering statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemplateEngineStats {
    pub total_renders: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub total_render_time: Duration,
    pub average_render_time: Duration,
    pub template_stats: HashMap<String, TemplateStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemplateStats {
    pub render_count: u64,
    pub cache_hits: u64,
    pub total_time: Duration,
    pub average_time: Duration,
    pub last_rendered: Option<Instant>,
    pub errors: u64,
}

impl TemplateEngineStats {
    pub fn cache_hit_rate(&self) -> f64 {
        if self.total_renders == 0 {
            0.0
        } else {
            self.cache_hits as f64 / self.total_renders as f64
        }
    }
    
    pub fn average_render_time_ms(&self) -> f64 {
        if self.total_renders == 0 {
            0.0
        } else {
            self.total_render_time.as_millis() as f64 / self.total_renders as f64
        }
    }
}

/// Built-in template helpers
pub mod helpers {
    use super::*;
    
    pub struct FormatDateHelper;
    
    #[async_trait::async_trait]
    impl TemplateHelper for FormatDateHelper {
        async fn call(&self, args: &[serde_json::Value]) -> Result<serde_json::Value, GatewayError> {
            if args.len() != 2 {
                return Err(GatewayError::InvalidRequest {
                    message: "format_date helper requires 2 arguments: timestamp and format".to_string()
                });
            }
            
            let timestamp = args[0].as_i64().ok_or_else(|| GatewayError::InvalidRequest {
                message: "First argument must be a timestamp".to_string()
            })?;
            
            let format = args[1].as_str().ok_or_else(|| GatewayError::InvalidRequest {
                message: "Second argument must be a format string".to_string()
            })?;
            
            let datetime = chrono::DateTime::from_timestamp(timestamp, 0)
                .ok_or_else(|| GatewayError::InvalidRequest {
                    message: "Invalid timestamp".to_string()
                })?;
            
            let formatted = datetime.format(format).to_string();
            Ok(serde_json::Value::String(formatted))
        }
    }
    
    pub struct NumberFormatHelper;
    
    #[async_trait::async_trait]
    impl TemplateHelper for NumberFormatHelper {
        async fn call(&self, args: &[serde_json::Value]) -> Result<serde_json::Value, GatewayError> {
            if args.is_empty() || args.len() > 2 {
                return Err(GatewayError::InvalidRequest {
                    message: "number_format helper requires 1-2 arguments: number and optional decimal places".to_string()
                });
            }
            
            let number = args[0].as_f64().ok_or_else(|| GatewayError::InvalidRequest {
                message: "First argument must be a number".to_string()
            })?;
            
            let decimal_places = if args.len() > 1 {
                args[1].as_u64().unwrap_or(2) as usize
            } else {
                2
            };
            
            let formatted = format!("{:.prec$}", number, prec = decimal_places);
            Ok(serde_json::Value::String(formatted))
        }
    }
    
    pub struct TruncateHelper;
    
    #[async_trait::async_trait]
    impl TemplateHelper for TruncateHelper {
        async fn call(&self, args: &[serde_json::Value]) -> Result<serde_json::Value, GatewayError> {
            if args.len() != 2 {
                return Err(GatewayError::InvalidRequest {
                    message: "truncate helper requires 2 arguments: string and length".to_string()
                });
            }
            
            let text = args[0].as_str().ok_or_else(|| GatewayError::InvalidRequest {
                message: "First argument must be a string".to_string()
            })?;
            
            let length = args[1].as_u64().ok_or_else(|| GatewayError::InvalidRequest {
                message: "Second argument must be a length".to_string()
            })? as usize;
            
            let truncated = if text.len() > length {
                format!("{}...", &text[..length.saturating_sub(3)])
            } else {
                text.to_string()
            };
            
            Ok(serde_json::Value::String(truncated))
        }
    }
}