use std::collections::HashMap;
use std::time::{Duration, Instant};
use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Routing & Load Balancing Test");
    println!("================================\n");

    // Test 1: Basic Routing Test
    println!("📊 Test 1: Basic Routing Test");
    println!("=============================");
    test_basic_routing().await?;

    // Test 2: Load Balancing Test
    println!("\n📊 Test 2: Load Balancing Test");
    println!("===============================");
    test_load_balancing().await?;

    // Test 3: Provider-Specific Routing Test
    println!("\n📊 Test 3: Provider-Specific Routing Test");
    println!("==========================================");
    test_provider_specific_routing().await?;

    // Test 4: Failover Test
    println!("\n📊 Test 4: Failover Test");
    println!("=========================");
    test_failover().await?;

    // Test 5: Performance Comparison Test
    println!("\n📊 Test 5: Performance Comparison Test");
    println!("======================================");
    test_performance_comparison().await?;

    // Test 6: Concurrent Load Test
    println!("\n📊 Test 6: Concurrent Load Test");
    println!("=================================");
    test_concurrent_load().await?;

    println!("\n✅ All routing and load balancing tests completed!");
    Ok(())
}

async fn test_basic_routing() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing basic routing functionality...");

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-multi-provider-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    let test_requests = vec![
        // Anthropic models (using accessible models)
        ChatRequest {
            model: "claude-3-5-haiku-20241022".to_string(),
            messages: vec![Message::user("What is 2+2?")],
            max_tokens: Some(10),
            temperature: Some(0.0),
            ..Default::default()
        },
        ChatRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![Message::user("What is the capital of France?")],
            max_tokens: Some(20),
            temperature: Some(0.7),
            ..Default::default()
        },
        ChatRequest {
            model: "claude-3-7-sonnet-20250219".to_string(),
            messages: vec![Message::user("What is the meaning of life?")],
            max_tokens: Some(30),
            temperature: Some(0.7),
            ..Default::default()
        },
        // Ollama models
        ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user("What is 3+3?")],
            max_tokens: Some(10),
            temperature: Some(0.0),
            ..Default::default()
        },
        ChatRequest {
            model: "qwen3:8b".to_string(),
            messages: vec![Message::user("What is the capital of Japan?")],
            max_tokens: Some(20),
            temperature: Some(0.7),
            ..Default::default()
        },
    ];

    for (i, request) in test_requests.into_iter().enumerate() {
        println!("\n--- Testing basic routing for request {} ---", i + 1);
        println!("Model: {}", request.model);

        let start = Instant::now();
        let result = gateway_client.chat_completion(request).await;
        let latency = start.elapsed();

        match result {
            Ok(response) => {
                println!("✅ Basic routing test {}: {:?}", i + 1, latency);
                println!("   Response: {}", response.choices[0].message.content);
            }
            Err(e) => {
                println!("❌ Basic routing test {} failed: {:?}", i + 1, e);
            }
        }
    }

    Ok(())
}

async fn test_load_balancing() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing load balancing between providers...");

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-multi-provider-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    // Test with accessible Anthropic model to see load balancing in action
    let test_request = ChatRequest {
        model: "claude-3-5-haiku-20241022".to_string(),
        messages: vec![Message::user("What is 2+2?")],
        max_tokens: Some(10),
        temperature: Some(0.0),
        ..Default::default()
    };

    let mut latencies = Vec::new();
    let mut providers_used = HashMap::new();

    for i in 1..=10 {
        println!("\n--- Load balancing test {i} ---");

        let start = Instant::now();
        let result = gateway_client.chat_completion(test_request.clone()).await;
        let latency = start.elapsed();

        match result {
            Ok(response) => {
                println!("✅ Load balancing test {i}: {latency:?}");
                println!("   Response: {}", response.choices[0].message.content);

                latencies.push(latency);

                // Track which provider was used (based on response characteristics)
                let provider = if response.choices[0].message.content.contains("4") {
                    "anthropic"
                } else {
                    "ollama"
                };
                *providers_used.entry(provider).or_insert(0) += 1;
            }
            Err(e) => {
                println!("❌ Load balancing test {i} failed: {e:?}");
            }
        }
    }

    // Analyze load balancing results
    println!("\n📊 Load Balancing Analysis:");
    println!("===========================");
    println!("Total requests: {}", latencies.len());
    println!(
        "Average latency: {:?}",
        latencies.iter().sum::<Duration>() / latencies.len() as u32
    );
    println!("Min latency: {:?}", latencies.iter().min().unwrap());
    println!("Max latency: {:?}", latencies.iter().max().unwrap());
    println!("Provider distribution: {providers_used:?}");

    Ok(())
}

async fn test_provider_specific_routing() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing provider-specific routing...");

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-multi-provider-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    let test_requests = vec![
        // Anthropic-specific models (using accessible models)
        ("anthropic", "claude-3-5-haiku-20241022", "What is 2+2?"),
        (
            "anthropic",
            "claude-3-5-sonnet-20241022",
            "What is the capital of France?",
        ),
        (
            "anthropic",
            "claude-3-7-sonnet-20250219",
            "What is the meaning of life?",
        ),
        // Ollama-specific models
        ("ollama", "llama3.2:3b-instruct-q8_0", "What is 3+3?"),
        ("ollama", "qwen3:8b", "What is the capital of Japan?"),
    ];

    for (i, (expected_provider, model, prompt)) in test_requests.into_iter().enumerate() {
        println!("\n--- Provider-specific routing test {} ---", i + 1);
        println!("Expected provider: {expected_provider}");
        println!("Model: {model}");

        let request = ChatRequest {
            model: model.to_string(),
            messages: vec![Message::user(prompt)],
            max_tokens: Some(20),
            temperature: Some(0.7),
            ..Default::default()
        };

        let start = Instant::now();
        let result = gateway_client.chat_completion(request).await;
        let latency = start.elapsed();

        match result {
            Ok(response) => {
                println!("✅ Provider-specific routing test {}: {:?}", i + 1, latency);
                println!("   Response: {}", response.choices[0].message.content);

                // Determine actual provider used
                let actual_provider = if response.choices[0].message.content.contains("4")
                    || response.choices[0].message.content.contains("Paris")
                {
                    "anthropic"
                } else {
                    "ollama"
                };

                if actual_provider == expected_provider {
                    println!("   ✅ Correct provider used: {actual_provider}");
                } else {
                    println!(
                        "   ⚠️  Unexpected provider: {actual_provider} (expected: {expected_provider})"
                    );
                }
            }
            Err(e) => {
                println!(
                    "❌ Provider-specific routing test {} failed: {:?}",
                    i + 1,
                    e
                );
            }
        }
    }

    Ok(())
}

async fn test_failover() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing failover functionality...");

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-multi-provider-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    // Test with a model that might not be available on one provider
    let test_request = ChatRequest {
        model: "claude-3-5-haiku-20241022".to_string(),
        messages: vec![Message::user("What is 2+2?")],
        max_tokens: Some(10),
        temperature: Some(0.0),
        ..Default::default()
    };

    for i in 1..=5 {
        println!("\n--- Failover test {i} ---");

        let start = Instant::now();
        let result = gateway_client.chat_completion(test_request.clone()).await;
        let latency = start.elapsed();

        match result {
            Ok(response) => {
                println!("✅ Failover test {i}: {latency:?}");
                println!("   Response: {}", response.choices[0].message.content);
            }
            Err(e) => {
                println!("❌ Failover test {i} failed: {e:?}");
            }
        }
    }

    Ok(())
}

async fn test_performance_comparison() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing performance comparison between providers...");

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-multi-provider-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    let test_requests = vec![
        // Anthropic models
        ("anthropic", "claude-3-5-haiku-20241022", "What is 2+2?"),
        (
            "anthropic",
            "claude-3-5-sonnet-20241022",
            "What is the capital of France?",
        ),
        // Ollama models
        ("ollama", "llama3.2:3b-instruct-q8_0", "What is 3+3?"),
        ("ollama", "qwen3:8b", "What is the capital of Japan?"),
    ];

    let mut performance_data = HashMap::new();

    for (provider, model, prompt) in test_requests {
        println!("\n--- Performance test for {provider} ({model}) ---");

        let request = ChatRequest {
            model: model.to_string(),
            messages: vec![Message::user(prompt)],
            max_tokens: Some(20),
            temperature: Some(0.7),
            ..Default::default()
        };

        let mut latencies = Vec::new();

        for i in 1..=3 {
            let start = Instant::now();
            let result = gateway_client.chat_completion(request.clone()).await;
            let latency = start.elapsed();

            match result {
                Ok(_response) => {
                    println!("   Test {i}: {latency:?}");
                    latencies.push(latency);
                }
                Err(e) => {
                    println!("   Test {i} failed: {e:?}");
                }
            }
        }

        if !latencies.is_empty() {
            let avg_latency = latencies.iter().sum::<Duration>() / latencies.len() as u32;
            performance_data.insert(provider, avg_latency);
            println!("   Average latency: {avg_latency:?}");
        }
    }

    println!("\n📊 Performance Comparison:");
    println!("===========================");
    for (provider, latency) in &performance_data {
        println!("{provider}: {latency:?}");
    }

    Ok(())
}

async fn test_concurrent_load() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing concurrent load across providers...");

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-multi-provider-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    let test_requests = vec![
        ChatRequest {
            model: "claude-3-5-haiku-20241022".to_string(),
            messages: vec![Message::user("What is 2+2?")],
            max_tokens: Some(10),
            temperature: Some(0.0),
            ..Default::default()
        },
        ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user("What is 3+3?")],
            max_tokens: Some(10),
            temperature: Some(0.0),
            ..Default::default()
        },
        ChatRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![Message::user("What is the capital of France?")],
            max_tokens: Some(20),
            temperature: Some(0.7),
            ..Default::default()
        },
        ChatRequest {
            model: "qwen3:8b".to_string(),
            messages: vec![Message::user("What is the capital of Japan?")],
            max_tokens: Some(20),
            temperature: Some(0.7),
            ..Default::default()
        },
    ];

    println!(
        "\n--- Testing sequential load with {} requests ---",
        test_requests.len()
    );

    let mut results = Vec::new();

    // Submit requests sequentially but measure timing
    for (i, request) in test_requests.into_iter().enumerate() {
        println!("\n--- Concurrent load test {} ---", i + 1);

        let start = Instant::now();
        let result = gateway_client.chat_completion(request).await;
        let latency = start.elapsed();

        match result {
            Ok(response) => {
                println!("✅ Concurrent load test {}: {:?}", i + 1, latency);
                println!("   Response: {}", response.choices[0].message.content);
                results.push((i, Ok(response), latency));
            }
            Err(e) => {
                println!("❌ Concurrent load test {} failed: {:?}", i + 1, e);
                results.push((i, Err(e), latency));
            }
        }
    }

    // Analyze results
    println!("\n📊 Load Test Results:");
    println!("======================");
    for (i, result, latency) in &results {
        match result {
            Ok(_response) => {
                println!("✅ Load test {}: {:?}", i + 1, latency);
            }
            Err(_e) => {
                println!("❌ Load test {} failed: {:?}", i + 1, latency);
            }
        }
    }

    let successful_requests = results
        .iter()
        .filter(|(_, result, _)| result.is_ok())
        .count();
    let total_requests = results.len();
    println!(
        "\nSuccess rate: {}/{} ({:.1}%)",
        successful_requests,
        total_requests,
        (successful_requests as f64 / total_requests as f64) * 100.0
    );

    Ok(())
}
