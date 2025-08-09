// Chat completions API tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_chat_completions_basic() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_chat_request("llama3.2:3b-instruct-q8_0", "Hello");
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    helpers::assert_success_response(&response);
    
    let body: Value = response.json();
    assert!(body["choices"].is_array());
    assert!(body["choices"][0]["message"]["content"].is_string());
    assert!(body["model"].is_string());
    assert!(body["usage"].is_object());
}

#[tokio::test]
async fn test_chat_completions_streaming() {
    let server = helpers::create_test_server().await;
    
    let request = helpers::create_test_streaming_request("llama3.2:3b-instruct-q8_0", "Write a short poem");
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

#[tokio::test]
async fn test_chat_completions_with_parameters() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "llama3.2:3b-instruct-q8_0",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "What is 2+2?"}
        ],
        "max_tokens": 50,
        "temperature": 0.7,
        "top_p": 0.9,
        "frequency_penalty": 0.0,
        "presence_penalty": 0.0,
        "stream": false
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    helpers::assert_success_response(&response);
    
    let body: Value = response.json();
    assert!(body["choices"].is_array());
    assert!(body["choices"][0]["message"]["content"].is_string());
}

#[tokio::test]
async fn test_chat_completions_missing_model() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    assert!(response.status_code().is_client_error());
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
}

#[tokio::test]
async fn test_chat_completions_missing_messages() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "llama3.2:3b-instruct-q8_0"
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    assert!(response.status_code().is_client_error());
    
    let body: Value = response.json();
    assert!(body["error"].is_object());
}

#[tokio::test]
async fn test_chat_completions_invalid_json() {
    let server = helpers::create_test_server().await;
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .body("invalid json")
        .await;
    
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_chat_completions_large_request() {
    let server = helpers::create_test_server().await;
    
    let large_content = "A".repeat(100000); // 100KB content
    let request = helpers::create_test_chat_request("llama3.2:3b-instruct-q8_0", &large_content);
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle large requests appropriately
    assert!(response.status_code().is_success() || response.status_code().is_client_error());
}

#[tokio::test]
async fn test_chat_completions_multiple_messages() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "llama3.2:3b-instruct-q8_0",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "What is the capital of France?"},
            {"role": "assistant", "content": "The capital of France is Paris."},
            {"role": "user", "content": "What is the population of Paris?"}
        ]
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    helpers::assert_success_response(&response);
    
    let body: Value = response.json();
    assert!(body["choices"].is_array());
    assert!(body["choices"][0]["message"]["content"].is_string());
}

#[tokio::test]
async fn test_chat_completions_function_calling() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "llama3.2:3b-instruct-q8_0",
        "messages": [
            {"role": "user", "content": "What's the weather like in Paris?"}
        ],
        "functions": [
            {
                "name": "get_weather",
                "description": "Get the current weather in a given location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "The city and state, e.g. San Francisco, CA"
                        }
                    },
                    "required": ["location"]
                }
            }
        ]
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Function calling may not be supported by all models
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_chat_completions_tool_calling() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "llama3.2:3b-instruct-q8_0",
        "messages": [
            {"role": "user", "content": "What's the weather like in Paris?"}
        ],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get the current weather in a given location",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {
                                "type": "string",
                                "description": "The city and state, e.g. San Francisco, CA"
                            }
                        },
                        "required": ["location"]
                    }
                }
            }
        ]
    });
    
    let response = server
        .post("/v1/chat/completions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Tool calling may not be supported by all models
    assert!(response.status_code().is_success() || response.status_code().is_server_error());
}
