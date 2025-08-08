use std::time::Instant;
use ultrafast_models_sdk::{models::EmbeddingRequest, ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🚀 Testing Ollama with Ultrafast Gateway - Standalone Mode");
    println!("========================================================\n");

    // Test 1: Standalone Mode with Ollama
    println!("🔧 Test 1: Standalone Mode");
    println!("==========================");
    test_standalone_mode().await?;

    println!("\n🎉 Standalone mode test completed!");
    Ok(())
}

async fn test_standalone_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating standalone client with Ollama...");

    let client = UltrafastClient::standalone()
        .with_ollama("http://localhost:11434")
        .build()?;

    println!("✅ Standalone client created successfully");

    // Test different models
    let models = vec!["llama3.2:3b-instruct-q8_0", "qwen3:8b", "gemma3:4b"];

    for model in models {
        println!("\n--- Testing model: {model} ---");

        // Test basic chat completion
        let start = Instant::now();
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
                let duration = start.elapsed();
                println!("✅ Success! Response: {}", resp.choices[0].message.content);
                println!("⏱️  Time taken: {duration:?}");
            }
            Err(e) => {
                println!("❌ Error with model {model}: {e:?}");
            }
        }

        // Test streaming
        println!("Testing streaming with {model}...");
        let stream_result = client
            .stream_chat_completion(ChatRequest {
                model: model.to_string(),
                messages: vec![Message::user("Write a haiku about programming")],
                max_tokens: Some(100),
                temperature: Some(0.8),
                stream: Some(true),
                ..Default::default()
            })
            .await;

        match stream_result {
            Ok(mut stream) => {
                print!("Streaming response: ");
                use futures::StreamExt;
                let mut content = String::new();
                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            if let Some(chunk_content) = &chunk.choices[0].delta.content {
                                print!("{chunk_content}");
                                content.push_str(chunk_content);
                            }
                        }
                        Err(e) => {
                            println!("\n❌ Error in stream: {e:?}");
                            break;
                        }
                    }
                }
                println!(
                    "\n✅ Streaming completed. Total content length: {}",
                    content.len()
                );
            }
            Err(e) => {
                println!("❌ Streaming failed: {e:?}");
            }
        }
    }

    // Test embeddings
    println!("\n--- Testing embeddings ---");
    let embedding_response = client
        .embedding(EmbeddingRequest {
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
                "✅ Embeddings successful! Dimensions: {}",
                resp.data[0].embedding.len()
            );
        }
        Err(e) => {
            println!("❌ Embedding error: {e:?}");
        }
    }

    Ok(())
}
