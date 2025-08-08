use futures::StreamExt;
use std::time::Duration;
use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🚀 Ultrafast Gateway with Ollama Client Example");
    println!("{}", "=".repeat(50));

    // Create gateway client
    let client = UltrafastClient::gateway("http://localhost:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    // Test 1: Basic chat completion with llama3.2
    println!("\n=== Test 1: Basic Chat Completion ===");
    let response = client
        .chat_completion(ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user("What is the capital of France?")],
            max_tokens: Some(100),
            temperature: Some(0.7),
            ..Default::default()
        })
        .await?;

    println!("Response: {}", response.choices[0].message.content);
    println!("Model: {}", response.model);
    println!("Tokens used: {}", response.usage.unwrap().total_tokens);

    // Test 2: Chat completion with qwen3
    println!("\n=== Test 2: Chat Completion with Qwen3 ===");
    let response = client
        .chat_completion(ChatRequest {
            model: "qwen3:8b".to_string(),
            messages: vec![Message::user("Explain quantum computing in simple terms")],
            max_tokens: Some(150),
            temperature: Some(0.8),
            ..Default::default()
        })
        .await?;

    println!("Response: {}", response.choices[0].message.content);
    println!("Model: {}", response.model);
    println!("Tokens used: {}", response.usage.unwrap().total_tokens);

    // Test 3: Streaming response
    println!("\n=== Test 3: Streaming Response ===");
    let mut stream = client
        .stream_chat_completion(ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user(
                "Write a short story about a robot learning to paint",
            )],
            max_tokens: Some(200),
            temperature: Some(0.9),
            stream: Some(true),
            ..Default::default()
        })
        .await?;

    print!("Streaming response: ");
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                if let Some(content) = &chunk.choices[0].delta.content {
                    print!("{content}");
                }
            }
            Err(e) => {
                println!("\nError in stream: {e:?}");
                break;
            }
        }
    }
    println!();

    // Test 4: Multiple requests in parallel
    println!("\n=== Test 4: Parallel Requests ===");
    let requests = vec![
        ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user("What is 2+2?")],
            max_tokens: Some(10),
            ..Default::default()
        },
        ChatRequest {
            model: "qwen3:8b".to_string(),
            messages: vec![Message::user("What is 3+3?")],
            max_tokens: Some(10),
            ..Default::default()
        },
        ChatRequest {
            model: "gemma3:4b".to_string(),
            messages: vec![Message::user("What is 4+4?")],
            max_tokens: Some(10),
            ..Default::default()
        },
    ];

    let futures: Vec<_> = requests
        .into_iter()
        .map(|req| client.chat_completion(req))
        .collect();

    let results = futures::future::join_all(futures).await;

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(response) => {
                println!("Request {}: {}", i + 1, response.choices[0].message.content);
            }
            Err(e) => {
                println!("Request {} failed: {:?}", i + 1, e);
            }
        }
    }

    // Test 5: Get last used provider
    println!("\n=== Test 5: Last Used Provider ===");
    if let Some(provider) = client.get_last_used_provider().await {
        println!("Last used provider: {provider}");
    } else {
        println!("No provider used yet");
    }

    println!("\n✅ All tests completed successfully!");
    Ok(())
}
