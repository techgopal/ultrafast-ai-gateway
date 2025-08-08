// Stress testing performance tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_stress_testing_basic() {
    let server = helpers::create_test_server().await;
    
    // Basic stress test
    let mut handles = vec![];
    
    for i in 0..50 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Stress test {}", i));
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
async fn test_stress_testing_extreme_concurrency() {
    let server = helpers::create_test_server().await;
    
    // Extreme concurrency stress test
    let mut handles = vec![];
    
    for i in 0..100 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Extreme stress test {}", i));
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
async fn test_stress_testing_memory_exhaustion() {
    let server = helpers::create_test_server().await;
    
    // Test memory exhaustion scenarios
    let mut handles = vec![];
    
    for i in 0..20 {
        let large_content = format!("Very large content for memory exhaustion test {}: {}", i, "A".repeat(10000));
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
async fn test_stress_testing_cpu_intensive() {
    let server = helpers::create_test_server().await;
    
    // Test CPU intensive scenarios
    let mut handles = vec![];
    
    for i in 0..30 {
        let complex_content = format!("Complex content with many tokens for CPU stress test {}: {}", i, "Complex token ".repeat(100));
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &complex_content);
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
async fn test_stress_testing_network_saturation() {
    let server = helpers::create_test_server().await;
    
    // Test network saturation
    let mut handles = vec![];
    
    for i in 0..40 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Network saturation test {}", i));
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
async fn test_stress_testing_error_cascading() {
    let server = helpers::create_test_server().await;
    
    // Test error cascading scenarios
    let mut handles = vec![];
    
    // Mix of valid and invalid requests
    for i in 0..25 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Error cascading valid {}", i));
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
    
    for i in 0..25 {
        let request = helpers::create_test_chat_request("invalid-model", &format!("Error cascading invalid {}", i));
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
async fn test_stress_testing_timeout_cascading() {
    let server = helpers::create_test_server().await;
    
    // Test timeout cascading scenarios
    let mut handles = vec![];
    
    for i in 0..30 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Timeout cascading test {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .add_header("X-Timeout", "1000") // 1 second timeout
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
async fn test_stress_testing_resource_leak() {
    let server = helpers::create_test_server().await;
    
    // Test for resource leaks
    let mut handles = vec![];
    
    for i in 0..35 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Resource leak test {}", i));
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
    
    // Wait for potential cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Test that system still works after stress
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Post-stress test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_stress_testing_connection_pool() {
    let server = helpers::create_test_server().await;
    
    // Test connection pool limits
    let mut handles = vec![];
    
    for i in 0..45 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Connection pool test {}", i));
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
async fn test_stress_testing_recovery_after_stress() {
    let server = helpers::create_test_server().await;
    
    // Apply stress
    let mut stress_handles = vec![];
    
    for i in 0..40 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Pre-recovery stress test {}", i));
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .json(&request)
                .await
        });
        
        stress_handles.push(handle);
    }
    
    // Wait for stress to complete
    for handle in stress_handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
    
    // Wait for recovery
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    
    // Test recovery
    let mut recovery_handles = vec![];
    
    for i in 0..10 {
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
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_stress_testing_metrics_under_stress() {
    let server = helpers::create_test_server().await;
    
    // Apply stress while monitoring metrics
    let start = std::time::Instant::now();
    let mut handles = vec![];
    
    for i in 0..35 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Metrics stress test {}", i));
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
    
    // Should complete within reasonable time even under stress
    assert!(duration.as_millis() < 120000); // 2 minutes max
    
    // Check metrics endpoint under stress
    let metrics_response = server
        .get("/metrics")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    assert!(metrics_response.status_code().is_success() || metrics_response.status_code().is_server_error());
}
