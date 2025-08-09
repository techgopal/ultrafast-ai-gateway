use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("MISTRAL_API_KEY").unwrap_or_default();

    let client = UltrafastClient::standalone().with_mistral(key).build()?;

    let req = ChatRequest {
        model: "mixtral-8x7b-32768".to_string(),
        messages: vec![Message::user("Say hello from Mistral standalone")],
        max_tokens: Some(32),
        ..Default::default()
    };
    let resp = client.chat_completion(req).await?;
    println!("{}: {}", resp.model, resp.choices[0].message.content);
    Ok(())
}
