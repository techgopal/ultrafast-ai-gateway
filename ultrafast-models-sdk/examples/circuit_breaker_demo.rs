use std::time::Duration;
use ultrafast_models_sdk::{ChatRequest, CircuitBreakerConfig, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("=== Circuit Breaker Demo ===");

    // Create client with circuit breaker enabled (default)
    let client = UltrafastClient::standalone()
        .with_ollama("http://localhost:11434")
        .build()?;

    println!("✓ Client created with circuit breakers enabled");

    // Test normal operation
    println!("\n--- Testing Normal Operation ---");
    let request = ChatRequest {
        model: "llama3.2:3b-instruct-q8_0".to_string(),
        messages: vec![Message::user("Hello!")],
        max_tokens: Some(10),
        temperature: Some(0.7),
        ..Default::default()
    };

    match client.chat_completion(request.clone()).await {
        Ok(response) => {
            println!("✓ Success: {}", response.choices[0].message.content);
        }
        Err(e) => {
            println!("✗ Error: {e:?}");
        }
    }

    // Check circuit breaker metrics
    println!("\n--- Circuit Breaker Metrics ---");
    let cb_metrics = client.get_circuit_breaker_metrics().await;
    for (provider_id, metrics) in cb_metrics {
        println!("Provider: {provider_id}");
        println!("  State: {:?}", metrics.state);
        println!("  Success Count: {}", metrics.success_count);
        println!("  Failure Count: {}", metrics.failure_count);
        if let Some(last_failure) = metrics.last_failure_time {
            println!("  Last Failure: {:?} ago", last_failure.elapsed());
        }
        if let Some(last_success) = metrics.last_success_time {
            println!("  Last Success: {:?} ago", last_success.elapsed());
        }
    }

    // Check provider health
    println!("\n--- Provider Health Status ---");
    let health_status = client.get_provider_health_status().await;
    for (provider_id, is_healthy) in health_status {
        println!(
            "Provider {}: {}",
            provider_id,
            if is_healthy { "Healthy" } else { "Unhealthy" }
        );
    }

    // Test with custom circuit breaker config
    println!("\n--- Testing with Custom Circuit Breaker Config ---");
    let custom_cb_config = CircuitBreakerConfig {
        failure_threshold: 2,
        recovery_timeout: Duration::from_secs(30),
        request_timeout: Duration::from_secs(10),
        half_open_max_calls: 1,
    };

    // Note: In a real application, you would configure this in the provider config
    println!("Custom circuit breaker config:");
    println!(
        "  Failure threshold: {}",
        custom_cb_config.failure_threshold
    );
    println!(
        "  Recovery timeout: {:?}",
        custom_cb_config.recovery_timeout
    );
    println!("  Request timeout: {:?}", custom_cb_config.request_timeout);
    println!(
        "  Half-open max calls: {}",
        custom_cb_config.half_open_max_calls
    );

    println!("\n=== Demo completed ===");
    Ok(())
}
