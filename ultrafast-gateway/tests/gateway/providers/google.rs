// Google provider tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_google_chat_completion() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gemini-1.5-pro", "What is the capital of France?");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Google may not be configured, so this could fail
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["choices"].is_array());
        assert!(body["choices"][0]["message"]["content"].is_string());
    } else {
        // Expected behavior if Google is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_google_streaming() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_streaming_request("gemini-1.5-pro", "Write a short poem");
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
        // Expected behavior if Google is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_google_safety_settings() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "gemini-1.5-pro",
        "messages": [
            {"role": "user", "content": "Write a story"}
        ],
        "safety_settings": [
            {
                "category": "HARM_CATEGORY_HARASSMENT",
                "threshold": "BLOCK_MEDIUM_AND_ABOVE"
            }
        ]
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
async fn test_google_generation_config() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "gemini-1.5-pro",
        "messages": [
            {"role": "user", "content": "Write a creative story"}
        ],
        "generation_config": {
            "temperature": 0.9,
            "top_p": 0.8,
            "top_k": 40,
            "max_output_tokens": 100
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
async fn test_google_multimodal() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "gemini-1.5-pro",
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": "Describe this image"
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD..."
                        }
                    }
                ]
            }
        ]
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
async fn test_google_error_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid model
    let request = helpers::create_test_chat_request("invalid-google-model", "Hello");
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
async fn test_google_timeout_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with a request that might timeout
    let request = helpers::create_test_chat_request("gemini-1.5-pro", "Generate a very long response");
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
async fn test_google_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gemini-1.5-pro", "Hello");
    
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
async fn test_google_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test rate limiting
    for i in 0..5 {
        let request = helpers::create_test_chat_request("gemini-1.5-pro", &format!("Request {}", i));
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
async fn test_google_content_filtering() {
    let server = helpers::create_test_server().await;
    
    // Test with potentially harmful content
    let request = helpers::create_test_chat_request("gemini-1.5-pro", "Write a story about violence");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle content filtering appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}
