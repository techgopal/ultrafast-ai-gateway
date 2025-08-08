use serde_json::json;
use std::time::{Duration, Instant};
use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Phase 3 & 4 Optimization Test");
    println!("================================\n");

    // Test 1: Phase 3 - Async Processing Test
    println!("📊 Test 1: Phase 3 - Async Processing");
    println!("=====================================");
    test_async_processing().await?;

    // Test 2: Phase 4 - JSON Optimization Test
    println!("\n📊 Test 2: Phase 4 - JSON Optimization");
    println!("=====================================");
    test_json_optimization().await?;

    // Test 3: Phase 4 - Request Optimization Test
    println!("\n📊 Test 3: Phase 4 - Request Optimization");
    println!("==========================================");
    test_request_optimization().await?;

    // Test 4: Combined Optimization Test
    println!("\n📊 Test 4: Combined Optimization Test");
    println!("=====================================");
    test_combined_optimizations().await?;

    println!("\n✅ All Phase 3 & 4 optimization tests completed!");
    Ok(())
}

async fn test_async_processing() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing async processing optimizations...");

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
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
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![Message::user("What is the capital of France?")],
            max_tokens: Some(50),
            temperature: Some(0.7),
            ..Default::default()
        },
    ];

    for (i, request) in test_requests.into_iter().enumerate() {
        println!("\n--- Testing async processing for request {} ---", i + 1);

        let start = Instant::now();
        let result = gateway_client.chat_completion(request).await;
        let latency = start.elapsed();

        match result {
            Ok(_) => {
                println!("✅ Async processing test {}: {:?}", i + 1, latency);
            }
            Err(e) => {
                println!("❌ Async processing test {} failed: {:?}", i + 1, e);
            }
        }
    }

    Ok(())
}

async fn test_json_optimization() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing JSON optimization features...");

    // Test JSON optimization with sample data
    let sample_request = json!({
        "model": "claude-3-5-haiku-20241022",
        "messages": [
            {"role": "user", "content": "Hello, this is a test message with some content to test JSON optimization"}
        ],
        "max_tokens": 100,
        "temperature": 0.7,
        "top_p": 1.0,
        "frequency_penalty": 0.0,
        "presence_penalty": 0.0,
        "unnecessary_field": null,
        "another_null_field": null,
        "nested_object": {
            "key1": "value1",
            "key2": null,
            "key3": "value3"
        }
    });

    let original_size = serde_json::to_string(&sample_request)?.len();
    println!("Original JSON size: {original_size} bytes");

    // Test payload optimization (simulated)
    let optimized_size = original_size * 85 / 100; // Simulate 15% reduction
    println!("Optimized JSON size: {optimized_size} bytes (simulated)");
    println!(
        "Size reduction: {:.1}%",
        ((original_size - optimized_size) as f64 / original_size as f64) * 100.0
    );

    // Test compression (simulated)
    let compressed_size = original_size * 75 / 100; // Simulate 25% reduction
    println!("Compressed JSON size: {compressed_size} bytes (simulated)");
    println!(
        "Compression reduction: {:.1}%",
        ((original_size - compressed_size) as f64 / original_size as f64) * 100.0
    );

    // Test decompression (simulated)
    let decompressed_size = original_size;
    println!("Decompressed JSON size: {decompressed_size} bytes (simulated)");

    Ok(())
}

async fn test_request_optimization() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing request optimization features...");

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    let test_requests = vec![
        // Request with unnecessary fields
        ChatRequest {
            model: "claude-3-5-haiku-20241022".to_string(),
            messages: vec![Message::user("What is 2+2?")],
            max_tokens: Some(10),
            temperature: Some(0.0),
            top_p: Some(1.0),
            frequency_penalty: Some(0.0),
            presence_penalty: Some(0.0),
            ..Default::default()
        },
        // Request with minimal fields
        ChatRequest {
            model: "claude-3-5-haiku-20241022".to_string(),
            messages: vec![Message::user("What is 3+3?")],
            max_tokens: Some(10),
            ..Default::default()
        },
    ];

    for (i, request) in test_requests.into_iter().enumerate() {
        println!(
            "\n--- Testing request optimization for request {} ---",
            i + 1
        );

        let start = Instant::now();
        let result = gateway_client.chat_completion(request).await;
        let latency = start.elapsed();

        match result {
            Ok(_) => {
                println!("✅ Request optimization test {}: {:?}", i + 1, latency);
            }
            Err(e) => {
                println!("❌ Request optimization test {} failed: {:?}", i + 1, e);
            }
        }
    }

    Ok(())
}

async fn test_combined_optimizations() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing combined Phase 3 & 4 optimizations...");

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    let test_request = ChatRequest {
        model: "claude-3-5-haiku-20241022".to_string(),
        messages: vec![
            Message::user("What is the meaning of life? Answer in one sentence."),
            Message::assistant("The meaning of life is to find purpose and fulfillment."),
            Message::user("Can you elaborate on that?"),
        ],
        max_tokens: Some(100),
        temperature: Some(0.7),
        top_p: Some(1.0),
        frequency_penalty: Some(0.0),
        presence_penalty: Some(0.0),
        ..Default::default()
    };

    println!("\n--- Testing combined optimizations ---");

    // Test multiple requests to see caching + optimization effects
    for i in 1..=5 {
        let start = Instant::now();
        let result = gateway_client.chat_completion(test_request.clone()).await;
        let latency = start.elapsed();

        match result {
            Ok(_) => {
                println!("✅ Combined optimization test {i}: {latency:?}");
            }
            Err(e) => {
                println!("❌ Combined optimization test {i} failed: {e:?}");
            }
        }
    }

    Ok(())
}
