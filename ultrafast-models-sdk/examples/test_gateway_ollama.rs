use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("=== Testing Ultrafast Gateway Mode with Ollama ===");

    // Create gateway client
    let client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .build()?;

    println!("✓ Gateway client created successfully");

    // Test chat completion
    println!("\n--- Testing chat completion through gateway ---");
    let response = client
        .chat_completion(ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user(
                "What is the capital of Germany? Answer in one sentence.",
            )],
            max_tokens: Some(50),
            temperature: Some(0.7),
            ..Default::default()
        })
        .await;

    match response {
        Ok(resp) => {
            println!("✓ Success! Response: {}", resp.choices[0].message.content);
        }
        Err(e) => {
            println!("✗ Error: {e:?}");
        }
    }

    // Test streaming through gateway
    println!("\n--- Testing streaming through gateway ---");
    let mut stream = client
        .stream_chat_completion(ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user("Write a short poem about technology")],
            max_tokens: Some(100),
            temperature: Some(0.8),
            stream: Some(true),
            ..Default::default()
        })
        .await?;

    print!("Streaming response: ");
    use futures::StreamExt;
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                if let Some(content) = &chunk.choices[0].delta.content {
                    print!("{content}");
                }
            }
            Err(e) => {
                println!("\n✗ Error in stream: {e:?}");
                break;
            }
        }
    }
    println!();

    println!("\n=== Gateway mode test completed ===");
    Ok(())
}
