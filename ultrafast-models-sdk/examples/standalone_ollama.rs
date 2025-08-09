use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base =
        std::env::var("OLLAMA_BASE").unwrap_or_else(|_| "http://localhost:11434".to_string());

    let client = UltrafastClient::standalone().with_ollama(base).build()?;

    let req = ChatRequest {
        model: "llama3.2:1b".to_string(),
        messages: vec![Message::user("Say hello from Ollama standalone")],
        max_tokens: Some(32),
        ..Default::default()
    };
    let resp = client.chat_completion(req).await?;
    println!("{}: {}", resp.model, resp.choices[0].message.content);
    Ok(())
}
