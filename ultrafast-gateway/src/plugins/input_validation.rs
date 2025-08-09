use crate::gateway_error::GatewayError;
use crate::plugins::{PluginHooks, PluginLifecycle, PluginMetadata, PluginState};
use axum::body::Body;
use axum::http::Request;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationConfig {
    pub enabled: bool,
    pub max_request_size: usize,
    pub max_model_name_length: usize,
}

#[derive(Debug, Clone)]
pub struct InputValidationPlugin {
    meta: PluginMetadata,
    config: ValidationConfig,
}

impl InputValidationPlugin {
    pub fn new(config: ValidationConfig) -> Self {
        Self {
            meta: PluginMetadata {
                id: uuid::Uuid::new_v4().to_string(),
                name: "input_validation".to_string(),
                version: "1.0.0".to_string(),
                enabled: config.enabled,
                state: PluginState::Inactive,
                dependencies: vec![],
                priority: 4,
                last_error: None,
            },
            config,
        }
    }

    pub fn enabled(&self) -> bool {
        self.meta.enabled
    }
}

#[async_trait::async_trait]
impl PluginLifecycle for InputValidationPlugin {
    async fn initialize(&mut self) -> Result<(), GatewayError> {
        Ok(())
    }
    async fn start(&mut self) -> Result<(), GatewayError> {
        Ok(())
    }
    async fn stop(&mut self) -> Result<(), GatewayError> {
        Ok(())
    }
    async fn cleanup(&mut self) -> Result<(), GatewayError> {
        Ok(())
    }
    async fn health_check(&self) -> Result<(), GatewayError> {
        Ok(())
    }
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }
    fn metadata_mut(&mut self) -> &mut PluginMetadata {
        &mut self.meta
    }
}

#[async_trait::async_trait]
impl PluginHooks for InputValidationPlugin {
    async fn before_request(&self, request: &mut Request<Body>) -> Result<(), GatewayError> {
        if !self.config.enabled {
            return Ok(());
        }

        // Lightweight check: respect Content-Length if present
        if let Some(len) = request
            .headers()
            .get(axum::http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| usize::from_str(s).ok())
        {
            if len > self.config.max_request_size {
                return Err(GatewayError::InvalidRequest {
                    message: "Request too large".into(),
                });
            }
        }

        // Skip deep JSON validation to avoid consuming the body; plugin is intentionally permissive
        Ok(())
    }

    async fn after_response(&self, _response: &mut Response<Body>) -> Result<(), GatewayError> {
        Ok(())
    }
    async fn on_error(&self, _error: &GatewayError) -> Result<(), GatewayError> {
        Ok(())
    }
}

pub fn build_input_validation_plugin(enabled: bool) -> InputValidationPlugin {
    InputValidationPlugin::new(ValidationConfig {
        enabled,
        max_request_size: 50 * 1024 * 1024,
        max_model_name_length: 200,
    })
}
