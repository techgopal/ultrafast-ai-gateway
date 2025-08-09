// Embeddings API tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_embeddings_basic() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "text-embedding-ada-002",
        "input": "Hello world"
    });
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Embeddings may not be supported by all providers
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert!(body["data"][0]["embedding"].is_array());
        assert!(body["model"].is_string());
        assert!(body["usage"].is_object());
    } else {
        // Expected behavior if embeddings are not supported
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_embeddings_multiple_inputs() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "text-embedding-ada-002",
        "input": [
            "Hello world",
            "This is a test",
            "Another sentence"
        ]
    });
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert_eq!(body["data"].as_array().unwrap().len(), 3);
        
        for embedding in body["data"].as_array().unwrap() {
            assert!(embedding["embedding"].is_array());
        }
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_embeddings_missing_model() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "input": "Hello world"
    });
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    assert!(response.status_code().is_client_error());
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
}

#[tokio::test]
async fn test_embeddings_missing_input() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "text-embedding-ada-002"
    });
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    assert!(response.status_code().is_client_error());
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
}

#[tokio::test]
async fn test_embeddings_invalid_json() {
    let server = helpers::create_test_server().await;
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .body("invalid json")
        .await;
    
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_embeddings_large_input() {
    let server = helpers::create_test_server().await;
    
    let large_text = "A".repeat(100000); // 100KB text
    let request = serde_json::json!({
        "model": "text-embedding-ada-002",
        "input": large_text
    });
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle large inputs appropriately
    assert!(response.status_code().is_success() || response.status_code().is_client_error());
}

#[tokio::test]
async fn test_embeddings_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "text-embedding-ada-002",
        "input": "Hello world"
    });
    
    // Test without authentication
    let response = server
        .post("/v1/embeddings")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject unauthenticated requests
    assert!(response.status_code().is_client_error());
    
    // Test with invalid API key
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey invalid-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject invalid API key
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_embeddings_with_parameters() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "text-embedding-ada-002",
        "input": "Hello world",
        "encoding_format": "float",
        "dimensions": 1536
    });
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert!(body["data"][0]["embedding"].is_array());
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_embeddings_error_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid model
    let request = serde_json::json!({
        "model": "invalid-embedding-model",
        "input": "Hello world"
    });
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle invalid model gracefully
    assert!(response.status_code().is_server_error());
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
}

#[tokio::test]
async fn test_embeddings_empty_input() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "text-embedding-ada-002",
        "input": ""
    });
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle empty input appropriately
    assert!(response.status_code().is_success() || response.status_code().is_client_error());
}

#[tokio::test]
async fn test_embeddings_mixed_input_types() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "text-embedding-ada-002",
        "input": [
            "Hello world",
            "This is a test",
            ""
        ]
    });
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert_eq!(body["data"].as_array().unwrap().len(), 3);
    } else {
        assert!(response.status_code().is_server_error());
    }
}
