use std::env;
use std::time::Duration;

use futures::StreamExt;
use ultrafast_models_sdk::client::UltrafastClient;
use ultrafast_models_sdk::models::{
    ChatRequest, EmbeddingInput, EmbeddingRequest, ImageRequest, Message,
};
use ultrafast_models_sdk::providers::ProviderConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY not set");

    // Prefer explicit headers recommended by OpenRouter
    let cfg = ProviderConfig::new("openrouter", api_key)
        .with_timeout(Duration::from_secs(60))
        .with_model_mapping("gpt-4o-mini", "openai/gpt-4o-mini")
        .with_model_mapping("text-embedding-3-small", "openai/text-embedding-3-small")
        .with_header("HTTP-Referer", "https://ultrafast-gateway.local/test")
        .with_header("X-Title", "Ultrafast OpenRouter Test");
    // Optional: base URL override if needed
    // cfg.base_url = Some("https://openrouter.ai/api/v1".to_string());

    let client = UltrafastClient::standalone()
        .with_provider("openrouter", cfg)
        .build()?;

    // 1) Chat completion (non-stream)
    println!("\n== Chat completion ==");
    let chat_resp = client
        .chat_completion(ChatRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![Message::user("Say hello from OpenRouter test.")],
            max_tokens: Some(64),
            temperature: Some(0.2),
            ..Default::default()
        })
        .await?;
    println!(
        "chat: {}",
        chat_resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default()
    );

    // 2) Chat completion (stream)
    println!("\n== Chat streaming ==");
    let mut stream = client
        .stream_chat_completion(ChatRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![Message::user("Stream a short sentence word by word.")],
            stream: Some(true),
            max_tokens: Some(64),
            temperature: Some(0.7),
            ..Default::default()
        })
        .await?;
    while let Some(chunk) = StreamExt::next(&mut stream).await {
        match chunk {
            Ok(ch) => {
                if let Some(delta) = ch.choices.first().and_then(|c| c.delta.content.clone()) {
                    print!("{delta}");
                }
            }
            Err(e) => {
                eprintln!("stream error: {e:?}");
                break;
            }
        }
    }
    println!();

    // 3) Embeddings (if supported by underlying model)
    println!("\n== Embeddings ==");
    match client
        .embedding(EmbeddingRequest {
            model: "text-embedding-3-small".to_string(),
            input: EmbeddingInput::StringArray(vec!["OpenRouter embedding test.".to_string()]),
            ..Default::default()
        })
        .await
    {
        Ok(emb) => println!(
            "embedding dims: {}",
            emb.data.first().map(|d| d.embedding.len()).unwrap_or(0)
        ),
        Err(e) => eprintln!("embedding error: {e:?}"),
    }

    // 4) Image generation (may or may not be supported depending on model availability)
    // Attempt; if not supported, an error will be printed but won't fail the example
    println!("\n== Image generation (best-effort) ==");
    match client
        .image_generation(ImageRequest {
            prompt: "A tiny robot waving next to a rocket ship".to_string(),
            model: Some("openai/dall-e-3".to_string()),
            n: Some(1),
            size: Some("512x512".to_string()),
            quality: None,
            response_format: None,
            style: None,
            user: None,
        })
        .await
    {
        Ok(img) => println!("image: {:?}", img.data.first()),
        Err(e) => eprintln!("image generation not available: {e:?}"),
    }

    // 5) Provider health overview via client helper
    let health = client.get_provider_health_status().await;
    println!("\n== Provider health ==\n{health:?}");

    Ok(())
}
