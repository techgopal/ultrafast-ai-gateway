use std::collections::HashMap;
use std::time::{Duration, Instant};
use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Conditional Routing Strategy Test");
    println!("===================================");
    println!();

    // Test the gateway with Conditional routing
    test_conditional_routing().await?;

    println!("\n✅ All conditional routing tests completed!");
    Ok(())
}

async fn test_conditional_routing() -> Result<(), Box<dyn std::error::Error>> {
    println!("📊 Testing Conditional Routing Strategy");
    println!("=====================================");

    let client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-multi-provider-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    // Test cases for different models and expected providers
    let test_cases = vec![
        // Anthropic models (should route to anthropic provider)
        ("claude-3-5-haiku-20241022", "anthropic", "What is 2+2?"),
        (
            "claude-3-5-sonnet-20241022",
            "anthropic",
            "What is the capital of France?",
        ),
        (
            "claude-3-7-sonnet-20250219",
            "anthropic",
            "What is the meaning of life?",
        ),
        (
            "claude-sonnet-4-20250514",
            "anthropic",
            "Explain quantum computing",
        ),
        (
            "claude-opus-4-20250514",
            "anthropic",
            "Write a haiku about AI",
        ),
        // Ollama models (should route to ollama provider)
        ("llama3.2:3b-instruct-q8_0", "ollama", "What is 3+3?"),
        ("qwen3:8b", "ollama", "What is the capital of Japan?"),
        ("gemma3:4b", "ollama", "What is machine learning?"),
    ];

    let mut results = HashMap::new();
    let mut successful_routes = 0;
    let mut total_tests = 0;

    for (model, expected_provider, prompt) in test_cases {
        println!("\n--- Testing model: {model} (expected: {expected_provider}) ---");

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

                // Determine actual provider based on response characteristics
                let actual_provider = determine_provider_from_response(&resp);
                let route_correct = actual_provider == expected_provider;

                if route_correct {
                    successful_routes += 1;
                    println!("✅ Correct routing: {model} -> {actual_provider}");
                } else {
                    println!(
                        "❌ Incorrect routing: {model} -> {actual_provider} (expected: {expected_provider})"
                    );
                }

                results.insert(
                    model.to_string(),
                    (true, duration, route_correct, actual_provider),
                );
            }
            Err(e) => {
                println!("❌ Error: {e:?}");
                results.insert(
                    model.to_string(),
                    (false, duration, false, "unknown".to_string()),
                );
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

    println!("\n📋 Detailed Results:");
    for (model, (success, duration, route_correct, provider)) in results {
        let status = if success && route_correct {
            "✅"
        } else if success {
            "⚠️"
        } else {
            "❌"
        };
        println!(
            "{} {} -> {} ({}ms)",
            status,
            model,
            provider,
            duration.as_millis()
        );
    }

    Ok(())
}

fn determine_provider_from_response(response: &ultrafast_models_sdk::ChatResponse) -> String {
    // Check model name in response - this is the most reliable indicator
    if response.model.starts_with("claude") {
        "anthropic".to_string()
    } else if response.model.starts_with("llama")
        || response.model.starts_with("qwen")
        || response.model.starts_with("gemma")
    {
        "ollama".to_string()
    } else {
        // Fallback based on response content patterns
        let content = &response.choices[0].message.content;
        if content.contains("Claude") || content.contains("Anthropic") {
            "anthropic".to_string()
        } else {
            "ollama".to_string()
        }
    }
}
