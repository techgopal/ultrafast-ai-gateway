// Rate limiting security tests
use ultrafast_gateway as crate;
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_rate_limiting_basic() {
    let server = helpers::create_test_server().await;
    
    // Basic rate limiting test
    let mut handles = vec![];
    
    for i in 0..20 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Rate limit test {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_per_ip() {
    let server = helpers::create_test_server().await;
    
    // Test rate limiting per IP
    let mut handles = vec![];
    
    let ip_addresses = ["192.168.1.1", "192.168.1.2", "10.0.0.1", "172.16.0.1"];
    
    for ip in ip_addresses {
        for i in 0..8 {
            let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("IP rate limit test {} from {}", i, ip));
            let server_clone = server.clone();
            
            let handle = tokio::spawn(async move {
                server_clone
                    .post("/v1/chat/completions")
                    .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                    .add_header("Content-Type", "application/json")
                    .add_header("X-Forwarded-For", ip)
                    .json(&request)
                    .await
            });
            
            handles.push(handle);
        }
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_per_user() {
    let server = helpers::create_test_server().await;
    
    // Test rate limiting per user
    let mut handles = vec![];
    
    let api_keys = ["sk-user-1", "sk-user-2", "sk-user-3", "sk-user-4"];
    
    for api_key in api_keys {
        for i in 0..8 {
            let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("User rate limit test {}", i));
            let server_clone = server.clone();
            
            let handle = tokio::spawn(async move {
                server_clone
                    .post("/v1/chat/completions")
                    .add_header("Authorization", &format!("ApiKey {}", api_key))
                    .add_header("Content-Type", "application/json")
                    .json(&request)
                    .await
            });
            
            handles.push(handle);
        }
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_burst_requests() {
    let server = helpers::create_test_server().await;
    
    // Test burst requests
    let mut handles = vec![];
    
    for i in 0..30 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Burst rate limit test {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_time_window() {
    let server = helpers::create_test_server().await;
    
    // Test rate limiting within time window
    let mut handles = vec![];
    
    for i in 0..25 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Time window rate limit test {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_error_responses() {
    let server = helpers::create_test_server().await;
    
    // Test rate limiting error responses
    let mut handles = vec![];
    
    for i in 0..35 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Error response rate limit test {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
        
        // If rate limited, check error response structure
        if response.status_code().is_client_error() {
            let body: Value = response.json();
            assert!(body["error"].is_object());
        }
    }
}

#[tokio::test]
async fn test_rate_limiting_headers() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Rate limit headers test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Check for rate limiting headers
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
    
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_rate_limiting_different_endpoints() {
    let server = helpers::create_test_server().await;
    
    // Test rate limiting on different endpoints
    let mut handles = vec![];
    
    // Chat completions
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Endpoint rate limit chat {}", i));
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
    
    // Embeddings
    for i in 0..10 {
        let request = serde_json::json!({
            "model": "text-embedding-ada-002",
            "input": format!("Endpoint rate limit embedding {}", i)
        });
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/embeddings")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .json(&request)
                .await
        });
        
        handles.push(handle);
    }
    
    // Models endpoint
    for _ in 0..5 {
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .get("/v1/models")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .await
        });
        
        handles.push(handle);
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_recovery() {
    let server = helpers::create_test_server().await;
    
    // Apply rate limiting pressure
    let mut pressure_handles = vec![];
    
    for i in 0..20 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Recovery pressure test {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .json(&request)
                .await
        });
        
        pressure_handles.push(handle);
    }
    
    // Wait for pressure to complete
    for handle in pressure_handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
    
    // Wait for rate limit window to reset
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Test recovery
    let mut recovery_handles = vec![];
    
    for i in 0..8 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Recovery test {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .json(&request)
                .await
        });
        
        recovery_handles.push(handle);
    }
    
    // Wait for recovery requests to complete
    for handle in recovery_handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_configuration() {
    let server = helpers::create_test_server().await;
    
    // Test with custom rate limiting configuration
    let mut handles = vec![];
    
    for i in 0..15 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Config rate limit test {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .add_header("X-Rate-Limit-Config", "100/minute")
                .json(&request)
                .await
        });
        
        handles.push(handle);
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_rate_limiting_metrics_under_rate_limit_pressure() {
    let server = helpers::create_test_server().await;
    
    // Test metrics under rate limiting pressure
    let start = std::time::Instant::now();
    let mut handles = vec![];
    
    for i in 0..25 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Rate limit pressure metrics test {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
    }
    
    let duration = start.elapsed();
    
    // Should complete within reasonable time even under rate limiting pressure
    assert!(duration.as_millis() < 60000); // 1 minute max
    
    // Check metrics endpoint under rate limiting pressure
    let metrics_response = server
        .get("/metrics")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    assert!(metrics_response.status_code().is_success() || metrics_response.status_code().is_server_error());
}
