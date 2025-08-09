use crate::error::ProviderError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, Choice, EmbeddingRequest,
    EmbeddingResponse, ImageRequest, ImageResponse, Message, Role, SpeechRequest, SpeechResponse,
    StreamChunk, Usage,
};
use crate::providers::{HealthStatus, Provider, ProviderConfig, ProviderHealth, StreamResult};
use async_stream::stream;
use serde::{Deserialize, Serialize};

use super::http_client::{map_error_response, AuthStrategy, HttpProviderClient};
use std::collections::HashMap;
use std::time::Instant;

pub struct AnthropicProvider {
    http: HttpProviderClient,
    config: ProviderConfig,
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    response_type: String,
    #[allow(dead_code)]
    role: String,
    content: Vec<AnthropicContent>,
    model: String,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    content_type: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

impl AnthropicProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        let mut headers = config.headers.clone();
        headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
        let http = HttpProviderClient::new(
            config.timeout,
            config.base_url.clone(),
            "https://api.anthropic.com",
            &headers,
            AuthStrategy::Header {
                name: "x-api-key".to_string(),
                value: config.api_key.clone(),
            },
        )?;

        Ok(Self { http, config })
    }

    fn map_model(&self, model: &str) -> String {
        self.config
            .model_mapping
            .get(model)
            .cloned()
            .unwrap_or_else(|| {
                // Map common model names to Anthropic equivalents
                match model {
                    "claude-3" | "claude" => "claude-3-5-sonnet-20241022".to_string(),
                    "claude-3-opus" => "claude-opus-4-20250514".to_string(),
                    "claude-3-sonnet" => "claude-3-5-sonnet-20241022".to_string(),
                    "claude-3-haiku" => "claude-3-5-haiku-20241022".to_string(),
                    "claude-4-opus" => "claude-opus-4-20250514".to_string(),
                    "claude-4-sonnet" => "claude-sonnet-4-20250514".to_string(),
                    _ => model.to_string(),
                }
            })
    }

    fn convert_messages(&self, messages: Vec<Message>) -> Vec<AnthropicMessage> {
        let mut anthropic_messages = Vec::new();
        let mut system_content = String::new();

        for msg in messages {
            match msg.role {
                Role::System => {
                    // Collect system messages into a single system prompt
                    if !msg.content.is_empty() {
                        if !system_content.is_empty() {
                            system_content.push('\n');
                        }
                        system_content.push_str(&msg.content);
                    }
                }
                Role::User => {
                    // If we have system content, prepend it to the first user message
                    if !system_content.is_empty() && anthropic_messages.is_empty() {
                        anthropic_messages.push(AnthropicMessage {
                            role: "user".to_string(),
                            content: format!("{}\n\n{}", system_content, msg.content),
                        });
                        system_content.clear(); // Clear after using
                    } else {
                        anthropic_messages.push(AnthropicMessage {
                            role: "user".to_string(),
                            content: msg.content,
                        });
                    }
                }
                Role::Assistant => {
                    anthropic_messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: msg.content,
                    });
                }
                _ => {
                    // Skip other role types (tool calls, etc.)
                    continue;
                }
            }
        }

        // If we have system content but no user messages, create a user message with just the system content
        if !system_content.is_empty() && anthropic_messages.is_empty() {
            anthropic_messages.push(AnthropicMessage {
                role: "user".to_string(),
                content: system_content,
            });
        }

        anthropic_messages
    }

    fn convert_response(&self, response: AnthropicResponse) -> ChatResponse {
        let content = response
            .content
            .into_iter()
            .map(|c| c.text)
            .collect::<Vec<_>>()
            .join("");

        ChatResponse {
            id: response.id,
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp() as u64,
            model: response.model,
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: Role::Assistant,
                    content,
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: response.stop_reason,
                logprobs: None,
            }],
            usage: Some(Usage {
                prompt_tokens: response.usage.input_tokens,
                completion_tokens: response.usage.output_tokens,
                total_tokens: response.usage.input_tokens + response.usage.output_tokens,
            }),
            system_fingerprint: None,
        }
    }

    // Use shared map_error_response
}

#[async_trait::async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        true
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "claude-opus-4-20250514".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            "claude-3-7-sonnet-20250219".to_string(),
            "claude-3-5-sonnet-20241022".to_string(),
            "claude-3-5-haiku-20241022".to_string(),
            "claude-3-5-sonnet-20240620".to_string(),
            "claude-3-haiku-20240307".to_string(),
            "claude-3".to_string(),
            "claude".to_string(),
        ]
    }

    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let model = self.map_model(&request.model);
        let messages = self.convert_messages(request.messages);

        let anthropic_request = AnthropicRequest {
            model,
            max_tokens: request.max_tokens.unwrap_or(4096),
            messages,
            temperature: request.temperature,
            stream: Some(false),
        };

        let anthropic_response: AnthropicResponse = self
            .http
            .post_json("/v1/messages", &anthropic_request)
            .await?;
        Ok(self.convert_response(anthropic_response))
    }

    async fn stream_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<StreamResult, ProviderError> {
        let model = self.map_model(&request.model);
        let messages = self.convert_messages(request.messages);

        let anthropic_request = AnthropicRequest {
            model,
            max_tokens: request.max_tokens.unwrap_or(4096),
            messages,
            temperature: request.temperature,
            stream: Some(true),
        };

        let response = self
            .http
            .post_json_raw("/v1/messages", &anthropic_request)
            .await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }

        let stream = Box::pin(stream! {
            let mut bytes_stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_result) = futures::StreamExt::next(&mut bytes_stream).await {
                match chunk_result {
                    Ok(chunk) => {
                        let chunk_str = String::from_utf8_lossy(&chunk);
                        buffer.push_str(&chunk_str);

                        while let Some(line_end) = buffer.find('\n') {
                            let line = buffer[..line_end].trim().to_string();
                            buffer = buffer[line_end + 1..].to_string();

                            if let Some(json_str) = line.strip_prefix("data: ") {
                                if json_str == "[DONE]" {
                                    return;
                                }

                                // Convert Anthropic stream format to OpenAI-compatible format
                                match serde_json::from_str::<serde_json::Value>(json_str) {
                                    Ok(anthropic_chunk) => {
                                        if let Some(content_delta) = anthropic_chunk
                                            .get("delta")
                                            .and_then(|d| d.get("text"))
                                            .and_then(|t| t.as_str()) {

                                            let stream_chunk = StreamChunk {
                                                id: anthropic_chunk.get("id")
                                                    .and_then(|id| id.as_str())
                                                    .unwrap_or("anthropic-stream")
                                                    .to_string(),
                                                object: "chat.completion.chunk".to_string(),
                                                created: chrono::Utc::now().timestamp() as u64,
                                                model: anthropic_chunk.get("model")
                                                    .and_then(|m| m.as_str())
                                                    .unwrap_or("claude-3-sonnet")
                                                    .to_string(),
                                                choices: vec![crate::models::StreamChoice {
                                                    index: 0,
                                                    delta: crate::models::Delta {
                                                        role: None,
                                                        content: Some(content_delta.to_string()),
                                                        tool_calls: None,
                                                    },
                                                    finish_reason: None,
                                                }],
                                            };
                                            yield Ok(stream_chunk);
                                        }
                                    }
                                    Err(e) => yield Err(ProviderError::Serialization(e)),
                                }
                            }
                        }
                    }
                    Err(e) => yield Err(ProviderError::Http(e)),
                }
            }
        });

        Ok(stream)
    }

    async fn embedding(
        &self,
        _request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Anthropic does not support embeddings".to_string(),
        })
    }

    async fn image_generation(
        &self,
        _request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Anthropic does not support image generation".to_string(),
        })
    }

    async fn audio_transcription(
        &self,
        _request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Anthropic does not support audio transcription".to_string(),
        })
    }

    async fn text_to_speech(
        &self,
        _request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Anthropic does not support text-to-speech".to_string(),
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
        let start = Instant::now();

        // Anthropic doesn't have a dedicated models endpoint, so we'll use a minimal completion request
        let health_request = AnthropicRequest {
            model: "claude-3-5-haiku-20241022".to_string(),
            max_tokens: 1,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: "Hi".to_string(),
            }],
            temperature: Some(0.0),
            stream: Some(false),
        };

        let response = self
            .http
            .post_json::<AnthropicRequest, serde_json::Value>("/v1/messages", &health_request)
            .await;

        let latency_ms = start.elapsed().as_millis() as u64;

        match response {
            Ok(_) => Ok(ProviderHealth {
                status: HealthStatus::Healthy,
                latency_ms: Some(latency_ms),
                error_rate: 0.0,
                last_check: chrono::Utc::now(),
                details: HashMap::new(),
            }),
            Err(e) => {
                let mut details = HashMap::new();
                details.insert("error".to_string(), e.to_string());

                Ok(ProviderHealth {
                    status: HealthStatus::Degraded,
                    latency_ms: Some(latency_ms),
                    error_rate: 1.0,
                    last_check: chrono::Utc::now(),
                    details,
                })
            }
        }
    }
}
