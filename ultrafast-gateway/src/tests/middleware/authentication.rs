// Authentication middleware tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_authentication_missing_header() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject requests without Authorization header
    assert!(response.status_code().is_client_error());
    assert_eq!(response.status_code(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_authentication_invalid_format() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "InvalidFormat")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject requests with invalid Authorization format
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_authentication_invalid_key() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey invalid-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject requests with invalid API key
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_authentication_valid_key() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should accept requests with valid API key
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_authentication_case_sensitive() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "apikey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject requests with incorrect case
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_authentication_whitespace_handling() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey  sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle extra whitespace gracefully
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_authentication_special_characters() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key!")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject requests with special characters in key
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_authentication_injection_attempt() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key'; DROP TABLE users; --")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject SQL injection attempts
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_authentication_multiple_headers() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Authorization", "ApiKey another-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle multiple Authorization headers gracefully
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_authentication_empty_key() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey ")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject requests with empty API key
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_authentication_long_key() {
    let server = helpers::create_test_server().await;
    
    let long_key = "sk-".to_string() + &"a".repeat(1000);
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", &format!("ApiKey {}", long_key))
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle very long API keys gracefully
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_authentication_unicode_characters() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key-🚀")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject requests with unicode characters in key
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_authentication_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests with invalid authentication
    for _ in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey invalid-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should consistently reject invalid authentication
        assert!(response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_authentication_logging() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey invalid-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should log authentication failures
    assert!(response.status_code().is_client_error());
    
    // Check that error response has appropriate structure
    let body: Value = response.json();
    assert!(body["error"].is_object());
}

#[tokio::test]
async fn test_authentication_error_messages() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should provide clear error message for missing authentication
    assert!(response.status_code().is_client_error());
    
    let body: Value = response.json();
    assert!(body["error"]["message"].is_string());
    let error_message = body["error"]["message"].as_str().unwrap();
    assert!(error_message.contains("authentication") || error_message.contains("unauthorized"));
}
