// Comprehensive test suite for Ultrafast Gateway
pub mod unit;
pub mod integration;
pub mod performance;
pub mod security;
pub mod providers;
pub mod middleware;
pub mod api;
pub mod dashboard;
pub mod websocket;
pub mod utils;

// Test utilities and helpers
pub mod helpers {
    use crate::config::Config;
    use crate::server::AppState;
    use axum::http::StatusCode;
    use axum_test::TestServer;
    use serde_json::Value;
    use std::collections::HashMap;
    use ultrafast_models_sdk::providers::ProviderConfig;

    pub fn create_test_config() -> Config {
        let mut config = Config::default();
        config.providers.insert(
            "test-provider".to_string(),
            ProviderConfig {
                name: "test-provider".to_string(),
                api_key: "test-key".to_string(),
                base_url: Some("http://localhost:11434".to_string()),
                timeout: std::time::Duration::from_secs(30),
                max_retries: 3,
                retry_delay: std::time::Duration::from_secs(1),
                enabled: true,
                model_mapping: HashMap::new(),
                headers: HashMap::new(),
                rate_limit: None,
                circuit_breaker: None,
            },
        );
        config
    }

    pub async fn create_test_server() -> TestServer {
        let config = create_test_config();
        let app = crate::server::create_server(config).await.unwrap();
        TestServer::new(app).unwrap()
    }

    pub fn create_test_chat_request(model: &str, content: &str) -> Value {
        serde_json::json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": content
                }
            ],
            "max_tokens": 100,
            "temperature": 0.7
        })
    }

    pub fn create_test_streaming_request(model: &str, content: &str) -> Value {
        serde_json::json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": content
                }
            ],
            "stream": true,
            "max_tokens": 100,
            "temperature": 0.7
        })
    }

    pub fn assert_success_response(response: &axum_test::TestResponse) {
        assert_eq!(response.status_code(), StatusCode::OK);
    }

    pub fn assert_error_response(response: &axum_test::TestResponse, expected_status: StatusCode) {
        assert_eq!(response.status_code(), expected_status);
    }
}
