use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("AZURE_OPENAI_API_KEY").unwrap_or_default();
    let deployment =
        std::env::var("AZURE_OPENAI_DEPLOYMENT").unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let client = UltrafastClient::standalone()
        .with_azure_openai(key, deployment.clone())
        .build()?;

    let req = ChatRequest {
        model: deployment,
        messages: vec![Message::user("Say hello from Azure OpenAI standalone")],
        max_tokens: Some(32),
        ..Default::default()
    };
    let resp = client.chat_completion(req).await?;
    println!("{}: {}", resp.model, resp.choices[0].message.content);
    Ok(())
}
