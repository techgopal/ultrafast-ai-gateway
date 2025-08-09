use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();

    let client = UltrafastClient::standalone().with_anthropic(key).build()?;

    let req = ChatRequest {
        model: "claude-3-5-sonnet-latest".to_string(),
        messages: vec![Message::user("Say hello from Anthropic standalone")],
        max_tokens: Some(32),
        ..Default::default()
    };
    let resp = client.chat_completion(req).await?;
    println!("{}: {}", resp.model, resp.choices[0].message.content);
    Ok(())
}
