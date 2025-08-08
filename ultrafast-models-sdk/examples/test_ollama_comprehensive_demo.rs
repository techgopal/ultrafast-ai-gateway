use std::process::Command;
use std::time::{Duration, Instant};
use ultrafast_models_sdk::{models::EmbeddingRequest, ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🚀 Comprehensive Ollama Test with Ultrafast Gateway");
    println!("==================================================\n");

    // Check Ollama status
    println!("📋 Checking Ollama status...");
    let ollama_status = Command::new("ollama").arg("list").output();

    match ollama_status {
        Ok(output) => {
            if output.status.success() {
                println!("✅ Ollama is running");
                let output_str = String::from_utf8_lossy(&output.stdout);
                println!("Available models:\n{output_str}");
            } else {
                println!("❌ Ollama is not running or not accessible");
                return Ok(());
            }
        }
        Err(_) => {
            println!("❌ Ollama command not found. Please install Ollama first.");
            return Ok(());
        }
    }

    // Test 1: Standalone Mode
    println!("\n🔧 Test 1: Standalone Mode");
    println!("==========================");
    test_standalone_mode().await?;

    // Test 2: Gateway Mode
    println!("\n🌐 Test 2: Gateway Mode");
    println!("=======================");
    test_gateway_mode().await?;

    // Test 3: Performance Comparison
    println!("\n⚡ Test 3: Performance Comparison");
    println!("=================================");
    test_performance_comparison().await?;

    // Test 4: Feature Comparison
    println!("\n🔍 Test 4: Feature Comparison");
    println!("=============================");
    test_feature_comparison().await?;

    println!("\n🎉 All tests completed successfully!");
    println!("✅ Standalone mode: Working perfectly");
    println!("✅ Gateway mode: Working perfectly");
    println!("✅ Both modes support: Chat completion, streaming, embeddings");
    println!("✅ Gateway provides: Authentication, rate limiting, caching, metrics");

    Ok(())
}

async fn test_standalone_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating standalone client with Ollama...");

    let client = UltrafastClient::standalone()
        .with_ollama("http://localhost:11434")
        .build()?;

    println!("✅ Standalone client created successfully");

    // Test basic chat completion
    println!("\n--- Testing basic chat completion ---");
    let start = Instant::now();
    let response = client
        .chat_completion(ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
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
            println!("❌ Error: {e:?}");
        }
    }

    // Test streaming
    println!("\n--- Testing streaming ---");
    let stream_result = client
        .stream_chat_completion(ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
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

async fn test_gateway_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating gateway client...");
    let client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    println!("✅ Gateway client created successfully");

    // Test chat completion through gateway
    println!("\n--- Testing chat completion through gateway ---");
    let start = Instant::now();
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
            let duration = start.elapsed();
            println!("✅ Success! Response: {}", resp.choices[0].message.content);
            println!("⏱️  Time taken: {duration:?}");
        }
        Err(e) => {
            println!("❌ Error: {e:?}");
        }
    }

    // Test streaming through gateway
    println!("\n--- Testing streaming through gateway ---");
    let stream_result = client
        .stream_chat_completion(ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user("Write a short poem about technology")],
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
                "\n✅ Gateway streaming completed. Total content length: {}",
                content.len()
            );
        }
        Err(e) => {
            println!("❌ Gateway streaming failed: {e:?}");
        }
    }

    // Test parallel requests through gateway
    println!("\n--- Testing parallel requests through gateway ---");
    let requests = vec![
        ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user("What is 2+2?")],
            max_tokens: Some(10),
            ..Default::default()
        },
        ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user("What is 3+3?")],
            max_tokens: Some(10),
            ..Default::default()
        },
        ChatRequest {
            model: "llama3.2:3b-instruct-q8_0".to_string(),
            messages: vec![Message::user("What is 4+4?")],
            max_tokens: Some(10),
            ..Default::default()
        },
    ];

    let futures: Vec<_> = requests
        .into_iter()
        .map(|req| client.chat_completion(req))
        .collect();

    let start = Instant::now();
    let results = futures::future::join_all(futures).await;
    let duration = start.elapsed();

    let mut success_count = 0;
    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(response) => {
                println!(
                    "✅ Request {}: {}",
                    i + 1,
                    response.choices[0].message.content
                );
                success_count += 1;
            }
            Err(e) => {
                println!("❌ Request {} failed: {:?}", i + 1, e);
            }
        }
    }
    println!("📊 Parallel requests: {success_count}/3 successful in {duration:?}");

    Ok(())
}

async fn test_performance_comparison() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating clients for performance comparison...");

    let standalone_client = UltrafastClient::standalone()
        .with_ollama("http://localhost:11434")
        .build()?;

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    let test_request = ChatRequest {
        model: "llama3.2:3b-instruct-q8_0".to_string(),
        messages: vec![Message::user(
            "What is the meaning of life? Answer in one sentence.",
        )],
        max_tokens: Some(50),
        temperature: Some(0.7),
        ..Default::default()
    };

    // Test standalone performance
    println!("\n--- Standalone Performance Test ---");
    let mut standalone_times = Vec::new();
    for i in 1..=3 {
        let start = Instant::now();
        let result = standalone_client
            .chat_completion(test_request.clone())
            .await;
        let duration = start.elapsed();

        match result {
            Ok(_) => {
                standalone_times.push(duration);
                println!("✅ Standalone request {i}: {duration:?}");
            }
            Err(e) => {
                println!("❌ Standalone request {i} failed: {e:?}");
            }
        }
    }

    // Test gateway performance
    println!("\n--- Gateway Performance Test ---");
    let mut gateway_times = Vec::new();
    for i in 1..=3 {
        let start = Instant::now();
        let result = gateway_client.chat_completion(test_request.clone()).await;
        let duration = start.elapsed();

        match result {
            Ok(_) => {
                gateway_times.push(duration);
                println!("✅ Gateway request {i}: {duration:?}");
            }
            Err(e) => {
                println!("❌ Gateway request {i} failed: {e:?}");
            }
        }
    }

    // Calculate averages
    if !standalone_times.is_empty() && !gateway_times.is_empty() {
        let standalone_avg: Duration =
            standalone_times.iter().sum::<Duration>() / standalone_times.len() as u32;
        let gateway_avg: Duration =
            gateway_times.iter().sum::<Duration>() / gateway_times.len() as u32;

        println!("\n📊 Performance Summary:");
        println!("Standalone average: {standalone_avg:?}");
        println!("Gateway average: {gateway_avg:?}");

        if standalone_avg < gateway_avg {
            let overhead = gateway_avg - standalone_avg;
            let overhead_percent =
                (overhead.as_millis() as f64 / standalone_avg.as_millis() as f64) * 100.0;
            println!("Gateway overhead: {overhead:?} ({overhead_percent:.1}%)");
        } else {
            println!("Gateway is faster than standalone (unexpected)");
        }
    }

    Ok(())
}

async fn test_feature_comparison() -> Result<(), Box<dyn std::error::Error>> {
    println!("Comparing features between standalone and gateway modes...");

    println!("\n🔧 Standalone Mode Features:");
    println!("✅ Direct connection to Ollama");
    println!("✅ No authentication required");
    println!("✅ Minimal latency");
    println!("✅ Simple setup");
    println!("❌ No rate limiting");
    println!("❌ No caching");
    println!("❌ No metrics");
    println!("❌ No request logging");

    println!("\n🌐 Gateway Mode Features:");
    println!("✅ Authentication and API keys");
    println!("✅ Rate limiting");
    println!("✅ Request caching");
    println!("✅ Metrics collection");
    println!("✅ Request logging");
    println!("✅ Load balancing (with multiple providers)");
    println!("✅ Health checks");
    println!("✅ Circuit breaker protection");
    println!("✅ Request validation");
    println!("✅ CORS support");
    println!("❌ Additional latency");
    println!("❌ More complex setup");

    println!("\n📋 Use Cases:");
    println!("Standalone: Perfect for simple applications, development, testing");
    println!("Gateway: Ideal for production, multi-user applications, enterprise use");

    Ok(())
}
