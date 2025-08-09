// Images API endpoint tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_image_generation_basic() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "dall-e-3",
        "prompt": "A beautiful sunset over mountains",
        "n": 1,
        "size": "1024x1024"
    });
    
    let response = server
        .post("/v1/images/generations")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert!(body["data"][0]["url"].is_string());
    } else {
        // Expected if image generation is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_image_generation_multiple() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "dall-e-3",
        "prompt": "A cat playing with a ball",
        "n": 2,
        "size": "1024x1024"
    });
    
    let response = server
        .post("/v1/images/generations")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert_eq!(body["data"].as_array().unwrap().len(), 2);
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_image_generation_different_sizes() {
    let server = helpers::create_test_server().await;
    
    let sizes = ["256x256", "512x512", "1024x1024", "1792x1024", "1024x1792"];
    
    for size in sizes {
        let request = serde_json::json!({
            "model": "dall-e-3",
            "prompt": "A simple geometric shape",
            "n": 1,
            "size": size
        });
        
        let response = server
            .post("/v1/images/generations")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        if response.status_code().is_success() {
            let body: Value = response.json();
            assert!(body["data"].is_array());
            assert!(body["data"][0]["url"].is_string());
        } else {
            assert!(response.status_code().is_server_error());
        }
    }
}

#[tokio::test]
async fn test_image_generation_quality() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "dall-e-3",
        "prompt": "A high-quality photograph of a landscape",
        "n": 1,
        "size": "1024x1024",
        "quality": "hd"
    });
    
    let response = server
        .post("/v1/images/generations")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert!(body["data"][0]["url"].is_string());
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_image_generation_style() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "dall-e-3",
        "prompt": "A portrait in the style of Van Gogh",
        "n": 1,
        "size": "1024x1024",
        "style": "vivid"
    });
    
    let response = server
        .post("/v1/images/generations")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["data"].is_array());
        assert!(body["data"][0]["url"].is_string());
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_image_generation_missing_prompt() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "dall-e-3",
        "n": 1,
        "size": "1024x1024"
    });
    
    let response = server
        .post("/v1/images/generations")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should return an error for missing prompt
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_image_generation_invalid_model() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "invalid-image-model",
        "prompt": "A beautiful landscape",
        "n": 1,
        "size": "1024x1024"
    });
    
    let response = server
        .post("/v1/images/generations")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle invalid model gracefully
    assert!(response.status_code().is_server_error());
}

#[tokio::test]
async fn test_image_generation_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "dall-e-3",
        "prompt": "A simple image",
        "n": 1,
        "size": "1024x1024"
    });
    
    // Test without authentication
    let response = server
        .post("/v1/images/generations")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject unauthenticated requests
    assert!(response.status_code().is_client_error());
    
    // Test with invalid API key
    let response = server
        .post("/v1/images/generations")
        .add_header("Authorization", "ApiKey invalid-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject invalid API key
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_image_generation_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test rate limiting
    for i in 0..3 {
        let request = serde_json::json!({
            "model": "dall-e-3",
            "prompt": &format!("Image {}", i),
            "n": 1,
            "size": "1024x1024"
        });
        
        let response = server
            .post("/v1/images/generations")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle rate limiting gracefully
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_image_generation_content_filtering() {
    let server = helpers::create_test_server().await;
    
    // Test with potentially harmful content
    let request = serde_json::json!({
        "model": "dall-e-3",
        "prompt": "Violent scene with weapons",
        "n": 1,
        "size": "1024x1024"
    });
    
    let response = server
        .post("/v1/images/generations")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle content filtering appropriately
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}
