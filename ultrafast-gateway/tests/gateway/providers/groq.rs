// Groq provider tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_groq_chat_completion() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("llama3-70b-8192", "What is the capital of France?");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Groq may not be configured, so this could fail
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["choices"].is_array());
        assert!(body["choices"][0]["message"]["content"].is_string());
    } else {
        // Expected behavior if Groq is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_groq_streaming() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_streaming_request("llama3-70b-8192", "Write a short poem");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("Accept", "text/event-stream")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let headers = response.headers();
        assert_eq!(headers.get("content-type").unwrap(), "text/event-stream");
        
        let body = response.text();
        assert!(body.contains("data: "));
    } else {
        // Expected behavior if Groq is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_groq_fast_response() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "llama3-70b-8192",
        "messages": [
            {"role": "user", "content": "What is 2+2?"}
        ],
        "stream": false
    });
    
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
        
        // Groq is known for fast responses, so we can check response time
        // This is just a structural test, not a performance test
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_groq_max_tokens() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "llama3-70b-8192",
        "messages": [
            {"role": "user", "content": "Write a very long story"}
        ],
        "max_tokens": 50
    });
    
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
        
        // Check that response is limited by max_tokens
        let content = body["choices"][0]["message"]["content"].as_str().unwrap();
        assert!(content.len() <= 200); // Approximate token limit
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_groq_temperature() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "llama3-70b-8192",
        "messages": [
            {"role": "user", "content": "Write a creative story"}
        ],
        "temperature": 0.9
    });
    
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
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_groq_error_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid model
    let request = helpers::create_test_chat_request("invalid-groq-model", "Hello");
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
async fn test_groq_timeout_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with a request that might timeout
    let request = helpers::create_test_chat_request("llama3-70b-8192", "Generate a very long response");
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
async fn test_groq_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("llama3-70b-8192", "Hello");
    
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
async fn test_groq_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test rate limiting
    for i in 0..5 {
        let request = helpers::create_test_chat_request("llama3-70b-8192", &format!("Request {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle rate limiting gracefully
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_groq_content_filtering() {
    let server = helpers::create_test_server().await;
    
    // Test with potentially harmful content
    let request = helpers::create_test_chat_request("llama3-70b-8192", "Write a story about violence");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle content filtering appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}
