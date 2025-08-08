use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("=== Testing Ultrafast Gateway Standalone Mode with Ollama ===");

    // Create standalone client with Ollama
    let client = UltrafastClient::standalone()
        .with_ollama("http://localhost:11434")
        .build()?;

    println!("✓ Client created successfully");

    // Test with different models
    let models = vec!["llama3.2:3b-instruct-q8_0", "qwen3:8b", "gemma3:4b"];

    for model in models {
        println!("\n--- Testing model: {model} ---");

        let response = client
            .chat_completion(ChatRequest {
                model: model.to_string(),
                messages: vec![Message::user(
                    "What is the capital of France? Answer in one sentence.",
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
                println!("✗ Error with model {model}: {e:?}");
            }
        }
    }

    // Test streaming
    println!("\n--- Testing streaming with llama3.2:3b-instruct-q8_0 ---");
    let mut stream = client
        .stream_chat_completion(ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user("Write a haiku about programming")],
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

    // Test embeddings
    println!("\n--- Testing embeddings ---");
    let embedding_response = client
        .embedding(ultrafast_models_sdk::EmbeddingRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            input: ultrafast_models_sdk::models::EmbeddingInput::String(
                "This is a test sentence for embeddings.".to_string(),
            ),
            encoding_format: None,
            dimensions: None,
            user: None,
        })
        .await;

    match embedding_response {
        Ok(resp) => {
            println!(
                "✓ Embeddings successful! Dimensions: {}",
                resp.data[0].embedding.len()
            );
        }
        Err(e) => {
            println!("✗ Embedding error: {e:?}");
        }
    }

    println!("\n=== Test completed ===");
    Ok(())
}
