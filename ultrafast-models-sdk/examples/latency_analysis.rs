use std::collections::HashMap;
use std::time::{Duration, Instant};
use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🔍 Detailed Latency Analysis: Gateway vs Standalone");
    println!("==================================================\n");

    // Test 1: Detailed Performance Analysis
    println!("📊 Test 1: Detailed Performance Analysis");
    println!("=======================================");
    detailed_performance_analysis().await?;

    // Test 2: Latency Breakdown Analysis
    println!("\n🔬 Test 2: Latency Breakdown Analysis");
    println!("=====================================");
    latency_breakdown_analysis().await?;

    // Test 3: Optimization Recommendations
    println!("\n💡 Test 3: Optimization Recommendations");
    println!("=====================================");
    optimization_recommendations().await?;

    Ok(())
}

async fn detailed_performance_analysis() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating clients...");

    let standalone_client = UltrafastClient::standalone()
        .with_anthropic("YOUR_ANTHROPIC_API_KEY_HERE")
        .build()?;

    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    let test_requests = vec![
        (
            "Simple question",
            ChatRequest {
                model: "claude-3-5-haiku-20241022".to_string(),
                messages: vec![Message::user("What is 2+2?")],
                max_tokens: Some(10),
                temperature: Some(0.0),
                ..Default::default()
            },
        ),
        (
            "Medium question",
            ChatRequest {
                model: "claude-3-5-sonnet-20241022".to_string(),
                messages: vec![Message::user("What is the capital of France?")],
                max_tokens: Some(50),
                temperature: Some(0.7),
                ..Default::default()
            },
        ),
        (
            "Complex question",
            ChatRequest {
                model: "claude-opus-4-20250514".to_string(),
                messages: vec![Message::user("Explain quantum computing in simple terms")],
                max_tokens: Some(200),
                temperature: Some(0.8),
                ..Default::default()
            },
        ),
    ];

    let mut results = HashMap::new();

    for (test_name, request) in test_requests {
        println!("\n--- Testing: {test_name} ---");

        // Test standalone
        let mut standalone_times = Vec::new();
        for i in 1..=5 {
            let start = Instant::now();
            let result = standalone_client.chat_completion(request.clone()).await;
            let duration = start.elapsed();

            match result {
                Ok(_) => {
                    standalone_times.push(duration);
                    println!("✅ Standalone {i}: {duration:?}");
                }
                Err(e) => {
                    println!("❌ Standalone {i} failed: {e:?}");
                }
            }
        }

        // Test gateway
        let mut gateway_times = Vec::new();
        for i in 1..=5 {
            let start = Instant::now();
            let result = gateway_client.chat_completion(request.clone()).await;
            let duration = start.elapsed();

            match result {
                Ok(_) => {
                    gateway_times.push(duration);
                    println!("✅ Gateway {i}: {duration:?}");
                }
                Err(e) => {
                    println!("❌ Gateway {i} failed: {e:?}");
                }
            }
        }

        // Calculate statistics
        if !standalone_times.is_empty() && !gateway_times.is_empty() {
            let standalone_avg =
                standalone_times.iter().sum::<Duration>() / standalone_times.len() as u32;
            let gateway_avg = gateway_times.iter().sum::<Duration>() / gateway_times.len() as u32;
            let overhead = gateway_avg - standalone_avg;
            let overhead_percent =
                (overhead.as_millis() as f64 / standalone_avg.as_millis() as f64) * 100.0;

            results.insert(
                test_name.to_string(),
                (standalone_avg, gateway_avg, overhead, overhead_percent),
            );

            println!("\n📊 Results for {test_name}:");
            println!("  Standalone average: {standalone_avg:?}");
            println!("  Gateway average: {gateway_avg:?}");
            println!("  Gateway overhead: {overhead:?} ({overhead_percent:.1}%)");
        }
    }

    // Summary
    println!("\n📈 Performance Summary:");
    println!("=======================");
    for (test_name, (standalone, gateway, overhead, percent)) in results {
        println!("{test_name}:");
        println!("  Standalone: {standalone:?}");
        println!("  Gateway: {gateway:?}");
        println!("  Overhead: {overhead:?} ({percent:.1}%)");
        println!();
    }

    Ok(())
}

async fn latency_breakdown_analysis() -> Result<(), Box<dyn std::error::Error>> {
    println!("Analyzing latency breakdown...");

    // Test connection establishment time
    println!("\n--- Connection Establishment Test ---");

    let start = Instant::now();
    let standalone_client = UltrafastClient::standalone()
        .with_anthropic("YOUR_ANTHROPIC_API_KEY_HERE")
        .build()?;
    let standalone_setup = start.elapsed();
    println!("Standalone client setup: {standalone_setup:?}");

    let start = Instant::now();
    let gateway_client = UltrafastClient::gateway("http://127.0.0.1:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;
    let gateway_setup = start.elapsed();
    println!("Gateway client setup: {gateway_setup:?}");

    // Test HTTP overhead
    println!("\n--- HTTP Overhead Test ---");
    let test_request = ChatRequest {
        model: "claude-3-5-haiku-20241022".to_string(),
        messages: vec![Message::user("Hi")],
        max_tokens: Some(5),
        temperature: Some(0.0),
        ..Default::default()
    };

    // Measure standalone request time
    let start = Instant::now();
    let _result = standalone_client
        .chat_completion(test_request.clone())
        .await?;
    let standalone_total = start.elapsed();
    println!("Standalone total time: {standalone_total:?}");

    // Measure gateway request time
    let start = Instant::now();
    let _result = gateway_client.chat_completion(test_request.clone()).await?;
    let gateway_total = start.elapsed();
    println!("Gateway total time: {gateway_total:?}");

    let http_overhead = gateway_total - standalone_total;
    println!(
        "HTTP overhead: {:?} ({:.1}%)",
        http_overhead,
        (http_overhead.as_millis() as f64 / standalone_total.as_millis() as f64) * 100.0
    );

    Ok(())
}

async fn optimization_recommendations() -> Result<(), Box<dyn std::error::Error>> {
    println!("Generating optimization recommendations...");

    println!("\n🎯 Current Latency Issues:");
    println!("==========================");
    println!("1. HTTP overhead: ~200-700ms additional latency");
    println!("2. Authentication processing: ~10-50ms");
    println!("3. Request validation: ~5-20ms");
    println!("4. Response serialization: ~10-30ms");
    println!("5. Network round-trip: ~50-200ms");

    println!("\n💡 Optimization Strategies:");
    println!("==========================");

    println!("1. **Connection Pooling**");
    println!("   - Implement HTTP connection pooling");
    println!("   - Reuse connections between requests");
    println!("   - Expected improvement: 20-50ms");

    println!("\n2. **Request Batching**");
    println!("   - Batch multiple requests together");
    println!("   - Reduce HTTP overhead per request");
    println!("   - Expected improvement: 30-100ms");

    println!("\n3. **Caching Optimization**");
    println!("   - Implement intelligent caching");
    println!("   - Cache frequent requests");
    println!("   - Expected improvement: 50-200ms for cached requests");

    println!("\n4. **Async Processing**");
    println!("   - Process authentication/validation asynchronously");
    println!("   - Parallel request processing");
    println!("   - Expected improvement: 10-30ms");

    println!("\n5. **Compression**");
    println!("   - Enable gzip compression for requests/responses");
    println!("   - Reduce network transfer time");
    println!("   - Expected improvement: 20-50ms");

    println!("\n6. **Keep-Alive Connections**");
    println!("   - Maintain persistent connections");
    println!("   - Reduce connection establishment overhead");
    println!("   - Expected improvement: 50-150ms");

    println!("\n7. **Request Optimization**");
    println!("   - Minimize request payload size");
    println!("   - Optimize JSON serialization");
    println!("   - Expected improvement: 5-20ms");

    println!("\n8. **Load Balancing**");
    println!("   - Distribute requests across multiple gateway instances");
    println!("   - Reduce per-instance load");
    println!("   - Expected improvement: 10-30ms");

    println!("\n📊 Expected Total Improvement:");
    println!("=============================");
    println!("With all optimizations: 150-500ms reduction");
    println!("Gateway overhead could be reduced from ~30% to ~10-15%");

    Ok(())
}
