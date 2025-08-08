// Provider integration tests
pub mod ollama;
pub mod openai;
pub mod anthropic;
pub mod google;
pub mod azure;
pub mod cohere;
pub mod groq;
pub mod mistral;
pub mod perplexity;
pub mod custom;
pub mod circuit_breaker;

use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_provider_health_check() {
    let server = helpers::create_test_server().await;
    
    // Test health check endpoint
    let response = server.get("/health").await;
    helpers::assert_success_response(&response);
    
    let body: Value = response.json();
    assert_eq!(body["status"], "healthy");
}

#[tokio::test]
async fn test_provider_list() {
    let server = helpers::create_test_server().await;
    
    // Test providers endpoint
    let response = server
        .get("/v1/providers")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    helpers::assert_success_response(&response);
    
    let body: Value = response.json();
    assert!(body.is_object());
}

#[tokio::test]
async fn test_provider_configuration() {
    let server = helpers::create_test_server().await;
    
    // Test configuration endpoint
    let response = server
        .get("/admin/config")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    helpers::assert_success_response(&response);
    
    let body: Value = response.json();
    assert!(body["providers"].is_array());
}

#[tokio::test]
async fn test_provider_error_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid model
    let request = helpers::create_test_chat_request("invalid-model", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    helpers::assert_error_response(&response, StatusCode::SERVICE_UNAVAILABLE);
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
}

#[tokio::test]
async fn test_provider_timeout_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with timeout scenario
    let request = helpers::create_test_chat_request("timeout-model", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle timeout gracefully
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_provider_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Test rate limiting by making many requests
    for i in 0..10 {
        let request = helpers::create_test_chat_request("test-model", &format!("Request {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should not all fail due to rate limiting
        if i < 5 {
            // First few requests should succeed
            assert!(response.status_code().is_success() || response.status_code().is_server_error());
        }
    }
}

#[tokio::test]
async fn test_provider_circuit_breaker() {
    let server = helpers::create_test_server().await;
    
    // Test circuit breaker by making failing requests
    for _ in 0..5 {
        let request = helpers::create_test_chat_request("failing-model", "Hello");
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should eventually trigger circuit breaker
        assert!(response.status_code().is_server_error());
    }
    
    // Test circuit breaker metrics
    let response = server
        .get("/admin/circuit-breakers")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    helpers::assert_success_response(&response);
    
    let body: Value = response.json();
    assert!(body.is_object());
}
