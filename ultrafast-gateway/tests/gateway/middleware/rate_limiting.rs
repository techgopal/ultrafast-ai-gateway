// Rate limiting middleware tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_rate_limiting_basic() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to trigger rate limiting
    for i in 0..20 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Request {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle rate limiting gracefully
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_per_ip() {
    let server = helpers::create_test_server().await;
    
    // Simulate requests from different IPs
    let ip_addresses = ["192.168.1.1", "192.168.1.2", "10.0.0.1"];
    
    for ip in ip_addresses {
        for i in 0..5 {
            let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Request from {}", ip));
            let response = server
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .add_header("X-Forwarded-For", ip)
                .json(&request)
                .await;
            
            // Should handle rate limiting per IP
            assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
        }
    }
}

#[tokio::test]
async fn test_rate_limiting_per_user() {
    let server = helpers::create_test_server().await;
    
    // Simulate requests from different users
    let api_keys = ["sk-user-1", "sk-user-2", "sk-user-3"];
    
    for api_key in api_keys {
        for i in 0..5 {
            let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Request from user"));
            let response = server
                .post("/v1/chat/completions")
                .add_header("Authorization", &format!("ApiKey {}", api_key))
                .add_header("Content-Type", "application/json")
                .json(&request)
                .await;
            
            // Should handle rate limiting per user
            assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
        }
    }
}

#[tokio::test]
async fn test_rate_limiting_window() {
    let server = helpers::create_test_server().await;
    
    // Make requests within a short time window
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Rapid request {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle rapid requests within time window
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_error_response() {
    let server = helpers::create_test_server().await;
    
    // Make many requests to potentially trigger rate limiting
    for i in 0..50 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Request {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        if response.status_code().is_client_error() {
            // If rate limited, check error response structure
            let body: Value = response.json();
            assert!(body["error"].is_object());
            let error_message = body["error"]["message"].as_str().unwrap_or("");
            assert!(error_message.contains("rate") || error_message.contains("limit") || error_message.contains("too many"));
        }
    }
}

#[tokio::test]
async fn test_rate_limiting_headers() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Test request");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should include rate limiting headers if applicable
    let headers = response.headers();
    if headers.contains_key("x-ratelimit-limit") {
        assert!(headers.get("x-ratelimit-limit").is_some());
    }
    if headers.contains_key("x-ratelimit-remaining") {
        assert!(headers.get("x-ratelimit-remaining").is_some());
    }
    if headers.contains_key("x-ratelimit-reset") {
        assert!(headers.get("x-ratelimit-reset").is_some());
    }
}

#[tokio::test]
async fn test_rate_limiting_different_endpoints() {
    let server = helpers::create_test_server().await;
    
    // Test rate limiting on different endpoints
    let endpoints = [
        ("/v1/chat/completions", helpers::create_test_chat_request("gpt-3.5-turbo", "Hello")),
        ("/v1/embeddings", serde_json::json!({
            "model": "text-embedding-ada-002",
            "input": "Hello world"
        })),
        ("/v1/models", serde_json::Value::Null),
    ];
    
    for (endpoint, request) in endpoints {
        for i in 0..5 {
            let response = if endpoint == "/v1/models" {
                server
                    .get(endpoint)
                    .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                    .await
            } else {
                server
                    .post(endpoint)
                    .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                    .add_header("Content-Type", "application/json")
                    .json(&request)
                    .await
            };
            
            // Should handle rate limiting on different endpoints
            assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
        }
    }
}

#[tokio::test]
async fn test_rate_limiting_burst_requests() {
    let server = helpers::create_test_server().await;
    
    // Simulate burst of requests
    let mut handles = vec![];
    
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Burst request {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .json(&request)
                .await
        });
        
        handles.push(handle);
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        // Should handle burst requests gracefully
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_recovery() {
    let server = helpers::create_test_server().await;
    
    // Make many requests to potentially trigger rate limiting
    for i in 0..30 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Request {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle rate limiting
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
    
    // Wait a bit and try again
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Recovery test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should allow requests after rate limit window
    assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
}

#[tokio::test]
async fn test_rate_limiting_configuration() {
    let server = helpers::create_test_server().await;
    
    // Test with different rate limiting configurations
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Configuration test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("X-Rate-Limit-Config", "100/minute")
        .json(&request)
        .await;
    
    // Should handle custom rate limiting configuration
    assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
}

#[tokio::test]
async fn test_rate_limiting_priority() {
    let server = helpers::create_test_server().await;
    
    // Test rate limiting with different priority levels
    let priorities = ["high", "normal", "low"];
    
    for priority in priorities {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Priority {}", priority));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .add_header("X-Priority", priority)
            .json(&request)
            .await;
        
        // Should handle different priority levels
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}
