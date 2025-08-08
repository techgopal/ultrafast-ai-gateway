use crate::gateway_error::GatewayError;
use crate::server::AppState;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::{error, warn};

const MAX_REQUEST_SIZE: usize = 50 * 1024 * 1024; // 50MB
const MAX_MESSAGE_LENGTH: usize = 100_000; // 100k characters per message
const MAX_MESSAGES_COUNT: usize = 100; // Max 100 messages in conversation
const MAX_MODEL_NAME_LENGTH: usize = 200;
const MAX_JSON_DEPTH: usize = 10;

// Dangerous patterns that should be blocked
static BLOCKED_PATTERNS: &[&str] = &[
    // Script injection attempts
    "<script",
    "</script>",
    "javascript:",
    "vbscript:",
    // SQL injection patterns
    "' OR '1'='1",
    "'; DROP TABLE",
    "UNION SELECT",
    // Command injection
    "|",
    "&",
    ";",
    "`",
    "$(",
    // Directory traversal
    "../",
    "..\\",
    "/etc/passwd",
    "/etc/shadow",
    // Template injection
    "{{",
    "}}",
    "${",
    "<%",
    "%>",
    // Prompt injection markers
    "IGNORE PREVIOUS INSTRUCTIONS",
    "SYSTEM:",
    "\\n\\nUser:",
    "\\n\\nAssistant:",
];

// Suspicious patterns that should be logged but not blocked
static SUSPICIOUS_PATTERNS: &[&str] = &[
    "password",
    "secret",
    "api_key",
    "token",
    "credentials",
    "admin",
    "root",
    "sudo",
    "chmod",
    "rm -rf",
    "eval(",
    "exec(",
    "system(",
    "shell_exec",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    pub max_request_size: usize,
    pub max_message_length: usize,
    pub max_messages_count: usize,
    pub max_model_name_length: usize,
    pub max_json_depth: usize,
    pub block_dangerous_patterns: bool,
    pub log_suspicious_patterns: bool,
    pub strict_mode: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_request_size: MAX_REQUEST_SIZE,
            max_message_length: MAX_MESSAGE_LENGTH,
            max_messages_count: MAX_MESSAGES_COUNT,
            max_model_name_length: MAX_MODEL_NAME_LENGTH,
            max_json_depth: MAX_JSON_DEPTH,
            block_dangerous_patterns: true,
            log_suspicious_patterns: true,
            strict_mode: false,
        }
    }
}

pub struct InputValidator {
    config: ValidationConfig,
    blocked_patterns: HashSet<String>,
    suspicious_patterns: HashSet<String>,
}

impl InputValidator {
    pub fn new(config: ValidationConfig) -> Self {
        let blocked_patterns = BLOCKED_PATTERNS.iter().map(|s| s.to_lowercase()).collect();

        let suspicious_patterns = SUSPICIOUS_PATTERNS
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        Self {
            config,
            blocked_patterns,
            suspicious_patterns,
        }
    }

    pub fn validate_request(&self, body: &str, headers: &HeaderMap) -> Result<(), GatewayError> {
        // Size validation
        if body.len() > self.config.max_request_size {
            warn!("Request size too large: {} bytes", body.len());
            return Err(GatewayError::InvalidRequest {
                message: format!(
                    "Request size too large: {} bytes (max: {})",
                    body.len(),
                    self.config.max_request_size
                ),
            });
        }

        // Content-Type validation for POST requests
        if let Some(content_type) = headers.get("content-type") {
            if let Ok(ct_str) = content_type.to_str() {
                if !ct_str.starts_with("application/json")
                    && !ct_str.starts_with("multipart/form-data")
                {
                    warn!("Invalid content type: {}", ct_str);
                    return Err(GatewayError::InvalidRequest {
                        message: format!("Invalid content type: {ct_str}"),
                    });
                }
            }
        }

        // JSON structure validation
        if !body.is_empty() {
            self.validate_json_structure(body)?;
        }

        // Pattern validation
        if self.config.block_dangerous_patterns {
            self.check_dangerous_patterns(body)?;
        }

        if self.config.log_suspicious_patterns {
            self.check_suspicious_patterns(body);
        }

        // Chat-specific validation
        if body.contains("messages") || body.contains("\"model\"") {
            self.validate_chat_request(body)?;
        }

        Ok(())
    }

    fn validate_json_structure(&self, body: &str) -> Result<(), GatewayError> {
        // Parse JSON and check depth
        match serde_json::from_str::<serde_json::Value>(body) {
            Ok(json) => {
                let depth = self.calculate_json_depth(&json);
                if depth > self.config.max_json_depth {
                    warn!("JSON depth too deep: {}", depth);
                    return Err(GatewayError::InvalidRequest {
                        message: format!(
                            "JSON nesting too deep: {} levels (max: {})",
                            depth, self.config.max_json_depth
                        ),
                    });
                }
            }
            Err(e) => {
                if self.config.strict_mode {
                    warn!("Invalid JSON: {}", e);
                    return Err(GatewayError::InvalidRequest {
                        message: format!("Invalid JSON: {e}"),
                    });
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::only_used_in_recursion)]
    fn calculate_json_depth(&self, value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::Object(obj) => {
                1 + obj
                    .values()
                    .map(|v| self.calculate_json_depth(v))
                    .max()
                    .unwrap_or(0)
            }
            serde_json::Value::Array(arr) => {
                1 + arr
                    .iter()
                    .map(|v| self.calculate_json_depth(v))
                    .max()
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn check_dangerous_patterns(&self, text: &str) -> Result<(), GatewayError> {
        let text_lower = text.to_lowercase();

        for pattern in &self.blocked_patterns {
            if text_lower.contains(pattern) {
                error!("Dangerous pattern detected: {}", pattern);
                return Err(GatewayError::ContentFiltered {
                    message: "Request contains potentially dangerous content".to_string(),
                });
            }
        }
        Ok(())
    }

    fn check_suspicious_patterns(&self, text: &str) {
        let text_lower = text.to_lowercase();

        for pattern in &self.suspicious_patterns {
            if text_lower.contains(pattern) {
                warn!("Suspicious pattern detected: {}", pattern);
            }
        }
    }

    fn validate_chat_request(&self, body: &str) -> Result<(), GatewayError> {
        // Parse as a potential chat request
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            // Validate model name
            if let Some(model) = json.get("model") {
                if let Some(model_str) = model.as_str() {
                    if model_str.len() > self.config.max_model_name_length {
                        warn!("Model name too long: {} chars", model_str.len());
                        return Err(GatewayError::InvalidRequest {
                            message: format!(
                                "Model name too long: {} characters (max: {})",
                                model_str.len(),
                                self.config.max_model_name_length
                            ),
                        });
                    }

                    // Basic model name format validation
                    if !model_str.chars().all(|c| {
                        c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ':'
                    }) {
                        warn!("Invalid model name format: {}", model_str);
                        return Err(GatewayError::InvalidRequest {
                            message: "Invalid model name format".to_string(),
                        });
                    }
                }
            }

            // Validate messages array
            if let Some(messages) = json.get("messages") {
                if let Some(messages_array) = messages.as_array() {
                    if messages_array.len() > self.config.max_messages_count {
                        warn!("Too many messages: {}", messages_array.len());
                        return Err(GatewayError::InvalidRequest {
                            message: format!(
                                "Too many messages: {} (max: {})",
                                messages_array.len(),
                                self.config.max_messages_count
                            ),
                        });
                    }

                    // Validate individual messages
                    for (i, message) in messages_array.iter().enumerate() {
                        self.validate_message(message, i)?;
                    }
                }
            }

            // Validate parameters
            self.validate_parameters(&json)?;
        }
        Ok(())
    }

    fn validate_message(
        &self,
        message: &serde_json::Value,
        index: usize,
    ) -> Result<(), GatewayError> {
        if let Some(content) = message.get("content") {
            if let Some(content_str) = content.as_str() {
                if content_str.len() > self.config.max_message_length {
                    warn!("Message {} too long: {} chars", index, content_str.len());
                    return Err(GatewayError::InvalidRequest {
                        message: format!(
                            "Message {} too long: {} characters (max: {})",
                            index,
                            content_str.len(),
                            self.config.max_message_length
                        ),
                    });
                }
            }
        }

        // Validate role
        if let Some(role) = message.get("role") {
            if let Some(role_str) = role.as_str() {
                match role_str {
                    "system" | "user" | "assistant" | "function" => {} // Valid roles
                    _ => {
                        warn!("Invalid role in message {}: {}", index, role_str);
                        return Err(GatewayError::InvalidRequest {
                            message: format!("Invalid role in message {index}: {role_str}"),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    fn validate_parameters(&self, json: &serde_json::Value) -> Result<(), GatewayError> {
        // Validate temperature
        if let Some(temp) = json.get("temperature") {
            if let Some(temp_num) = temp.as_f64() {
                if !(0.0..=2.0).contains(&temp_num) {
                    warn!("Invalid temperature: {}", temp_num);
                    return Err(GatewayError::InvalidRequest {
                        message: format!(
                            "Invalid temperature: {temp_num} (must be between 0.0 and 2.0)"
                        ),
                    });
                }
            }
        }

        // Validate max_tokens
        if let Some(max_tokens) = json.get("max_tokens") {
            if let Some(tokens_num) = max_tokens.as_u64() {
                if tokens_num > 32000 {
                    // Reasonable upper limit
                    warn!("Max tokens too high: {}", tokens_num);
                    return Err(GatewayError::InvalidRequest {
                        message: format!("Max tokens too high: {tokens_num} (max: 32000)"),
                    });
                }
            }
        }

        // Validate top_p
        if let Some(top_p) = json.get("top_p") {
            if let Some(top_p_num) = top_p.as_f64() {
                if !(0.0..=1.0).contains(&top_p_num) {
                    warn!("Invalid top_p: {}", top_p_num);
                    return Err(GatewayError::InvalidRequest {
                        message: format!(
                            "Invalid top_p: {top_p_num} (must be between 0.0 and 1.0)"
                        ),
                    });
                }
            }
        }

        Ok(())
    }
}

pub async fn input_validation_middleware(
    State(_state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Get request body
    let (parts, body) = request.into_parts();
    let body_bytes = match axum::body::to_bytes(body, MAX_REQUEST_SIZE).await {
        Ok(bytes) => bytes,
        Err(_) => {
            error!("Failed to read request body for validation");
            return Err(StatusCode::BAD_REQUEST);
        }
    };
    let body = String::from_utf8_lossy(&body_bytes).to_string();

    // Create validator with default config
    let validator = InputValidator::new(ValidationConfig::default());

    // Validate the request
    if let Err(validation_error) = validator.validate_request(&body, &parts.headers) {
        match validation_error {
            GatewayError::InvalidRequest { message } => {
                error!("Input validation failed: {}", message);
                return Err(StatusCode::BAD_REQUEST);
            }
            GatewayError::ContentFiltered { message } => {
                error!("Content filtered: {}", message);
                return Err(StatusCode::FORBIDDEN);
            }
            _ => {
                error!("Validation error: {}", validation_error);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    // Restore the body for downstream handlers
    let request = Request::from_parts(parts, axum::body::Body::from(body));

    // Continue to next middleware/handler
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_validator_size_limit() {
        let config = ValidationConfig {
            max_request_size: 100,
            ..Default::default()
        };
        let validator = InputValidator::new(config);
        let headers = HeaderMap::new();

        let large_body = "x".repeat(200);
        let result = validator.validate_request(&large_body, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_input_validator_dangerous_patterns() {
        let validator = InputValidator::new(ValidationConfig::default());
        let headers = HeaderMap::new();

        let dangerous_body = r#"{"message": "<script>alert('xss')</script>"}"#;
        let result = validator.validate_request(dangerous_body, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_input_validator_valid_chat_request() {
        let validator = InputValidator::new(ValidationConfig::default());
        let headers = HeaderMap::new();

        let valid_body = r#"{
            "model": "gpt-3.5-turbo",
            "messages": [
                {"role": "user", "content": "Hello, world!"}
            ],
            "temperature": 0.7,
            "max_tokens": 100
        }"#;
        let result = validator.validate_request(valid_body, &headers);
        assert!(result.is_ok());
    }

    #[test]
    fn test_input_validator_invalid_parameters() {
        let validator = InputValidator::new(ValidationConfig::default());
        let headers = HeaderMap::new();

        let invalid_body = r#"{
            "model": "gpt-3.5-turbo",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "temperature": 5.0
        }"#;
        let result = validator.validate_request(invalid_body, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_json_depth_calculation() {
        let validator = InputValidator::new(ValidationConfig::default());

        let shallow_json = serde_json::json!({"key": "value"});
        assert_eq!(validator.calculate_json_depth(&shallow_json), 1);

        let deep_json = serde_json::json!({
            "level1": {
                "level2": {
                    "level3": {
                        "level4": "value"
                    }
                }
            }
        });
        assert_eq!(validator.calculate_json_depth(&deep_json), 4);
    }

    #[test]
    fn test_message_validation() {
        let validator = InputValidator::new(ValidationConfig {
            max_message_length: 50,
            ..Default::default()
        });

        let long_message = serde_json::json!({
            "role": "user",
            "content": "x".repeat(100)
        });
        let result = validator.validate_message(&long_message, 0);
        assert!(result.is_err());

        let invalid_role_message = serde_json::json!({
            "role": "invalid_role",
            "content": "Hello"
        });
        let result = validator.validate_message(&invalid_role_message, 0);
        assert!(result.is_err());
    }
}
