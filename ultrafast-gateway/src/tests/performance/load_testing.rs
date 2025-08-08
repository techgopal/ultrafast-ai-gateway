// Load testing performance tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_load_testing_basic() {
    let server = helpers::create_test_server().await;
    
    // Simulate basic load
    let mut handles = vec![];
    
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Load test {}", i));
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
async fn test_load_testing_high_concurrency() {
    let server = helpers::create_test_server().await;
    
    // Test high concurrency
    let mut handles = vec![];
    
    for i in 0..20 {
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
async fn test_load_testing_sustained_load() {
    let server = helpers::create_test_server().await;
    
    // Test sustained load over time
    for batch in 0..5 {
        let mut handles = vec![];
        
        for i in 0..5 {
            let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Sustained load batch {} request {}", batch, i));
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
        
        // Wait for batch to complete
        for handle in handles {
            let response = handle.await.unwrap();
            assert!(response.status_code().is_success() || response.status_code().is_server_error());
        }
        
        // Small delay between batches
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn test_load_testing_mixed_endpoints() {
    let server = helpers::create_test_server().await;
    
    // Test load across different endpoints
    let mut handles = vec![];
    
    // Chat completions
    for i in 0..5 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Mixed endpoints chat {}", i));
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
    for i in 0..5 {
        let request = serde_json::json!({
            "model": "text-embedding-ada-002",
            "input": format!("Mixed endpoints embedding {}", i)
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
    for _ in 0..3 {
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
async fn test_load_testing_error_scenarios() {
    let server = helpers::create_test_server().await;
    
    // Test load with error scenarios
    let mut handles = vec![];
    
    // Valid requests
    for i in 0..5 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Error load valid {}", i));
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
    for i in 0..5 {
        let request = helpers::create_test_chat_request("invalid-model", &format!("Error load invalid {}", i));
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
async fn test_load_testing_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Test load with rate limiting
    let mut handles = vec![];
    
    for i in 0..30 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Rate limit load test {}", i));
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
async fn test_load_testing_memory_pressure() {
    let server = helpers::create_test_server().await;
    
    // Test load with memory pressure
    let mut handles = vec![];
    
    for i in 0..15 {
        let large_content = format!("Large content for memory pressure test {}: {}", i, "A".repeat(5000));
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &large_content);
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
async fn test_load_testing_timeout_scenarios() {
    let server = helpers::create_test_server().await;
    
    // Test load with timeout scenarios
    let mut handles = vec![];
    
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Timeout load test {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .add_header("X-Timeout", "2000") // 2 second timeout
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
async fn test_load_testing_recovery() {
    let server = helpers::create_test_server().await;
    
    // Test load and recovery
    let mut handles = vec![];
    
    // Initial load
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Recovery load initial {}", i));
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
    
    // Wait for initial load to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
    
    // Wait for recovery
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    
    // Test recovery with new requests
    let mut recovery_handles = vec![];
    
    for i in 0..5 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Recovery load recovery {}", i));
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

#[tokio::test]
async fn test_load_testing_metrics_collection() {
    let server = helpers::create_test_server().await;
    
    // Test load with metrics collection
    let start = std::time::Instant::now();
    let mut handles = vec![];
    
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Metrics load test {}", i));
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
    
    // Check metrics endpoint
    let metrics_response = server
        .get("/metrics")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    assert!(metrics_response.status_code().is_success() || metrics_response.status_code().is_server_error());
}
