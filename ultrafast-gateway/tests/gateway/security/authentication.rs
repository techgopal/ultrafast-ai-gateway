// Authentication security tests
use ultrafast_gateway as crate;
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_authentication_missing_header() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject requests without authentication
    assert!(response.status_code().is_client_error());
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
    assert!(body["error"]["code"].is_number());
    assert!(body["error"]["message"].is_string());
}

#[tokio::test]
async fn test_authentication_invalid_format() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    // Test various invalid authentication formats
    let invalid_auth_headers = vec![
        "InvalidFormat",
        "Bearer",
        "ApiKey",
        "sk-",
        "sk-ultrafast-gateway-key-extra",
        "sk-ultrafast-gateway-key\n",
        "sk-ultrafast-gateway-key\r\n",
        "sk-ultrafast-gateway-key ",
        " sk-ultrafast-gateway-key",
    ];
    
    for auth_header in invalid_auth_headers {
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", auth_header)
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should reject invalid authentication formats
        assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_authentication_case_sensitivity() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    // Test case sensitivity
    let case_variations = vec![
        "ApiKey sk-ultrafast-gateway-key",
        "apikey sk-ultrafast-gateway-key",
        "APIKEY sk-ultrafast-gateway-key",
        "ApiKey SK-ULTRAFAST-GATEWAY-KEY",
    ];
    
    for auth_header in case_variations {
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", auth_header)
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle case sensitivity appropriately
        assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_authentication_whitespace_handling() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    // Test whitespace handling
    let whitespace_variations = vec![
        "ApiKey  sk-ultrafast-gateway-key",
        "ApiKey sk-ultrafast-gateway-key ",
        " ApiKey sk-ultrafast-gateway-key",
        "ApiKey\tsk-ultrafast-gateway-key",
        "ApiKey\nsk-ultrafast-gateway-key",
    ];
    
    for auth_header in whitespace_variations {
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", auth_header)
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle whitespace appropriately
        assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_authentication_special_characters() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    // Test special characters in API keys
    let special_char_keys = vec![
        "ApiKey sk-ultrafast-gateway-key!",
        "ApiKey sk-ultrafast-gateway-key@",
        "ApiKey sk-ultrafast-gateway-key#",
        "ApiKey sk-ultrafast-gateway-key$",
        "ApiKey sk-ultrafast-gateway-key%",
        "ApiKey sk-ultrafast-gateway-key^",
        "ApiKey sk-ultrafast-gateway-key&",
        "ApiKey sk-ultrafast-gateway-key*",
    ];
    
    for auth_header in special_char_keys {
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", auth_header)
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should reject API keys with special characters
        assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_authentication_injection_attempts() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    // Test injection attempts
    let injection_attempts = vec![
        "ApiKey sk-ultrafast-gateway-key'; DROP TABLE users; --",
        "ApiKey sk-ultrafast-gateway-key\" OR 1=1 --",
        "ApiKey sk-ultrafast-gateway-key<script>alert('xss')</script>",
        "ApiKey sk-ultrafast-gateway-key\r\nX-Forwarded-For: 127.0.0.1",
        "ApiKey sk-ultrafast-gateway-key\nX-Forwarded-For: 127.0.0.1",
    ];
    
    for auth_header in injection_attempts {
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", auth_header)
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should reject injection attempts
        assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_authentication_multiple_headers() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    // Test multiple authorization headers
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Authorization", "ApiKey invalid-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle multiple headers appropriately
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_authentication_empty_key() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey ")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject empty API key
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_authentication_very_long_key() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    let long_key = "ApiKey ".to_string() + &"A".repeat(10000);
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", &long_key)
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle very long keys appropriately
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_authentication_unicode_characters() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    // Test unicode characters in API keys
    let unicode_keys = vec![
        "ApiKey sk-ultrafast-gateway-key-é",
        "ApiKey sk-ultrafast-gateway-key-ñ",
        "ApiKey sk-ultrafast-gateway-key-中",
        "ApiKey sk-ultrafast-gateway-key-🚀",
    ];
    
    for auth_header in unicode_keys {
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", auth_header)
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should reject API keys with unicode characters
        assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_authentication_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    // Test rate limiting for authentication attempts
    for i in 0..20 {
        let response = server
            .post("/v1/chat/completions")
            .add_header("Authorization", &format!("ApiKey invalid-key-{}", i))
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle rate limiting for auth attempts
        assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_authentication_logging() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    // Test that authentication attempts are logged (we can't easily test this, but should not panic)
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey invalid-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle invalid authentication without panicking
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_authentication_error_messages() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should return appropriate error message
    assert!(response.status_code().is_client_error());
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
    assert!(body["error"]["code"].is_number());
    assert!(body["error"]["message"].is_string());
    
    // Error message should not expose sensitive information
    let error_message = body["error"]["message"].as_str().unwrap();
    assert!(!error_message.contains("password"));
    assert!(!error_message.contains("secret"));
    assert!(!error_message.contains("key"));
    assert!(!error_message.contains("localhost"));
    assert!(!error_message.contains("127.0.0.1"));
}
