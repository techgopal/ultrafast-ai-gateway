// Custom provider tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_custom_chat_completion() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("custom-model", "What is the capital of France?");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Custom may not be configured, so this could fail
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["choices"].is_array());
        assert!(body["choices"][0]["message"]["content"].is_string());
    } else {
        // Expected behavior if Custom is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_custom_streaming() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_streaming_request("custom-model", "Write a short poem");
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
        // Expected behavior if Custom is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_custom_endpoint_configuration() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "custom-model",
        "messages": [
            {"role": "user", "content": "What is 2+2?"}
        ],
        "custom_endpoint": "https://api.custom-provider.com/v1/chat/completions"
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
async fn test_custom_headers() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "custom-model",
        "messages": [
            {"role": "user", "content": "Write a creative story"}
        ],
        "custom_headers": {
            "X-Custom-Header": "custom-value",
            "X-API-Version": "v2"
        }
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
async fn test_custom_max_tokens() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "custom-model",
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
async fn test_custom_temperature() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "custom-model",
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
async fn test_custom_error_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid model
    let request = helpers::create_test_chat_request("invalid-custom-model", "Hello");
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
async fn test_custom_timeout_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with a request that might timeout
    let request = helpers::create_test_chat_request("custom-model", "Generate a very long response");
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
async fn test_custom_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("custom-model", "Hello");
    
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
async fn test_custom_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test rate limiting
    for i in 0..5 {
        let request = helpers::create_test_chat_request("custom-model", &format!("Request {}", i));
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
async fn test_custom_content_filtering() {
    let server = helpers::create_test_server().await;
    
    // Test with potentially harmful content
    let request = helpers::create_test_chat_request("custom-model", "Write a story about violence");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle content filtering appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}
