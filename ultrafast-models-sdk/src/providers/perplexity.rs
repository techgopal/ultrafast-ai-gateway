use crate::error::ProviderError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
    ImageRequest, ImageResponse, SpeechRequest, SpeechResponse, StreamChunk,
};
use crate::providers::{HealthStatus, Provider, ProviderConfig, ProviderHealth, StreamResult};
use async_stream::stream;
use serde_json::json;

use super::http_client::{map_error_response, AuthStrategy, HttpProviderClient};

use std::collections::HashMap;
use std::time::Instant;

pub struct PerplexityProvider {
    http: HttpProviderClient,
    config: ProviderConfig,
}

impl PerplexityProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        let http = HttpProviderClient::new(
            config.timeout,
            config.base_url.clone(),
            "https://api.perplexity.ai",
            &config.headers,
            AuthStrategy::Bearer {
                token: config.api_key.clone(),
            },
        )?;
        Ok(Self { http, config })
    }

    fn map_model(&self, model: &str) -> String {
        self.config
            .model_mapping
            .get(model)
            .cloned()
            .unwrap_or_else(|| model.to_string())
    }

    #[allow(dead_code)]
    async fn handle_error_response(&self, response: reqwest::Response) -> ProviderError {
        let status = response.status();

        match response.text().await {
            Ok(body) => {
                if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&body) {
                    let message = error_json
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown API error")
                        .to_string();

                    match status.as_u16() {
                        401 => ProviderError::InvalidApiKey,
                        404 => ProviderError::ModelNotFound {
                            model: "unknown".to_string(),
                        },
                        429 => ProviderError::RateLimit,
                        _ => ProviderError::Api {
                            code: status.as_u16(),
                            message,
                        },
                    }
                } else {
                    ProviderError::Api {
                        code: status.as_u16(),
                        message: body,
                    }
                }
            }
            Err(_) => ProviderError::Api {
                code: status.as_u16(),
                message: "Failed to read error response".to_string(),
            },
        }
    }
}

#[async_trait::async_trait]
impl Provider for PerplexityProvider {
    fn name(&self) -> &str {
        "perplexity"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        false // Perplexity doesn't support function calling yet
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "llama-3.1-8b-instant".to_string(),
            "llama-3.1-70b-vision".to_string(),
            "llama-3.1-8b-online".to_string(),
            "llama-3.1-70b-online".to_string(),
            "mixtral-8x7b-instruct".to_string(),
            "codellama-70b-instruct".to_string(),
            "pplx-7b-online".to_string(),
            "pplx-70b-online".to_string(),
            "pplx-7b-chat".to_string(),
            "pplx-70b-chat".to_string(),
        ]
    }

    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let model = self.map_model(&request.model);

        // Convert OpenAI format to Perplexity format
        let perplexity_request = json!({
            "model": model,
            "messages": request.messages.iter().map(|msg| {
                json!({
                    "role": match msg.role {
                        crate::models::Role::User => "user",
                        crate::models::Role::Assistant => "assistant",
                        crate::models::Role::System => "system",
                        crate::models::Role::Tool => "user",
                    },
                    "content": msg.content
                })
            }).collect::<Vec<_>>(),
            "temperature": request.temperature.unwrap_or(0.7),
            "max_tokens": request.max_tokens,
            "stream": false,
        });

        let perplexity_response: serde_json::Value = self
            .http
            .post_json("/chat/completions", &perplexity_request)
            .await?;

        // Convert Perplexity response to OpenAI format
        let chat_response = ChatResponse {
            id: perplexity_response["id"].as_str().unwrap_or("").to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp() as u64,
            model,
            choices: vec![crate::models::Choice {
                index: 0,
                message: crate::models::Message {
                    role: crate::models::Role::Assistant,
                    content: perplexity_response["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: Some(crate::models::Usage {
                prompt_tokens: perplexity_response["usage"]["prompt_tokens"]
                    .as_u64()
                    .unwrap_or(0) as u32,
                completion_tokens: perplexity_response["usage"]["completion_tokens"]
                    .as_u64()
                    .unwrap_or(0) as u32,
                total_tokens: perplexity_response["usage"]["total_tokens"]
                    .as_u64()
                    .unwrap_or(0) as u32,
            }),
            system_fingerprint: None,
        };

        Ok(chat_response)
    }

    async fn stream_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<StreamResult, ProviderError> {
        let model = self.map_model(&request.model);

        let perplexity_request = json!({
            "model": model,
            "messages": request.messages.iter().map(|msg| {
                json!({
                    "role": match msg.role {
                        crate::models::Role::User => "user",
                        crate::models::Role::Assistant => "assistant",
                        crate::models::Role::System => "system",
                        crate::models::Role::Tool => "user",
                    },
                    "content": msg.content
                })
            }).collect::<Vec<_>>(),
            "temperature": request.temperature.unwrap_or(0.7),
            "max_tokens": request.max_tokens,
            "stream": true,
        });

        let response = self
            .http
            .post_json_raw("/chat/completions", &perplexity_request)
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

                                // Parse Perplexity streaming format and convert to OpenAI format
                                if let Ok(perplexity_chunk) = serde_json::from_str::<serde_json::Value>(json_str) {
                                    if let Some(choices) = perplexity_chunk["choices"].as_array() {
                                        if let Some(first_choice) = choices.first() {
                                            if let Some(delta) = first_choice.get("delta") {
                                                if let Some(content) = delta["content"].as_str() {
                                                    let stream_chunk = StreamChunk {
                                                        id: perplexity_chunk["id"].as_str().unwrap_or("").to_string(),
                                                        object: "chat.completion.chunk".to_string(),
                                                        created: chrono::Utc::now().timestamp() as u64,
                                                        model: model.clone(),
                                                        choices: vec![crate::models::StreamChoice {
                                                            index: 0,
                                                            delta: crate::models::Delta {
                                                                role: None,
                                                                content: Some(content.to_string()),
                                                                tool_calls: None,
                                                            },
                                                            finish_reason: None,
                                                        }],
                                                    };
                                                    yield Ok(stream_chunk);
                                                }
                                            }
                                        }
                                    }
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
            message: "Embeddings not supported by Perplexity".to_string(),
        })
    }

    async fn image_generation(
        &self,
        _request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Image generation not supported by Perplexity".to_string(),
        })
    }

    async fn audio_transcription(
        &self,
        _request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Audio transcription not supported by Perplexity".to_string(),
        })
    }

    async fn text_to_speech(
        &self,
        _request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Text-to-speech not supported by Perplexity".to_string(),
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
        let start = Instant::now();

        let response = self.http.get_json::<serde_json::Value>("/models").await;

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
