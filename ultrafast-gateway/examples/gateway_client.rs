use futures::StreamExt;
use std::time::Duration;
use ultrafast_models_sdk::{
    models::{EmbeddingInput, EmbeddingRequest},
    ChatRequest, Message, UltrafastClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Example 1: Basic gateway client
    println!("=== Example 1: Basic Gateway Client ===");
    let client = UltrafastClient::gateway("http://localhost:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    let response = client
        .chat_completion(ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("Hello! What is the capital of Japan?")],
            max_tokens: Some(100),
            temperature: Some(0.7),
            ..Default::default()
        })
        .await?;

    println!("Response: {}", response.choices[0].message.content);

    // Example 2: Gateway client with custom timeout
    println!("\n=== Example 2: Gateway Client with Custom Timeout ===");
    let client = UltrafastClient::gateway("http://localhost:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .with_timeout(Duration::from_secs(60))
        .build()?;

    let response = client
        .chat_completion(ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user(
                "Write a detailed explanation of blockchain technology",
            )],
            max_tokens: Some(500),
            temperature: Some(0.8),
            ..Default::default()
        })
        .await?;

    println!("Response: {}", response.choices[0].message.content);

    // Example 3: Streaming through gateway
    println!("\n=== Example 3: Streaming Through Gateway ===");
    let mut stream = client
        .stream_chat_completion(ChatRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: vec![Message::user("Tell me a story about a magical forest")],
            max_tokens: Some(300),
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

    // Example 4: Embeddings through gateway
    println!("\n=== Example 4: Embeddings Through Gateway ===");
    let embedding_response = client
        .embedding(EmbeddingRequest {
            model: "text-embedding-ada-002".to_string(),
            input: EmbeddingInput::String(
                "This is a test sentence for embeddings through the gateway.".to_string(),
            ),
            ..Default::default()
        })
        .await?;

    println!(
        "Embedding dimensions: {}",
        embedding_response.data[0].embedding.len()
    );

    // Example 5: Error handling
    println!("\n=== Example 5: Error Handling ===");
    match client
        .chat_completion(ChatRequest {
            model: "non-existent-model".to_string(),
            messages: vec![Message::user("This should fail")],
            max_tokens: Some(10),
            ..Default::default()
        })
        .await
    {
        Ok(response) => {
            println!(
                "Unexpected success: {}",
                response.choices[0].message.content
            );
        }
        Err(e) => {
            println!("Expected error: {e:?}");
        }
    }

    // Example 6: Multiple requests in parallel
    println!("\n=== Example 6: Parallel Requests ===");
    let requests = vec![
        ChatRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: vec![Message::user("What is 2+2?")],
            max_tokens: Some(10),
            ..Default::default()
        },
        ChatRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: vec![Message::user("What is 3+3?")],
            max_tokens: Some(10),
            ..Default::default()
        },
        ChatRequest {
            model: "gpt-3.5-turbo".to_string(),
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

    // Example 7: Health check simulation
    println!("\n=== Example 7: Health Check ===");
    let health_response = client
        .chat_completion(ChatRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: vec![Message::user("test")],
            max_tokens: Some(1),
            temperature: Some(0.0),
            ..Default::default()
        })
        .await;

    match health_response {
        Ok(_) => println!("Gateway is healthy"),
        Err(e) => println!("Gateway health check failed: {e:?}"),
    }

    Ok(())
}
