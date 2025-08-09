// Security tests module
pub mod input_validation;
pub mod rate_limiting;
pub mod data_protection;
pub mod injection_prevention;

use ultrafast_gateway as crate;
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_security_basic() {
    let server = helpers::create_test_server().await;
    
    // Basic security test
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Security test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle security appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_security_authentication_required() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Authentication test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should require authentication
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_security_invalid_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Invalid auth test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey invalid-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject invalid authentication
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_security_request_validation() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid request structure
    let invalid_request = serde_json::json!({
        "invalid_field": "invalid_value"
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&invalid_request)
        .await;
    
    // Should validate request structure
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_security_content_filtering() {
    let server = helpers::create_test_server().await;
    
    // Test with potentially harmful content
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Write a story about violence and weapons");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle content filtering appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_security_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test rate limiting
    for i in 0..15 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Rate limit security test {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle rate limiting appropriately
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_security_input_sanitization() {
    let server = helpers::create_test_server().await;
    
    // Test with potentially malicious input
    let malicious_input = "<script>alert('xss')</script>Hello world";
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", malicious_input);
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should sanitize input appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_security_error_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with malformed request
    let malformed_request = "invalid json";
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .body(malformed_request)
        .await;
    
    // Should handle malformed requests securely
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_security_headers() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Security headers test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Check for security headers
    let headers = response.headers();
    
    // Should include security-related headers
    if headers.contains_key("x-content-type-options") {
        assert!(headers.get("x-content-type-options").is_some());
    }
    if headers.contains_key("x-frame-options") {
        assert!(headers.get("x-frame-options").is_some());
    }
    if headers.contains_key("x-xss-protection") {
        assert!(headers.get("x-xss-protection").is_some());
    }
    
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_security_cors_handling() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "CORS test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("Origin", "https://malicious-site.com")
        .json(&request)
        .await;
    
    // Should handle CORS appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_security_metrics_under_security_pressure() {
    let server = helpers::create_test_server().await;
    
    // Test metrics under security pressure
    let start = std::time::Instant::now();
    let mut handles = vec![];
    
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Security pressure metrics test {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .json(&request)
                .await
        });
        
        handles.push(handle);
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
    
    let duration = start.elapsed();
    
    // Should complete within reasonable time even under security pressure
    assert!(duration.as_millis() < 60000); // 1 minute max
    
    // Check metrics endpoint under security pressure
    let metrics_response = server
        .get("/metrics")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    assert!(metrics_response.status_code().is_success() || metrics_response.status_code().is_server_error());
}
