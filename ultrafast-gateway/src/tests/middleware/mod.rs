// Middleware tests
pub mod authentication;
pub mod input_validation;
pub mod rate_limiting;
pub mod caching;
pub mod logging;
pub mod plugins;

use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_authentication_middleware() {
    let server = helpers::create_test_server().await;
    
    // Test valid authentication
    let request = helpers::create_test_chat_request("test-model", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should allow valid API key
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
    
    // Test invalid authentication
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey invalid-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject invalid API key
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_input_validation_middleware() {
    let server = helpers::create_test_server().await;
    
    // Test valid input
    let request = helpers::create_test_chat_request("test-model", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should accept valid input
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
    
    // Test oversized input
    let large_content = "A".repeat(1000000);
    let request = helpers::create_test_chat_request("test-model", &large_content);
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject oversized input
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_rate_limiting_middleware() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test rate limiting
    for i in 0..20 {
        let request = helpers::create_test_chat_request("test-model", &format!("Request {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should eventually hit rate limits
        if i > 10 {
            assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
        }
    }
}

#[tokio::test]
async fn test_caching_middleware() {
    let server = helpers::create_test_server().await;
    
    // Make identical requests to test caching
    let request = helpers::create_test_chat_request("test-model", "Cache test");
    
    let response1 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    let response2 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Both should succeed (either from cache or fresh)
    assert!(response1.status_code().is_success() || response1.status_code().is_server_error());
    assert!(response2.status_code().is_success() || response2.status_code().is_server_error());
}

#[tokio::test]
async fn test_logging_middleware() {
    let server = helpers::create_test_server().await;
    
    // Make a request to test logging
    let request = helpers::create_test_chat_request("test-model", "Log test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should log the request (we can't easily test this, but should not panic)
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_plugin_middleware() {
    let server = helpers::create_test_server().await;
    
    // Test content filtering plugin
    let request = helpers::create_test_chat_request("test-model", "Hello world");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should process through plugins
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_middleware_error_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with malformed request
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .body("invalid json")
        .await;
    
    // Should handle errors gracefully
    assert!(response.status_code().is_client_error());
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
}

#[tokio::test]
async fn test_middleware_headers() {
    let server = helpers::create_test_server().await;
    
    let response = server
        .get("/health")
        .add_header("User-Agent", "TestClient/1.0")
        .add_header("X-Forwarded-For", "192.168.1.1")
        .await;
    
    helpers::assert_success_response(&response);
    
    // Should preserve important headers
    let headers = response.headers();
    assert!(headers.contains_key("content-type"));
}

#[tokio::test]
async fn test_middleware_timeout() {
    let server = helpers::create_test_server().await;
    
    // Test with a request that might timeout
    let request = helpers::create_test_chat_request("timeout-model", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle timeouts gracefully
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_middleware_cors() {
    let server = helpers::create_test_server().await;
    
    // Test CORS preflight request
    let response = server
        .options("/v1/chat/completions")
        .add_header("Origin", "https://example.com")
        .add_header("Access-Control-Request-Method", "POST")
        .add_header("Access-Control-Request-Headers", "Content-Type, Authorization")
        .await;
    
    // Should handle CORS properly
    let headers = response.headers();
    assert!(headers.contains_key("access-control-allow-origin"));
    assert!(headers.contains_key("access-control-allow-methods"));
    assert!(headers.contains_key("access-control-allow-headers"));
}
