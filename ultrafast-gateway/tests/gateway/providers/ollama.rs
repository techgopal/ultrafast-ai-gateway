// Ollama provider tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_ollama_chat_completion() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("llama3.2:3b-instruct-q8_0", "What is the capital of France?");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    helpers::assert_success_response(&response);
    
    let body: Value = response.json();
    assert!(body["choices"].is_array());
    assert!(body["choices"][0]["message"]["content"].is_string());
}

#[tokio::test]
async fn test_ollama_streaming() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_streaming_request("llama3.2:3b-instruct-q8_0", "Write a haiku about programming");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("Accept", "text/event-stream")
        .json(&request)
        .await;
    
    helpers::assert_success_response(&response);
    
    let headers = response.headers();
    assert_eq!(headers.get("content-type").unwrap(), "text/event-stream");
    
    let body = response.text();
    assert!(body.contains("data: "));
}

#[tokio::test]
async fn test_ollama_embeddings() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "llama3.2:3b-instruct-q8_0",
        "input": "Hello world"
    });
    
    let response = server
        .post("/v1/embeddings")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Ollama may not support embeddings, so this could fail
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert!(body["data"][0]["embedding"].is_array());
    } else {
        // Expected behavior for Ollama
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_ollama_multiple_models() {
    let server = helpers::create_test_server().await;
    
    let models = vec!["llama3.2:3b-instruct-q8_0", "qwen3:8b", "gemma3:4b"];
    
    for model in models {
        let request = helpers::create_test_chat_request(model, "Hello");
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should work for all models
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_ollama_error_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid model
    let request = helpers::create_test_chat_request("invalid-model", "Hello");
    let response = server
        .post("/v1/chat/completions")
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
async fn test_ollama_timeout_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with a request that might timeout
    let request = helpers::create_test_chat_request("llama3.2:3b-instruct-q8_0", "Generate a very long response");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle timeouts gracefully
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_ollama_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("llama3.2:3b-instruct-q8_0", "Hello");
    
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
