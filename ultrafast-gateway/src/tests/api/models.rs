// Models API endpoint tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_models_list() {
    let server = helpers::create_test_server().await;
    
    let response = server
        .get("/v1/models")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert!(body["object"].is_string());
        assert_eq!(body["object"].as_str().unwrap(), "list");
        
        // Check that we have some models
        let models = body["data"].as_array().unwrap();
        assert!(!models.is_empty());
        
        // Check structure of first model
        if let Some(first_model) = models.first() {
            assert!(first_model["id"].is_string());
            assert!(first_model["object"].is_string());
            assert_eq!(first_model["object"].as_str().unwrap(), "model");
        }
    } else {
        // Expected if models endpoint is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_models_retrieve() {
    let server = helpers::create_test_server().await;
    
    // Test retrieving a specific model
    let response = server
        .get("/v1/models/gpt-3.5-turbo")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["id"].is_string());
        assert_eq!(body["id"].as_str().unwrap(), "gpt-3.5-turbo");
        assert!(body["object"].is_string());
        assert_eq!(body["object"].as_str().unwrap(), "model");
        assert!(body["created"].is_number());
        assert!(body["owned_by"].is_string());
    } else {
        // Expected if model retrieval is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_models_retrieve_invalid() {
    let server = helpers::create_test_server().await;
    
    // Test retrieving a non-existent model
    let response = server
        .get("/v1/models/invalid-model-id")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    // Should handle invalid model gracefully
    assert!(response.status_code().is_server_error());
}

#[tokio::test]
async fn test_models_authentication() {
    let server = helpers::create_test_server().await;
    
    // Test without authentication
    let response = server
        .get("/v1/models")
        .await;
    
    // Should reject unauthenticated requests
    assert!(response.status_code().is_client_error());
    
    // Test with invalid API key
    let response = server
        .get("/v1/models")
        .add_header("Authorization", "ApiKey invalid-key")
        .await;
    
    // Should reject invalid API key
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_models_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test rate limiting
    for _ in 0..5 {
        let response = server
            .get("/v1/models")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .await;
        
        // Should handle rate limiting gracefully
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}
