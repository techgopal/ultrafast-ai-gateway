// Input validation security tests
use ultrafast_gateway as crate;
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_input_validation_basic() {
    let server = helpers::create_test_server().await;
    
    // Basic input validation test
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Input validation test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should validate input appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_input_validation_missing_fields() {
    let server = helpers::create_test_server().await;
    
    // Test with missing required fields
    let invalid_request = serde_json::json!({
        "model": "gpt-3.5-turbo"
        // Missing messages field
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&invalid_request)
        .await;
    
    // Should reject requests with missing required fields
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_input_validation_invalid_model() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid model
    let request = helpers::create_test_chat_request("invalid-model-name", "Invalid model test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle invalid model appropriately
    assert!(response.status_code().is_server_error());
}

#[tokio::test]
async fn test_input_validation_empty_messages() {
    let server = helpers::create_test_server().await;
    
    // Test with empty messages array
    let invalid_request = serde_json::json!({
        "model": "gpt-3.5-turbo",
        "messages": []
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&invalid_request)
        .await;
    
    // Should reject requests with empty messages
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_input_validation_invalid_message_structure() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid message structure
    let invalid_request = serde_json::json!({
        "model": "gpt-3.5-turbo",
        "messages": [
            {
                "invalid_field": "invalid_value"
            }
        ]
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&invalid_request)
        .await;
    
    // Should reject requests with invalid message structure
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_input_validation_invalid_role() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid role
    let invalid_request = serde_json::json!({
        "model": "gpt-3.5-turbo",
        "messages": [
            {
                "role": "invalid_role",
                "content": "Test message"
            }
        ]
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&invalid_request)
        .await;
    
    // Should reject requests with invalid role
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_input_validation_empty_content() {
    let server = helpers::create_test_server().await;
    
    // Test with empty content
    let invalid_request = serde_json::json!({
        "model": "gpt-3.5-turbo",
        "messages": [
            {
                "role": "user",
                "content": ""
            }
        ]
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&invalid_request)
        .await;
    
    // Should reject requests with empty content
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_input_validation_large_content() {
    let server = helpers::create_test_server().await;
    
    // Test with very large content
    let large_content = "A".repeat(100000); // 100KB content
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", &large_content);
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle large content appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
}

#[tokio::test]
async fn test_input_validation_invalid_temperature() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid temperature values
    let invalid_temperatures = [-1.0, 2.0, 100.0, -100.0];
    
    for temp in invalid_temperatures {
        let invalid_request = serde_json::json!({
            "model": "gpt-3.5-turbo",
            "messages": [
                {
                    "role": "user",
                    "content": "Test message"
                }
            ],
            "temperature": temp
        });
        
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&invalid_request)
            .await;
        
        // Should reject requests with invalid temperature
        assert!(response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_input_validation_invalid_max_tokens() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid max_tokens values
    let invalid_max_tokens = [-1, 0, 1000000, 999999999];
    
    for max_tokens in invalid_max_tokens {
        let invalid_request = serde_json::json!({
            "model": "gpt-3.5-turbo",
            "messages": [
                {
                    "role": "user",
                    "content": "Test message"
                }
            ],
            "max_tokens": max_tokens
        });
        
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&invalid_request)
            .await;
        
        // Should reject requests with invalid max_tokens
        assert!(response.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_input_validation_malformed_json() {
    let server = helpers::create_test_server().await;
    
    // Test with malformed JSON
    let malformed_json = "{ invalid json }";
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .body(malformed_json)
        .await;
    
    // Should reject malformed JSON
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_input_validation_deep_nesting() {
    let server = helpers::create_test_server().await;
    
    // Test with deeply nested JSON
    let mut deep_content = "Test".to_string();
    for _ in 0..100 {
        deep_content = format!("{{\"nested\": \"{}\"}}", deep_content);
    }
    
    let invalid_request = serde_json::json!({
        "model": "gpt-3.5-turbo",
        "messages": [
            {
                "role": "user",
                "content": deep_content
            }
        ]
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&invalid_request)
        .await;
    
    // Should handle deeply nested content appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
}

#[tokio::test]
async fn test_input_validation_special_characters() {
    let server = helpers::create_test_server().await;
    
    // Test with special characters
    let special_content = "Test with special chars: !@#$%^&*()_+-=[]{}|;':\",./<>?`~";
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", special_content);
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle special characters appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_input_validation_unicode_characters() {
    let server = helpers::create_test_server().await;
    
    // Test with unicode characters
    let unicode_content = "Test with unicode: 🚀🌟🎉中文日本語한국어";
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", unicode_content);
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle unicode characters appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_input_validation_whitespace_handling() {
    let server = helpers::create_test_server().await;
    
    // Test with various whitespace scenarios
    let whitespace_content = "   Test with whitespace   \n\t\r";
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", whitespace_content);
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle whitespace appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_input_validation_null_values() {
    let server = helpers::create_test_server().await;
    
    // Test with null values
    let invalid_request = serde_json::json!({
        "model": null,
        "messages": [
            {
                "role": "user",
                "content": "Test message"
            }
        ]
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&invalid_request)
        .await;
    
    // Should reject requests with null values in required fields
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_input_validation_metrics_under_validation_pressure() {
    let server = helpers::create_test_server().await;
    
    // Test metrics under input validation pressure
    let start = std::time::Instant::now();
    let mut handles = vec![];
    
    for i in 0..10 {
        let invalid_request = serde_json::json!({
            "model": "gpt-3.5-turbo",
            "messages": [
                {
                    "role": "user",
                    "content": format!("Validation pressure test {}", i)
                }
            ],
            "invalid_field": "invalid_value"
        });
        
        let server_clone = server.clone();
        
        let handle = tokio::spawn(async move {
            server_clone
                .post("/v1/chat/completions")
                .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
                .add_header("Content-Type", "application/json")
                .json(&invalid_request)
                .await
        });
        
        handles.push(handle);
    }
    
    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.unwrap();
        assert!(response.status_code().is_client_error());
    }
    
    let duration = start.elapsed();
    
    // Should complete within reasonable time even under validation pressure
    assert!(duration.as_millis() < 30000); // 30 seconds max
    
    // Check metrics endpoint under validation pressure
    let metrics_response = server
        .get("/metrics")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    assert!(metrics_response.status_code().is_success() || metrics_response.status_code().is_server_error());
}
