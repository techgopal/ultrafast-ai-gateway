use futures::StreamExt;
use ultrafast_models_sdk::{
    models::{ChatRequest, Message},
    UltrafastClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Testing Gateway Routing with UltrafastClient SDK ===");
    println!();

    // Create UltrafastClient in gateway mode
    let client = UltrafastClient::gateway("http://localhost:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .build()?;

    println!("✅ Gateway client created successfully");
    println!();

    // Test different models through gateway
    let models = vec!["llama3.2:3b-instruct-q8_0", "qwen3:8b", "gemma3:4b"];

    for model in models {
        println!("🔍 Testing model: {model}");

        let request = ChatRequest {
            model: model.to_string(),
            messages: vec![Message::user(format!("Test routing with model: {model}"))],
            max_tokens: Some(50),
            temperature: Some(0.7),
            stream: Some(false),
            tools: None,
            tool_choice: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            user: None,
        };

        match client.chat_completion(request).await {
            Ok(response) => {
                let content = &response.choices[0].message.content;
                println!("  ✅ Gateway routing successful: {content}");
            }
            Err(e) => {
                println!("  ❌ Gateway routing failed: {e:?}");
            }
        }
        println!();
    }

    // Test streaming with gateway
    println!("🔍 Testing streaming with gateway...");
    let streaming_request = ChatRequest {
        model: "llama3.2:3b-instruct-q8_0".to_string(),
        messages: vec![Message::user("Write a haiku about programming".to_string())],
        max_tokens: Some(100),
        temperature: Some(0.8),
        stream: Some(true),
        tools: None,
        tool_choice: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        user: None,
    };

    match client.stream_chat_completion(streaming_request).await {
        Ok(mut stream) => {
            println!("  ✅ Streaming started successfully");
            let mut full_response = String::new();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if let Some(content) = chunk.choices[0].delta.content.as_ref() {
                            print!("{content}");
                            full_response.push_str(content);
                        }
                    }
                    Err(e) => {
                        println!("\n  ❌ Streaming error: {e:?}");
                        break;
                    }
                }
            }
            println!("\n  ✅ Streaming completed: {full_response}");
        }
        Err(e) => {
            println!("  ❌ Streaming failed: {e:?}");
        }
    }

    println!();
    println!("🎉 Gateway routing test completed successfully!");
    Ok(())
}
