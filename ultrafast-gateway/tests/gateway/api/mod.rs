// API endpoint tests
pub mod chat_completions;
pub mod embeddings;
pub mod images;
pub mod audio;
pub mod models;
pub mod admin;

use ultrafast_gateway as crate;
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_health_endpoint() {
    let server = helpers::create_test_server().await;
    
    let response = server.get("/health").await;
    helpers::assert_success_response(&response);
    
    let body: Value = response.json();
    assert_eq!(body["status"], "healthy");
    assert!(body["timestamp"].is_string());
    assert!(body["version"].is_string());
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let server = helpers::create_test_server().await;
    
    let response = server.get("/metrics").await;
    helpers::assert_success_response(&response);
    
    let body: Value = response.json();
    assert!(body["total_requests"].is_number());
    assert!(body["successful_requests"].is_number());
    assert!(body["failed_requests"].is_number());
    assert!(body["average_latency_ms"].is_number());
}

#[tokio::test]
async fn test_prometheus_metrics_endpoint() {
    let server = helpers::create_test_server().await;
    
    let response = server.get("/metrics/prometheus").await;
    helpers::assert_success_response(&response);
    
    let body = response.text();
    assert!(body.contains("ultrafast_gateway_requests_total"));
    assert!(body.contains("ultrafast_gateway_latency_seconds"));
}

#[tokio::test]
async fn test_dashboard_endpoint() {
    let server = helpers::create_test_server().await;
    
    let response = server.get("/dashboard").await;
    helpers::assert_success_response(&response);
    
    let body = response.text();
    assert!(body.contains("<!DOCTYPE html>"));
    assert!(body.contains("Ultrafast Gateway"));
}

#[tokio::test]
async fn test_dashboard_assets() {
    let server = helpers::create_test_server().await;
    
    // Test JavaScript
    let js_response = server.get("/dashboard.js").await;
    helpers::assert_success_response(&js_response);
    let js_body = js_response.text();
    assert!(js_body.contains("Ultrafast Gateway Dashboard"));
    
    // Test CSS
    let css_response = server.get("/dashboard.css").await;
    helpers::assert_success_response(&css_response);
    let css_body = css_response.text();
    assert!(css_body.contains("/* Ultrafast Gateway Dashboard"));
}

#[tokio::test]
async fn test_authentication_required() {
    let server = helpers::create_test_server().await;
    
    // Test endpoints that require authentication
    let endpoints = vec![
        "/v1/chat/completions",
        "/v1/embeddings",
        "/v1/images/generations",
        "/v1/audio/transcriptions",
        "/v1/audio/speech",
        "/v1/models",
        "/v1/providers",
        "/admin/config",
        "/admin/circuit-breakers",
    ];
    
    for endpoint in endpoints {
        let response = server.post(endpoint).await;
        // Should return 401 or 503 depending on configuration
        assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_invalid_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("test-model", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey invalid-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject invalid API key
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_request_validation() {
    let server = helpers::create_test_server().await;
    
    // Test malformed JSON
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .body("invalid json")
        .await;
    
    assert!(response.status_code().is_client_error());
    
    // Test missing required fields
    let invalid_request = serde_json::json!({
        "messages": [{"role": "user", "content": "Hello"}]
        // Missing model field
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&invalid_request)
        .await;
    
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_request_size_limits() {
    let server = helpers::create_test_server().await;
    
    // Test oversized request
    let large_content = "A".repeat(1000000); // 1MB content
    let request = helpers::create_test_chat_request("test-model", &large_content);
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle large requests appropriately
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_cors_headers() {
    let server = helpers::create_test_server().await;
    
    let response = server
        .get("/health")
        .add_header("Origin", "https://example.com")
        .await;
    
    helpers::assert_success_response(&response);
    
    // Check CORS headers
    let headers = response.headers();
    assert!(headers.contains_key("access-control-allow-origin"));
}

#[tokio::test]
async fn test_error_response_format() {
    let server = helpers::create_test_server().await;
    
    // Test with invalid model
    let request = helpers::create_test_chat_request("invalid-model", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
    assert!(body["error"]["code"].is_number());
    assert!(body["error"]["message"].is_string());
    assert!(body["error"]["type"].is_string());
}

#[tokio::test]
async fn test_streaming_response_format() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_streaming_request("test-model", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("Accept", "text/event-stream")
        .json(&request)
        .await;
    
    helpers::assert_success_response(&response);
    
    let headers = response.headers();
    assert_eq!(headers.get("content-type").unwrap(), "text/event-stream");
    assert_eq!(headers.get("cache-control").unwrap(), "no-cache");
    assert_eq!(headers.get("connection").unwrap(), "keep-alive");
    
    let body = response.text();
    assert!(body.contains("data: "));
}
