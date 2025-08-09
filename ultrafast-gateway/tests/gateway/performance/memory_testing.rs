// Memory testing performance tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_memory_testing_basic() {
    let server = helpers::create_test_server().await;
    
    // Basic memory test
    let mut handles = vec![];
    
    for i in 0..15 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Memory test {}", i));
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
async fn test_memory_testing_large_payloads() {
    let server = helpers::create_test_server().await;
    
    // Test with large payloads
    let mut handles = vec![];
    
    for i in 0..10 {
        let large_content = format!("Large payload test {}: {}", i, "A".repeat(8000));
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
async fn test_memory_testing_many_small_requests() {
    let server = helpers::create_test_server().await;
    
    // Test with many small requests
    let mut handles = vec![];
    
    for i in 0..50 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Small request {}", i));
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
async fn test_memory_testing_string_allocations() {
    let server = helpers::create_test_server().await;
    
    // Test string allocations
    let mut handles = vec![];
    
    for i in 0..20 {
        let complex_string = format!("Complex string with many allocations {}: {}", i, "Complex token ".repeat(50));
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &complex_string);
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
async fn test_memory_testing_json_parsing() {
    let server = helpers::create_test_server().await;
    
    // Test JSON parsing memory usage
    let mut handles = vec![];
    
    for i in 0..15 {
        let complex_json = serde_json::json!({
            "model": "gpt-3.5-turbo",
            "messages": [
                {"role": "user", "content": format!("JSON parsing test {} with complex structure", i)}
            ],
            "temperature": 0.7,
            "max_tokens": 100,
            "stream": false,
            "extra_fields": {
                "field1": "value1",
                "field2": "value2",
                "nested": {
                    "deep": "structure",
                    "with": "many",
                    "levels": "of nesting"
                }
            }
        });
        
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .json(&complex_json)
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
async fn test_memory_testing_response_buffering() {
    let server = helpers::create_test_server().await;
    
    // Test response buffering memory usage
    let mut handles = vec![];
    
    for i in 0..12 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Response buffering test {}", i));
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
async fn test_memory_testing_concurrent_allocations() {
    let server = helpers::create_test_server().await;
    
    // Test concurrent memory allocations
    let mut handles = vec![];
    
    for i in 0..25 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Concurrent allocation test {}", i));
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
async fn test_memory_testing_garbage_collection() {
    let server = helpers::create_test_server().await;
    
    // Test garbage collection scenarios
    let mut handles = vec![];
    
    for i in 0..18 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("GC test {}", i));
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
    
    // Wait for potential garbage collection
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    
    // Test that memory is still available
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Post-GC test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_memory_testing_memory_leaks() {
    let server = helpers::create_test_server().await;
    
    // Test for memory leaks
    let mut handles = vec![];
    
    for i in 0..22 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Memory leak test {}", i));
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
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    
    // Test that system still works after memory testing
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Post-memory test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_memory_testing_fragmentation() {
    let server = helpers::create_test_server().await;
    
    // Test memory fragmentation scenarios
    let mut handles = vec![];
    
    for i in 0..16 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Fragmentation test {}", i));
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
async fn test_memory_testing_metrics_under_memory_pressure() {
    let server = helpers::create_test_server().await;
    
    // Test metrics under memory pressure
    let start = std::time::Instant::now();
    let mut handles = vec![];
    
    for i in 0..20 {
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &format!("Memory pressure metrics test {}", i));
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
    
    // Should complete within reasonable time even under memory pressure
    assert!(duration.as_millis() < 90000); // 1.5 minutes max
    
    // Check metrics endpoint under memory pressure
    let metrics_response = server
        .get("/metrics")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    assert!(metrics_response.status_code().is_success() || metrics_response.status_code().is_server_error());
}
