use futures::StreamExt;
use ultrafast_models_sdk::{
    models::{EmbeddingInput, EmbeddingRequest},
    ChatRequest, Message, RoutingStrategy, UltrafastClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Example 1: Simple standalone client with OpenAI
    println!("=== Example 1: Simple OpenAI Client ===");
    let client = UltrafastClient::standalone()
        .with_openai("sk-your-openai-key")
        .build()?;

    let response = client
        .chat_completion(ChatRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: vec![Message::user("Hello! What is the capital of France?")],
            max_tokens: Some(100),
            temperature: Some(0.7),
            ..Default::default()
        })
        .await?;

    println!("Response: {}", response.choices[0].message.content);

    // Example 2: Multiple providers with load balancing
    println!("\n=== Example 2: Multiple Providers with Load Balancing ===");
    let client = UltrafastClient::standalone()
        .with_openai("sk-your-openai-key")
        .with_anthropic("sk-ant-your-anthropic-key")
        .with_routing_strategy(RoutingStrategy::LoadBalance {
            weights: vec![0.6, 0.4],
        })
        .build()?;

    let response = client
        .chat_completion(ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("Explain quantum computing in simple terms")],
            max_tokens: Some(200),
            temperature: Some(0.8),
            ..Default::default()
        })
        .await?;

    println!("Response: {}", response.choices[0].message.content);

    // Example 3: Azure OpenAI with custom deployment
    println!("\n=== Example 3: Azure OpenAI ===");
    let client = UltrafastClient::standalone()
        .with_azure_openai("your-azure-api-key", "gpt-4-deployment")
        .build()?;

    let response = client
        .chat_completion(ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user(
                "What are the benefits of using Azure OpenAI?",
            )],
            max_tokens: Some(150),
            temperature: Some(0.5),
            ..Default::default()
        })
        .await?;

    println!("Response: {}", response.choices[0].message.content);

    // Example 4: Google Vertex AI
    println!("\n=== Example 4: Google Vertex AI ===");
    let client = UltrafastClient::standalone()
        .with_google_vertex_ai("your-google-api-key", "your-project-id")
        .build()?;

    let response = client
        .chat_completion(ChatRequest {
            model: "chat-bison".to_string(),
            messages: vec![Message::user("What is machine learning?")],
            max_tokens: Some(200),
            temperature: Some(0.7),
            ..Default::default()
        })
        .await?;

    println!("Response: {}", response.choices[0].message.content);

    // Example 5: Embeddings
    println!("\n=== Example 5: Embeddings ===");
    let embedding_response = client
        .embedding(EmbeddingRequest {
            model: "text-embedding-ada-002".to_string(),
            input: EmbeddingInput::String("This is a test sentence for embeddings.".to_string()),
            ..Default::default()
        })
        .await?;

    println!(
        "Embedding dimensions: {}",
        embedding_response.data[0].embedding.len()
    );

    // Example 6: Streaming responses
    println!("\n=== Example 6: Streaming Response ===");
    let mut stream = client
        .stream_chat_completion(ChatRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: vec![Message::user(
                "Write a short story about a robot learning to paint",
            )],
            max_tokens: Some(300),
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

    // Example 7: Fallback strategy
    println!("\n=== Example 7: Fallback Strategy ===");
    let client = UltrafastClient::standalone()
        .with_openai("sk-your-openai-key")
        .with_anthropic("sk-ant-your-anthropic-key")
        .with_routing_strategy(RoutingStrategy::Fallback)
        .build()?;

    let response = client
        .chat_completion(ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("What is the meaning of life?")],
            max_tokens: Some(100),
            temperature: Some(0.9),
            ..Default::default()
        })
        .await?;

    println!("Response: {}", response.choices[0].message.content);

    Ok(())
}
