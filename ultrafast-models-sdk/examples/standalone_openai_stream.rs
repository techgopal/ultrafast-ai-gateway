use futures::StreamExt;
use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let openai_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    let client = UltrafastClient::standalone()
        .with_openai(openai_key)
        .build()?;

    let req = ChatRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![Message::user("Stream hello from OpenAI standalone")],
        max_tokens: Some(64),
        stream: Some(true),
        ..Default::default()
    };

    let mut stream = client.stream_chat_completion(req).await?;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if let Some(delta) = chunk.choices.first().and_then(|c| c.delta.content.clone()) {
            print!("{delta}");
        }
    }
    println!();
    Ok(())
}
