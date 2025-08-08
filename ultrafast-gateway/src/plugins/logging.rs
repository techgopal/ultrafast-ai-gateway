use crate::config::PluginConfig;
use crate::gateway_error::GatewayError;
use axum::body::Body;
use axum::http::{Request, Response};
use serde_json::json;

#[derive(Clone, Debug)]
pub struct LoggingPlugin {
    name: String,
    enabled: bool,
    log_requests: bool,
    log_responses: bool,
    log_errors: bool,
}

impl LoggingPlugin {
    pub fn new(config: &PluginConfig) -> Result<Self, GatewayError> {
        let log_requests = config
            .config
            .get("log_requests")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let log_responses = config
            .config
            .get("log_responses")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let log_errors = config
            .config
            .get("log_errors")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Ok(Self {
            name: config.name.clone(),
            enabled: config.enabled,
            log_requests,
            log_responses,
            log_errors,
        })
    }

    fn extract_request_info(&self, request: &Request<Body>) -> serde_json::Value {
        json!({
            "method": request.method().as_str(),
            "uri": request.uri().to_string(),
            "headers": {
                "user_agent": request.headers().get("user-agent").and_then(|h| h.to_str().ok()),
                "content_type": request.headers().get("content-type").and_then(|h| h.to_str().ok()),
                "authorization": if request.headers().contains_key("authorization") { "present" } else { "absent" },
            }
        })
    }

    fn extract_response_info(&self, response: &Response<Body>) -> serde_json::Value {
        json!({
            "status": response.status().as_u16(),
            "headers": {
                "content_type": response.headers().get("content-type").and_then(|h| h.to_str().ok()),
                "content_length": response.headers().get("content-length").and_then(|h| h.to_str().ok()),
            }
        })
    }
}

impl LoggingPlugin {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub async fn before_request(&self, request: &mut Request<Body>) -> Result<(), GatewayError> {
        if self.log_requests {
            let request_info = self.extract_request_info(request);
            tracing::info!(
                "Request received: {:?}",
                serde_json::to_string(&request_info).unwrap_or_default()
            );
        }
        Ok(())
    }

    pub async fn after_response(&self, response: &mut Response<Body>) -> Result<(), GatewayError> {
        if self.log_responses {
            let response_info = self.extract_response_info(response);
            tracing::info!(
                "Response sent: {:?}",
                serde_json::to_string(&response_info).unwrap_or_default()
            );
        }
        Ok(())
    }

    pub async fn on_error(&self, error: &GatewayError) -> Result<(), GatewayError> {
        if self.log_errors {
            tracing::error!(
                "Request failed: {:?}, error_type: {:?}",
                error,
                std::mem::discriminant(error)
            );
        }
        Ok(())
    }
}
