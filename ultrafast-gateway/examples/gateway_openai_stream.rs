use futures::StreamExt;
use std::time::Duration;
use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gateway =
        std::env::var("GATEWAY_BASE").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());
    let client = UltrafastClient::gateway(gateway)
        .with_timeout(Duration::from_secs(30))
        .build()?;

    let req = ChatRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![Message::user("Stream hello via gateway (OpenAI)")],
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
