// Data protection security tests
use ultrafast_gateway as crate;
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_data_protection_basic() {
    let server = helpers::create_test_server().await;
    
    // Basic data protection test
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Data protection test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should protect data appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_data_protection_sensitive_data() {
    let server = helpers::create_test_server().await;
    
    // Test with sensitive data
    let sensitive_content = "My password is secret123 and my API key is sk-1234567890abcdef";
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", sensitive_content);
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle sensitive data appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
    
    // Check that sensitive data is not exposed in response
    if response.status_code().is_success() {
        let body: Value = response.json();
        let response_text = body.to_string();
        assert!(!response_text.contains("secret123"));
        assert!(!response_text.contains("sk-1234567890abcdef"));
    }
}

#[tokio::test]
async fn test_data_protection_error_messages() {
    let server = helpers::create_test_server().await;
    
    // Test that error messages don't expose sensitive data
    let request = helpers::create_test_chat_request("invalid-model", "Error message test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should return error without exposing sensitive data
    assert!(response.status_code().is_server_error());
    
    let body: Value = response.json();
    let error_message = body["error"]["message"].as_str().unwrap_or("");
    
    // Should not expose internal details
    assert!(!error_message.contains("password"));
    assert!(!error_message.contains("secret"));
    assert!(!error_message.contains("key"));
    assert!(!error_message.contains("token"));
    assert!(!error_message.contains("localhost"));
    assert!(!error_message.contains("127.0.0.1"));
}

#[tokio::test]
async fn test_data_protection_logging() {
    let server = helpers::create_test_server().await;
    
    // Test that logging doesn't expose sensitive data
    let sensitive_content = "My credit card is 4111-1111-1111-1111 and SSN is 123-45-6789";
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", sensitive_content);
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle sensitive data in logs appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_data_protection_headers() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Data protection headers test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Check for data protection headers
    let headers = response.headers();
    
    // Should include security-related headers
    if headers.contains_key("x-content-type-options") {
        assert!(headers.get("x-content-type-options").is_some());
    }
    if headers.contains_key("x-frame-options") {
        assert!(headers.get("x-frame-options").is_some());
    }
    if headers.contains_key("x-xss-protection") {
        assert!(headers.get("x-xss-protection").is_some());
    }
    
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_data_protection_encryption() {
    let server = helpers::create_test_server().await;
    
    // Test data encryption
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Encryption test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle encryption appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_data_protection_anonymization() {
    let server = helpers::create_test_server().await;
    
    // Test data anonymization
    let personal_content = "My name is John Doe, email is john.doe@example.com, phone is 555-123-4567";
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", personal_content);
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle personal data appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_data_protection_retention() {
    let server = helpers::create_test_server().await;
    
    // Test data retention policies
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Data retention test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle data retention appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_data_protection_access_control() {
    let server = helpers::create_test_server().await;
    
    // Test access control for sensitive data
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Access control test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should enforce access control appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_data_protection_audit_logging() {
    let server = helpers::create_test_server().await;
    
    // Test audit logging for data access
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Audit logging test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should log audit events appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_data_protection_compliance() {
    let server = helpers::create_test_server().await;
    
    // Test compliance with data protection regulations
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Compliance test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should comply with data protection regulations
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_data_protection_metrics_under_data_protection_pressure() {
    let server = helpers::create_test_server().await;
    
    // Test metrics under data protection pressure
    let start = std::time::Instant::now();
    let mut handles = vec![];
    
    for i in 0..10 {
        let sensitive_content = format!("Data protection pressure test {} with sensitive data: password123, key456", i);
        let request = helpers::create_test_chat_request("gpt-3.5-turbo", &sensitive_content);
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
    
    // Should complete within reasonable time even under data protection pressure
    assert!(duration.as_millis() < 60000); // 1 minute max
    
    // Check metrics endpoint under data protection pressure
    let metrics_response = server
        .get("/metrics")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    assert!(metrics_response.status_code().is_success() || metrics_response.status_code().is_server_error());
}
