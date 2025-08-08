use crate::gateway_error::GatewayError;
use crate::plugins::PluginConfig;
use axum::{body::Body, http::Request, response::Response};
use std::collections::HashSet;
use ultrafast_models_sdk::models::{ChatRequest, EmbeddingRequest, ImageRequest};

#[derive(Clone, Debug)]
pub struct ContentFilteringPlugin {
    name: String,
    enabled: bool,
    blocked_words: HashSet<String>,
    blocked_patterns: Vec<String>,
    max_input_length: usize,
}

impl ContentFilteringPlugin {
    pub fn new(config: &PluginConfig) -> Result<Self, GatewayError> {
        let mut blocked_words = HashSet::new();

        // Add default blocked words
        blocked_words.insert("hate".to_string());
        blocked_words.insert("violence".to_string());
        blocked_words.insert("discrimination".to_string());

        let blocked_patterns = vec![r"\b(hate|violence|discrimination)\b".to_string()];

        let max_input_length = config
            .config
            .get("max_input_length")
            .and_then(|v| v.as_u64())
            .unwrap_or(10000) as usize;

        Ok(Self {
            name: config.name.clone(),
            enabled: config.enabled,
            blocked_words,
            blocked_patterns,
            max_input_length,
        })
    }

    fn check_content(&self, content: &str) -> Result<(), GatewayError> {
        // Check input length
        if content.len() > self.max_input_length {
            return Err(GatewayError::InvalidRequest {
                message: format!(
                    "Input too long: {} characters (max: {})",
                    content.len(),
                    self.max_input_length
                ),
            });
        }

        // Check for blocked words
        let content_lower = content.to_lowercase();
        for word in &self.blocked_words {
            if content_lower.contains(word) {
                return Err(GatewayError::ContentFiltered {
                    message: format!("Content contains blocked word: {word}"),
                });
            }
        }

        // Check for blocked patterns (simplified regex check)
        for pattern in &self.blocked_patterns {
            if content_lower.contains(&pattern.replace(r"\b", "")) {
                return Err(GatewayError::ContentFiltered {
                    message: format!("Content matches blocked pattern: {pattern}"),
                });
            }
        }

        Ok(())
    }

    fn extract_content_from_request(&self, request: &Request<Body>) -> Option<String> {
        // Extract content from request based on path and method
        let path = request.uri().path();
        let _method = request.method().as_str();

        // For chat completions, we would extract the messages content
        // For embeddings, we would extract the input text
        // For image generation, we would extract the prompt

        // Since we can't easily access the body here in the middleware,
        // we'll check the Content-Type header and URL path to determine
        // if this is a request type we should filter
        if path.contains("/chat/completions")
            || path.contains("/completions")
            || path.contains("/embeddings")
            || path.contains("/images/generations")
        {
            // In a real implementation, you would:
            // 1. Read the request body (need to be careful about consuming it)
            // 2. Parse the JSON
            // 3. Extract the relevant text fields
            // 4. Return the concatenated content

            // For now, we'll extract content from headers and query parameters
            let mut content_parts = Vec::new();

            // Extract from query parameters
            if let Some(query) = request.uri().query() {
                // Parse query parameters manually
                for param in query.split('&') {
                    if let Some((key, value)) = param.split_once('=') {
                        if key == "prompt" || key == "input" || key == "text" {
                            content_parts.push(value.to_string());
                        }
                    }
                }
            }

            // Extract from headers that might contain content
            if let Some(content_type) = request.headers().get("content-type") {
                if let Ok(content_type_str) = content_type.to_str() {
                    if content_type_str.contains("application/json") {
                        // This would be a JSON request, content would be in body
                        content_parts.push("JSON request body".to_string());
                    }
                }
            }

            // Extract from user agent (might contain suspicious content)
            if let Some(user_agent) = request.headers().get("user-agent") {
                if let Ok(ua_str) = user_agent.to_str() {
                    if ua_str.len() > 100 {
                        content_parts.push(format!("Long user agent: {}", &ua_str[..50]));
                    }
                }
            }

            if content_parts.is_empty() {
                None
            } else {
                Some(content_parts.join(" | "))
            }
        } else {
            None
        }
    }
}

impl ContentFilteringPlugin {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub async fn before_request(&self, request: &mut Request<Body>) -> Result<(), GatewayError> {
        if let Some(content) = self.extract_content_from_request(request) {
            self.check_content(&content)?;
        }
        Ok(())
    }

    pub async fn after_response(&self, _response: &mut Response<Body>) -> Result<(), GatewayError> {
        // Could check response content here
        Ok(())
    }

    pub async fn on_error(&self, _error: &GatewayError) -> Result<(), GatewayError> {
        // Could log filtering events here
        Ok(())
    }

    // Handler-level content filtering methods
    pub fn filter_chat_request(&self, request: &ChatRequest) -> Result<(), GatewayError> {
        // Check system message
        if let Some(system_msg) = request
            .messages
            .iter()
            .find(|m| m.role == ultrafast_models_sdk::models::Role::System)
        {
            self.check_content(&system_msg.content)?;
        }

        // Check user messages
        for message in &request.messages {
            if message.role == ultrafast_models_sdk::models::Role::User {
                self.check_content(&message.content)?;
            }
        }

        Ok(())
    }

    pub fn filter_embedding_request(&self, request: &EmbeddingRequest) -> Result<(), GatewayError> {
        match &request.input {
            ultrafast_models_sdk::models::EmbeddingInput::String(text) => {
                self.check_content(text)?;
            }
            ultrafast_models_sdk::models::EmbeddingInput::StringArray(texts) => {
                for text in texts {
                    self.check_content(text)?;
                }
            }
            ultrafast_models_sdk::models::EmbeddingInput::TokenArray(_) => {
                // Token arrays don't contain readable text to filter
            }
            ultrafast_models_sdk::models::EmbeddingInput::TokenArrayArray(_) => {
                // Token arrays don't contain readable text to filter
            }
        }
        Ok(())
    }

    pub fn filter_image_request(&self, request: &ImageRequest) -> Result<(), GatewayError> {
        self.check_content(&request.prompt)?;

        // Note: ImageRequest doesn't have negative_prompt field in the current model
        // If needed, this can be added to the ImageRequest struct in models.rs

        Ok(())
    }
}
