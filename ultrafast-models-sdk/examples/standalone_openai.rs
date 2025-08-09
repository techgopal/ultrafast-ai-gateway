use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let openai_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();

    let client = UltrafastClient::standalone()
        .with_openai(openai_key)
        .build()?;

    let req = ChatRequest {
        model: "gpt-4o-mini".to_string(),
        messages: vec![Message::user("Say hello from OpenAI standalone")],
        max_tokens: Some(32),
        ..Default::default()
    };

    let resp = client.chat_completion(req).await?;
    println!("{}: {}", resp.model, resp.choices[0].message.content);
    Ok(())
}
