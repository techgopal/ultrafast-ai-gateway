use std::time::{Duration, Instant};
use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Conditional Routing Strategy - Simple Test");
    println!("=============================================");
    println!();

    // Test the gateway with Conditional routing (Ollama models only)
    test_conditional_routing_ollama().await?;

    println!("\n✅ Conditional routing test completed!");
    Ok(())
}

async fn test_conditional_routing_ollama() -> Result<(), Box<dyn std::error::Error>> {
    println!("📊 Testing Conditional Routing Strategy (Ollama Models)");
    println!("=====================================================");

    let client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-multi-provider-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    // Test cases for Ollama models (should route to ollama provider)
    let test_cases = vec![
        ("llama3.2:3b-instruct-q8_0", "What is 3+3?"),
        ("qwen3:8b", "What is the capital of Japan?"),
        ("gemma3:4b", "What is machine learning?"),
    ];

    let mut successful_routes = 0;
    let mut total_tests = 0;

    for (model, prompt) in test_cases {
        println!("\n--- Testing model: {model} ---");

        let start = Instant::now();
        let response = client
            .chat_completion(ChatRequest {
                model: model.to_string(),
                messages: vec![Message::user(prompt)],
                max_tokens: Some(50),
                temperature: Some(0.7),
                ..Default::default()
            })
            .await;

        let duration = start.elapsed();
        total_tests += 1;

        match response {
            Ok(resp) => {
                println!("✅ Success! Response: {}", resp.choices[0].message.content);
                println!("⏱️  Time taken: {duration:?}");
                println!("📊 Model in response: {}", resp.model);

                // Check if the model in response matches the requested model
                let route_correct = resp.model == model;

                if route_correct {
                    successful_routes += 1;
                    println!("✅ Correct routing: {model} -> ollama");
                } else {
                    println!("❌ Incorrect routing: {} -> {}", model, resp.model);
                }
            }
            Err(e) => {
                println!("❌ Error: {e:?}");
            }
        }
    }

    // Print summary
    println!("\n📊 Conditional Routing Test Results");
    println!("==================================");
    println!("Total tests: {total_tests}");
    println!("Successful routes: {successful_routes}");
    println!(
        "Success rate: {:.1}%",
        (successful_routes as f64 / total_tests as f64) * 100.0
    );

    Ok(())
}
