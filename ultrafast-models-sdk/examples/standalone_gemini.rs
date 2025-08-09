use ultrafast_models_sdk::{providers::ProviderConfig, ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("GEMINI_API_KEY").unwrap_or_default();

    let mut cfg = ProviderConfig::new("gemini", key);
    // Optionally set base_url via GEMINI_BASE_URL
    if let Ok(base) = std::env::var("GEMINI_BASE_URL") {
        cfg.base_url = Some(base);
    }

    let client = UltrafastClient::standalone()
        .with_provider("gemini", cfg)
        .build()?;

    let req = ChatRequest {
        model: "gemini-1.5-pro".to_string(),
        messages: vec![Message::user("Say hello from Gemini standalone")],
        max_tokens: Some(32),
        ..Default::default()
    };
    let resp = client.chat_completion(req).await?;
    println!("{}: {}", resp.model, resp.choices[0].message.content);
    Ok(())
}
