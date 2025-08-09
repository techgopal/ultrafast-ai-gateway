use ultrafast_models_sdk::{ChatRequest, Message, UltrafastClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("GOOGLE_API_KEY").unwrap_or_default();
    let project = std::env::var("GOOGLE_PROJECT_ID").unwrap_or_else(|_| "my-project".to_string());

    let client = UltrafastClient::standalone()
        .with_google_vertex_ai(key, project)
        .build()?;

    let req = ChatRequest {
        model: "gemini-1.5-pro".to_string(),
        messages: vec![Message::user("Say hello from Google Vertex AI standalone")],
        max_tokens: Some(32),
        ..Default::default()
    };
    let resp = client.chat_completion(req).await?;
    println!("{}: {}", resp.model, resp.choices[0].message.content);
    Ok(())
}
