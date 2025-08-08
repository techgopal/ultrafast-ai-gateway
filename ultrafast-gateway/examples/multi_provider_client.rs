use futures::StreamExt;
use ultrafast_models_sdk::{
    models::{EmbeddingInput, EmbeddingRequest},
    ChatRequest, Message, RoutingStrategy, UltrafastClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("=== Ultrafast Gateway Multi-Provider Example ===\n");

    // Example 1: Multiple providers with load balancing
    println!("1. Load Balancing with Multiple Providers");
    let client = UltrafastClient::standalone()
        .with_openai("sk-your-openai-key")
        .with_anthropic("sk-ant-your-anthropic-key")
        .with_cohere("your-cohere-key")
        .with_groq("your-groq-key")
        .with_mistral("your-mistral-key")
        .with_perplexity("your-perplexity-key")
        .with_routing_strategy(RoutingStrategy::LoadBalance {
            weights: vec![0.3, 0.2, 0.2, 0.1, 0.1, 0.1],
        })
        .build()?;

    let response = client
        .chat_completion(ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("Explain quantum computing in simple terms")],
            max_tokens: Some(200),
            temperature: Some(0.7),
            ..Default::default()
        })
        .await?;

    println!("Response: {}", response.choices[0].message.content);

    // Example 2: Ollama local provider
    println!("\n2. Ollama Local Provider");
    let ollama_client = UltrafastClient::standalone()
        .with_ollama("http://localhost:11434")
        .build()?;

    let ollama_response = ollama_client
        .chat_completion(ChatRequest {
            model: "llama2".to_string(),
            messages: vec![Message::user("What is machine learning?")],
            max_tokens: Some(150),
            temperature: Some(0.8),
            ..Default::default()
        })
        .await?;

    println!(
        "Ollama Response: {}",
        ollama_response.choices[0].message.content
    );

    // Example 3: Custom HTTP provider
    println!("\n3. Custom HTTP Provider");
    let _custom_client = UltrafastClient::standalone()
        .with_custom(
            "my-custom-provider",
            "custom-api-key",
            "http://localhost:8080",
        )
        .build()?;

    // Example 4: Fallback strategy
    println!("\n4. Fallback Strategy");
    let fallback_client = UltrafastClient::standalone()
        .with_openai("sk-your-openai-key")
        .with_anthropic("sk-ant-your-anthropic-key")
        .with_groq("your-groq-key")
        .with_routing_strategy(RoutingStrategy::Fallback)
        .build()?;

    let fallback_response = fallback_client
        .chat_completion(ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("Write a short poem about technology")],
            max_tokens: Some(100),
            temperature: Some(0.9),
            ..Default::default()
        })
        .await?;

    println!(
        "Fallback Response: {}",
        fallback_response.choices[0].message.content
    );

    // Example 5: Conditional routing
    println!("\n5. Conditional Routing");
    let conditional_client = UltrafastClient::standalone()
        .with_openai("sk-your-openai-key")
        .with_anthropic("sk-ant-your-anthropic-key")
        .with_mistral("your-mistral-key")
        .with_routing_strategy(RoutingStrategy::Conditional {
            rules: vec![
                ultrafast_models_sdk::routing::RoutingRule {
                    condition: ultrafast_models_sdk::routing::Condition::ModelName(
                        "gpt-4".to_string(),
                    ),
                    provider: "openai".to_string(),
                    weight: 1.0,
                },
                ultrafast_models_sdk::routing::RoutingRule {
                    condition: ultrafast_models_sdk::routing::Condition::ModelName(
                        "claude".to_string(),
                    ),
                    provider: "anthropic".to_string(),
                    weight: 1.0,
                },
                ultrafast_models_sdk::routing::RoutingRule {
                    condition: ultrafast_models_sdk::routing::Condition::ModelName(
                        "mistral".to_string(),
                    ),
                    provider: "mistral".to_string(),
                    weight: 1.0,
                },
            ],
        })
        .build()?;

    let conditional_response = conditional_client
        .chat_completion(ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("What is artificial intelligence?")],
            max_tokens: Some(150),
            temperature: Some(0.7),
            ..Default::default()
        })
        .await?;

    println!(
        "Conditional Response: {}",
        conditional_response.choices[0].message.content
    );

    // Example 6: Embeddings with different providers
    println!("\n6. Embeddings with Different Providers");

    // OpenAI embeddings
    let openai_embeddings = client
        .embedding(EmbeddingRequest {
            model: "text-embedding-ada-002".to_string(),
            input: EmbeddingInput::String("This is a test sentence for embeddings.".to_string()),
            ..Default::default()
        })
        .await?;

    println!(
        "OpenAI Embedding dimensions: {}",
        openai_embeddings.data[0].embedding.len()
    );

    // Mistral embeddings
    let mistral_embeddings = client
        .embedding(EmbeddingRequest {
            model: "mistral-embed".to_string(),
            input: EmbeddingInput::String("This is another test sentence.".to_string()),
            ..Default::default()
        })
        .await?;

    println!(
        "Mistral Embedding dimensions: {}",
        mistral_embeddings.data[0].embedding.len()
    );

    // Example 7: Streaming with multiple providers
    println!("\n7. Streaming with Multiple Providers");
    let mut stream = client
        .stream_chat_completion(ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user(
                "Write a story about a robot learning to paint",
            )],
            max_tokens: Some(200),
            temperature: Some(0.8),
            stream: Some(true),
            ..Default::default()
        })
        .await?;

    print!("Streaming response: ");
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                if let Some(content) = &chunk.choices[0].delta.content {
                    print!("{content}");
                }
            }
            Err(e) => {
                println!("\nError in stream: {e:?}");
                break;
            }
        }
    }
    println!();

    // Example 8: Error handling with provider failures
    println!("\n8. Error Handling with Provider Failures");
    let error_client = UltrafastClient::standalone()
        .with_openai("invalid-key")
        .with_anthropic("invalid-key")
        .with_routing_strategy(RoutingStrategy::Fallback)
        .build()?;

    match error_client
        .chat_completion(ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("This should fail")],
            max_tokens: Some(10),
            ..Default::default()
        })
        .await
    {
        Ok(response) => {
            println!(
                "Unexpected success: {}",
                response.choices[0].message.content
            );
        }
        Err(e) => {
            println!("Expected error: {e:?}");
        }
    }

    println!("\n=== Example completed successfully! ===");
    Ok(())
}
