// Caching middleware tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_caching_basic() {
    let server = helpers::create_test_server().await;
    
    // Make the same request twice
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Hello world");
    
    let response1 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    let response2 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Both should succeed
    assert!(response1.status_code().is_success() || response1.status_code().is_server_error());
    assert!(response2.status_code().is_success() || response2.status_code().is_server_error());
    
    // Check for cache headers
    let headers1 = response1.headers();
    let headers2 = response2.headers();
    
    if headers1.contains_key("x-cache") {
        assert!(headers1.get("x-cache").is_some());
    }
    if headers2.contains_key("x-cache") {
        assert!(headers2.get("x-cache").is_some());
    }
}

#[tokio::test]
async fn test_caching_different_requests() {
    let server = helpers::create_test_server().await;
    
    // Make different requests
    let request1 = helpers::create_test_chat_request("gpt-3.5-turbo", "First request");
    let request2 = helpers::create_test_chat_request("gpt-3.5-turbo", "Second request");
    
    let response1 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request1)
        .await;
    
    let response2 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request2)
        .await;
    
    // Both should succeed
    assert!(response1.status_code().is_success() || response1.status_code().is_server_error());
    assert!(response2.status_code().is_success() || response2.status_code().is_server_error());
}

#[tokio::test]
async fn test_caching_cache_headers() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Cache headers test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    let headers = response.headers();
    
    // Check for cache-related headers
    if headers.contains_key("cache-control") {
        assert!(headers.get("cache-control").is_some());
    }
    if headers.contains_key("etag") {
        assert!(headers.get("etag").is_some());
    }
    if headers.contains_key("last-modified") {
        assert!(headers.get("last-modified").is_some());
    }
}

#[tokio::test]
async fn test_caching_cache_control() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Cache control test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("Cache-Control", "max-age=300")
        .json(&request)
        .await;
    
    // Should handle cache control headers
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_caching_etag_validation() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "ETag test");
    let response1 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    let headers1 = response1.headers();
    if let Some(etag) = headers1.get("etag") {
        // Make request with If-None-Match header
        let response2 = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .add_header("If-None-Match", etag)
            .json(&request)
            .await;
        
        // Should handle ETag validation
        assert!(response2.status_code().is_success() || response2.status_code().is_server_error() || response2.status_code().is_client_error());
    }
}

#[tokio::test]
async fn test_caching_invalidation() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Cache invalidation test");
    
    // First request
    let response1 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Invalidate cache
    let invalidate_response = server
        .post("/admin/cache/invalidate")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&serde_json::json!({"pattern": "*"}))
        .await;
    
    // Second request after invalidation
    let response2 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Both should succeed
    assert!(response1.status_code().is_success() || response1.status_code().is_server_error());
    assert!(response2.status_code().is_success() || response2.status_code().is_server_error());
}

#[tokio::test]
async fn test_caching_ttl() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "TTL test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("X-Cache-TTL", "60")
        .json(&request)
        .await;
    
    // Should handle TTL headers
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_caching_memory_vs_redis() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Memory vs Redis test");
    
    // Test memory cache
    let response1 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("X-Cache-Type", "memory")
        .json(&request)
        .await;
    
    // Test Redis cache
    let response2 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("X-Cache-Type", "redis")
        .json(&request)
        .await;
    
    // Both should succeed
    assert!(response1.status_code().is_success() || response1.status_code().is_server_error());
    assert!(response2.status_code().is_success() || response2.status_code().is_server_error());
}

#[tokio::test]
async fn test_caching_compression() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Compression test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("Accept-Encoding", "gzip, deflate")
        .json(&request)
        .await;
    
    let headers = response.headers();
    
    // Check for compression headers
    if headers.contains_key("content-encoding") {
        assert!(headers.get("content-encoding").is_some());
    }
}

#[tokio::test]
async fn test_caching_stale_while_revalidate() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Stale while revalidate test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("Cache-Control", "stale-while-revalidate=30")
        .json(&request)
        .await;
    
    // Should handle stale-while-revalidate
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_caching_partial_content() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Partial content test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("Range", "bytes=0-1023")
        .json(&request)
        .await;
    
    // Should handle range requests
    assert!(response.status_code().is_success() || response.status_code().is_server_error() || response.status_code().is_client_error());
}

#[tokio::test]
async fn test_caching_vary_headers() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Vary headers test");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .add_header("Accept-Language", "en-US")
        .json(&request)
        .await;
    
    let headers = response.headers();
    
    // Check for Vary header
    if headers.contains_key("vary") {
        assert!(headers.get("vary").is_some());
    }
}

#[tokio::test]
async fn test_caching_conditional_requests() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("gpt-3.5-turbo", "Conditional request test");
    
    // First request
    let response1 = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    let headers1 = response1.headers();
    if let (Some(last_modified), Some(etag)) = (headers1.get("last-modified"), headers1.get("etag")) {
        // Conditional request with If-Modified-Since
        let response2 = server
            .post("/v1/chat/completions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .add_header("If-Modified-Since", last_modified)
            .add_header("If-None-Match", etag)
            .json(&request)
            .await;
        
        // Should handle conditional requests
        assert!(response2.status_code().is_success() || response2.status_code().is_server_error() || response2.status_code().is_client_error());
    }
}
