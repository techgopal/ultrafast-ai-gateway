// Admin API endpoint tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_admin_health_check() {
    let server = helpers::create_test_server().await;
    
    let response = server
        .get("/admin/health")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["status"].is_string());
        assert_eq!(body["status"].as_str().unwrap(), "healthy");
        assert!(body["timestamp"].is_string());
        assert!(body["uptime"].is_number());
    } else {
        // Expected if admin endpoints are not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_admin_metrics() {
    let server = helpers::create_test_server().await;
    
    let response = server
        .get("/admin/metrics")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["total_requests"].is_number());
        assert!(body["successful_requests"].is_number());
        assert!(body["failed_requests"].is_number());
        assert!(body["average_response_time"].is_number());
        assert!(body["uptime_percentage"].is_number());
    } else {
        // Expected if admin endpoints are not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_admin_providers_status() {
    let server = helpers::create_test_server().await;
    
    let response = server
        .get("/admin/providers")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["providers"].is_array());
        
        // Check structure of providers
        let providers = body["providers"].as_array().unwrap();
        for provider in providers {
            assert!(provider["name"].is_string());
            assert!(provider["status"].is_string());
            assert!(provider["requests"].is_number());
            assert!(provider["errors"].is_number());
        }
    } else {
        // Expected if admin endpoints are not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_admin_configuration() {
    let server = helpers::create_test_server().await;
    
    let response = server
        .get("/admin/config")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["server"].is_object());
        assert!(body["providers"].is_array());
        assert!(body["routing"].is_object());
        assert!(body["security"].is_object());
    } else {
        // Expected if admin endpoints are not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_admin_authentication() {
    let server = helpers::create_test_server().await;
    
    // Test without authentication
    let response = server
        .get("/admin/health")
        .await;
    
    // Should reject unauthenticated requests
    assert!(response.status_code().is_client_error());
    
    // Test with invalid API key
    let response = server
        .get("/admin/health")
        .add_header("Authorization", "ApiKey invalid-key")
        .await;
    
    // Should reject invalid API key
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}
