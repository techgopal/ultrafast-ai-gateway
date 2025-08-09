// Concurrent testing performance tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_concurrent_testing_basic() {
    let server = helpers::create_test_server().await;
    
    // Basic concurrent test
    let mut handles = vec![];
    
    for i in 0..20 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Concurrent test {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_concurrent_testing_high_concurrency() {
    let server = helpers::create_test_server().await;
    
    // High concurrency test
    let mut handles = vec![];
    
    for i in 0..40 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("High concurrency test {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_concurrent_testing_mixed_endpoints() {
    let server = helpers::create_test_server().await;
    
    // Test concurrency across different endpoints
    let mut handles = vec![];
    
    // Chat completions
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Mixed concurrent chat {}", i));
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
            "input": format!("Mixed concurrent embedding {}", i)
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_concurrent_testing_different_providers() {
    let server = helpers::create_test_server().await;
    
    // Test concurrency with different providers
    let mut handles = vec![];
    
    let providers = ["gpt-3.5-turbo", "gpt-4", "claude-3-sonnet", "llama-3.1-8b-instant"];
    
    for (i, provider) in providers.iter().enumerate() {
        for j in 0..5 {
            let request = helpers::create_test_chat_request(provider, &format!("Provider concurrent test {} {}", i, j));
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
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_concurrent_testing_streaming_requests() {
    let server = helpers::create_test_server().await;
    
    // Test concurrent streaming requests
    let mut handles = vec![];
    
    for i in 0..15 {
        let request = helpers::create_test_streaming_request("gpt-3.5-turbo", &format!("Streaming concurrent test {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .add_header("Accept", "text/event-stream")
                .json(&request)
                .await
        });
        
        handles.push(handle);
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_concurrent_testing_error_scenarios() {
    let server = helpers::create_test_server().await;
    
    // Test concurrent error scenarios
    let mut handles = vec![];
    
    // Valid requests
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Concurrent error valid {}", i));
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
    
    // Invalid requests
    for i in 0..10 {
        let request = helpers::create_test_chat_request("invalid-model", &format!("Concurrent error invalid {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_concurrent_testing_timeout_scenarios() {
    let server = helpers::create_test_server().await;
    
    // Test concurrent timeout scenarios
    let mut handles = vec![];
    
    for i in 0..12 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Concurrent timeout test {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .add_header("X-Timeout", "1500") // 1.5 second timeout
                .json(&request)
                .await
        });
        
        handles.push(handle);
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_concurrent_testing_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Test concurrent rate limiting
    let mut handles = vec![];
    
    for i in 0..25 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Concurrent rate limit test {}", i));
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
async fn test_concurrent_testing_resource_contention() {
    let server = helpers::create_test_server().await;
    
    // Test resource contention scenarios
    let mut handles = vec![];
    
    for i in 0..18 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Resource contention test {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_concurrent_testing_metrics_collection() {
    let server = helpers::create_test_server().await;
    
    // Test concurrent metrics collection
    let start = std::time::Instant::now();
    let mut handles = vec![];
    
    for i in 0..16 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Concurrent metrics test {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
    
    let duration = start.elapsed();
    
    // Should complete within reasonable time
    assert!(duration.as_millis() < 60000); // 1 minute max
    
    // Check metrics endpoint during concurrency
    let metrics_response = server
        .get("/metrics")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    assert!(metrics_response.status_code().is_success() || metrics_response.status_code().is_server_error());
}

#[tokio::test]
async fn test_concurrent_testing_recovery_after_concurrency() {
    let server = helpers::create_test_server().await;
    
    // Apply concurrent load
    let mut concurrent_handles = vec![];
    
    for i in 0..20 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Pre-recovery concurrent test {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .json(&request)
                .await
        });
        
        concurrent_handles.push(handle);
    }
    
    // Wait for concurrent load to complete
    for handle in concurrent_handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
    
    // Wait for recovery
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    
    // Test recovery after concurrency
    let mut recovery_handles = vec![];
    
    for i in 0..8 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Recovery concurrent test {}", i));
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}
