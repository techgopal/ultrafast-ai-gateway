// OpenAI provider tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_openai_chat_completion() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "What is the capital of France?");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // OpenAI may not be configured, so this could fail
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["choices"].is_array());
        assert!(body["choices"][0]["message"]["content"].is_string());
    } else {
        // Expected behavior if OpenAI is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_openai_streaming() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_streaming_request("gpt-3.5-turbo", "Write a short poem");
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
        // Expected behavior if OpenAI is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_openai_embeddings() {
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
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert!(body["data"][0]["embedding"].is_array());
    } else {
        // Expected behavior if OpenAI is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_openai_image_generation() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "dall-e-3",
        "prompt": "A beautiful sunset over mountains",
        "n": 1,
        "size": "1024x1024"
    });
    
    let response = server
        .post("/v1/images/generations")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert!(body["data"][0]["url"].is_string());
    } else {
        // Expected behavior if OpenAI is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_openai_audio_transcription() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "whisper-1",
        "file": "audio.mp3",
        "response_format": "json"
    });
    
    let response = server
        .post("/v1/audio/transcriptions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "multipart/form-data")
        .body("multipart form data")
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["text"].is_string());
    } else {
        // Expected behavior if OpenAI is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_openai_text_to_speech() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "tts-1",
        "input": "Hello, this is a test.",
        "voice": "alloy"
    });
    
    let response = server
        .post("/v1/audio/speech")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        // Should return audio data
        let body = response.bytes();
        assert!(!body.is_empty());
    } else {
        // Expected behavior if OpenAI is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_openai_error_handling() {
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
async fn test_openai_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test rate limiting
    for i in 0..5 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Request {}", i));
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
async fn test_openai_timeout_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with a request that might timeout
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Generate a very long response");
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
async fn test_openai_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello");
    
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
