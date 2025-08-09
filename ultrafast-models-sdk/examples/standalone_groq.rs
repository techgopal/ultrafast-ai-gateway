use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("GROQ_API_KEY").unwrap_or_default();

    let client = UltrafastClient::standalone().with_groq(key).build()?;

    let req = ChatRequest {
        model: "openai/gpt-oss-120b".to_string(),
        messages: vec![Message::user("Say hello from Groq standalone")],
        max_tokens: Some(32),
        ..Default::default()
    };
    let resp = client.chat_completion(req).await?;
    println!("{}: {}", resp.model, resp.choices[0].message.content);
    Ok(())
}
