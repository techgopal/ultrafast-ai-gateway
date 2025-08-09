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
        model: "command-r".to_string(),
        messages: vec![Message::user("Say hello via gateway (Cohere)")],
        max_tokens: Some(32),
        ..Default::default()
    };
    let resp = client.chat_completion(req).await?;
    println!("{}: {}", resp.model, resp.choices[0].message.content);
    Ok(())
}
