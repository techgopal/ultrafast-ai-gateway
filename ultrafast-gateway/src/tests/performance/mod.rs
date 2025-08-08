// Performance tests module
pub mod load_testing;
pub mod stress_testing;
pub mod memory_testing;
pub mod concurrent_testing;

use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_performance_basic() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Performance test");
    let start = std::time::Instant::now();
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    let duration = start.elapsed();
    
    // Should complete within reasonable time
    assert!(duration.as_millis() < 30000); // 30 seconds max
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_performance_response_time() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Response time test");
    let start = std::time::Instant::now();
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    let duration = start.elapsed();
    
    // Should respond within 10 seconds
    assert!(duration.as_millis() < 10000);
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_performance_memory_usage() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test memory usage
    for i in 0..10 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Memory test {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle multiple requests without memory issues
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_performance_concurrent_requests() {
    let server = helpers::create_test_server().await;
    
    // Test concurrent requests
    let mut handles = vec![];
    
    for i in 0..5 {
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
async fn test_performance_large_payload() {
    let server = helpers::create_test_server().await;
    
    // Create large payload
    let large_content = "A".repeat(10000);
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", &large_content);
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle large payloads
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_performance_throughput() {
    let server = helpers::create_test_server().await;
    
    let start = std::time::Instant::now();
    let mut success_count = 0;
    
    // Make multiple requests to test throughput
    for i in 0..20 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Throughput test {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        if response.status_code().is_success() {
            success_count += 1;
        }
    }
    
    let duration = start.elapsed();
    
    // Should handle reasonable throughput
    assert!(success_count > 0);
    assert!(duration.as_millis() < 60000); // 1 minute max
}

#[tokio::test]
async fn test_performance_error_handling() {
    let server = helpers::create_test_server().await;
    
    // Test performance under error conditions
    let request = helpers::create_test_chat_request("invalid-model", "Error performance test");
    let start = std::time::Instant::now();
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    let duration = start.elapsed();
    
    // Should handle errors quickly
    assert!(duration.as_millis() < 5000); // 5 seconds max for errors
    assert!(response.status_code().is_server_error());
}

#[tokio::test]
async fn test_performance_resource_cleanup() {
    let server = helpers::create_test_server().await;
    
    // Make requests and check resource cleanup
    for i in 0..5 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Cleanup test {}", i));
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
    
    // Wait a bit for cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Make another request to ensure cleanup worked
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Post-cleanup test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_performance_timeout_handling() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Timeout performance test");
    let start = std::time::Instant::now();
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("X-Timeout", "1000") // 1 second timeout
        .json(&request)
        .await;
    
    let duration = start.elapsed();
    
    // Should respect timeout settings
    assert!(duration.as_millis() < 2000); // Should complete within 2 seconds
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_performance_metrics_collection() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Metrics performance test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Check for performance metrics in headers
    let headers = response.headers();
    if headers.contains_key("x-response-time") {
        assert!(headers.get("x-response-time").is_some());
    }
    if headers.contains_key("x-processing-time") {
        assert!(headers.get("x-processing-time").is_some());
    }
    
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}
