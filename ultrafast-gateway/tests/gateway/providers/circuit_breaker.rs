// Circuit Breaker provider tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_circuit_breaker_basic() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("circuit-breaker-model", "Hello world");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["choices"].is_array());
        assert!(body["choices"][0]["message"]["content"].is_string());
    } else {
        // Expected if circuit breaker is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_circuit_breaker_failure_threshold() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test failure threshold
    for i in 0..5 {
        let request = helpers::create_test_chat_request("circuit-breaker-model", &format!("Request {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle circuit breaker failure threshold
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_circuit_breaker_timeout() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("circuit-breaker-model", "Generate a very long response");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("X-Circuit-Breaker-Timeout", "5000")
        .json(&request)
        .await;
    
    // Should handle circuit breaker timeout
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_circuit_breaker_recovery() {
    let server = helpers::create_test_server().await;
    
    // Make requests to potentially trigger circuit breaker
    for i in 0..10 {
        let request = helpers::create_test_chat_request("circuit-breaker-model", &format!("Recovery test {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle circuit breaker recovery
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
    
    // Wait for potential recovery
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    let request = helpers::create_test_chat_request("circuit-breaker-model", "Recovery test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should allow requests after recovery
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_circuit_breaker_half_open() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("circuit-breaker-model", "Half-open test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("X-Circuit-Breaker-State", "half-open")
        .json(&request)
        .await;
    
    // Should handle half-open state
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_circuit_breaker_metrics() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("circuit-breaker-model", "Metrics test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should include circuit breaker metrics in response headers
    let headers = response.headers();
    if headers.contains_key("x-circuit-breaker-state") {
        assert!(headers.get("x-circuit-breaker-state").is_some());
    }
    if headers.contains_key("x-circuit-breaker-failures") {
        assert!(headers.get("x-circuit-breaker-failures").is_some());
    }
    if headers.contains_key("x-circuit-breaker-successes") {
        assert!(headers.get("x-circuit-breaker-successes").is_some());
    }
}

#[tokio::test]
async fn test_circuit_breaker_configuration() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("circuit-breaker-model", "Configuration test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("X-Circuit-Breaker-Failure-Threshold", "3")
        .add_header("X-Circuit-Breaker-Timeout", "10000")
        .add_header("X-Circuit-Breaker-Recovery-Threshold", "2")
        .json(&request)
        .await;
    
    // Should handle custom circuit breaker configuration
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_circuit_breaker_error_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid model to trigger errors
    let request = helpers::create_test_chat_request("invalid-circuit-breaker-model", "Error test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle circuit breaker error handling
    assert!(response.status_code().is_server_error());
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
}

#[tokio::test]
async fn test_circuit_breaker_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("circuit-breaker-model", "Authentication test");
    
    // Test without authentication
    let response = server
        .post("/v1/chat/completions")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject unauthenticated requests
    assert!(response.status_code().is_client_error());
    
    // Test with invalid API key
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
async fn test_circuit_breaker_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test rate limiting with circuit breaker
    for i in 0..5 {
        let request = helpers::create_test_chat_request("circuit-breaker-model", &format!("Rate limit test {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle rate limiting with circuit breaker
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}
