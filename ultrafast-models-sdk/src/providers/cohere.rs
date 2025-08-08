use crate::error::ProviderError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
    ImageRequest, ImageResponse, SpeechRequest, SpeechResponse, StreamChunk,
};
use crate::providers::{HealthStatus, Provider, ProviderConfig, ProviderHealth, StreamResult};
use async_stream::stream;
use serde_json::json;

use reqwest::Client;

use std::collections::HashMap;
use std::time::Instant;

pub struct CohereProvider {
    client: Client,
    config: ProviderConfig,
    base_url: String,
}

impl CohereProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| ProviderError::Configuration {
                message: format!("Failed to create HTTP client: {e}"),
            })?;

        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.cohere.ai/v1".to_string());

        Ok(Self {
            client,
            config,
            base_url,
        })
    }

    fn build_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();

        headers.insert(
            "Authorization",
            format!("Bearer {}", self.config.api_key).parse().unwrap(),
        );

        headers.insert("Content-Type", "application/json".parse().unwrap());

        for (key, value) in &self.config.headers {
            if let (Ok(header_name), Ok(header_value)) =
                (key.parse::<reqwest::header::HeaderName>(), value.parse())
            {
                headers.insert(header_name, header_value);
            }
        }

        headers
    }

    fn map_model(&self, model: &str) -> String {
        self.config
            .model_mapping
            .get(model)
            .cloned()
            .unwrap_or_else(|| model.to_string())
    }

    async fn handle_error_response(&self, response: reqwest::Response) -> ProviderError {
        let status = response.status();

        match response.text().await {
            Ok(body) => {
                if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&body) {
                    let message = error_json
                        .get("message")
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

    #[allow(dead_code)]
    fn convert_messages_to_cohere_format(
        &self,
        messages: &[crate::models::Message],
    ) -> Vec<serde_json::Value> {
        messages
            .iter()
            .map(|msg| {
                let role = match msg.role {
                    crate::models::Role::User => "user",
                    crate::models::Role::Assistant => "assistant",
                    crate::models::Role::System => "system",
                    crate::models::Role::Tool => "user", // Cohere doesn't have tool role
                };

                json!({
                    "role": role,
                    "content": msg.content
                })
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl Provider for CohereProvider {
    fn name(&self) -> &str {
        "cohere"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        false // Cohere doesn't support function calling yet
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "command".to_string(),
            "command-light".to_string(),
            "command-nightly".to_string(),
            "command-light-nightly".to_string(),
            "embed-english-v3.0".to_string(),
            "embed-multilingual-v3.0".to_string(),
        ]
    }

    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let model = self.map_model(&request.model);

        // Convert OpenAI format to Cohere format
        let cohere_request = json!({
            "model": model,
            "message": request.messages.last().map(|m| m.content.clone()).unwrap_or_default(),
            "chat_history": request.messages[..request.messages.len()-1].iter().map(|m| {
                json!({
                    "role": match m.role {
                        crate::models::Role::User => "user",
                        crate::models::Role::Assistant => "assistant",
                        crate::models::Role::System => "system",
                        crate::models::Role::Tool => "user",
                    },
                    "message": m.content
                })
            }).collect::<Vec<_>>(),
            "temperature": request.temperature.unwrap_or(0.7),
            "max_tokens": request.max_tokens,
            "stream": false,
        });

        let url = format!("{}/chat", self.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&cohere_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let cohere_response: serde_json::Value = response.json().await?;

        // Convert Cohere response to OpenAI format
        let chat_response = ChatResponse {
            id: cohere_response["response_id"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp() as u64,
            model,
            choices: vec![crate::models::Choice {
                index: 0,
                message: crate::models::Message {
                    role: crate::models::Role::Assistant,
                    content: cohere_response["text"].as_str().unwrap_or("").to_string(),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: Some(crate::models::Usage {
                prompt_tokens: cohere_response["meta"]["billed_units"]["input_tokens"]
                    .as_u64()
                    .unwrap_or(0) as u32,
                completion_tokens: cohere_response["meta"]["billed_units"]["output_tokens"]
                    .as_u64()
                    .unwrap_or(0) as u32,
                total_tokens: cohere_response["meta"]["billed_units"]["input_tokens"]
                    .as_u64()
                    .unwrap_or(0) as u32
                    + cohere_response["meta"]["billed_units"]["output_tokens"]
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

        let cohere_request = json!({
            "model": model,
            "message": request.messages.last().map(|m| m.content.clone()).unwrap_or_default(),
            "chat_history": request.messages[..request.messages.len()-1].iter().map(|m| {
                json!({
                    "role": match m.role {
                        crate::models::Role::User => "user",
                        crate::models::Role::Assistant => "assistant",
                        crate::models::Role::System => "system",
                        crate::models::Role::Tool => "user",
                    },
                    "message": m.content
                })
            }).collect::<Vec<_>>(),
            "temperature": request.temperature.unwrap_or(0.7),
            "max_tokens": request.max_tokens,
            "stream": true,
        });

        let url = format!("{}/chat", self.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&cohere_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
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

                                // Parse Cohere streaming format and convert to OpenAI format
                                if let Ok(cohere_chunk) = serde_json::from_str::<serde_json::Value>(json_str) {
                                    if let Some(text) = cohere_chunk["text"].as_str() {
                                        let stream_chunk = StreamChunk {
                                            id: "cohere-stream".to_string(),
                                            object: "chat.completion.chunk".to_string(),
                                            created: chrono::Utc::now().timestamp() as u64,
                                            model: model.clone(),
                                            choices: vec![crate::models::StreamChoice {
                                                index: 0,
                                                delta: crate::models::Delta {
                                                    role: None,
                                                    content: Some(text.to_string()),
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
                    Err(e) => yield Err(ProviderError::Http(e)),
                }
            }
        });

        Ok(stream)
    }

    async fn embedding(
        &self,
        request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        let model = self.map_model(&request.model);

        let input = match &request.input {
            crate::models::EmbeddingInput::String(s) => vec![s.clone()],
            crate::models::EmbeddingInput::StringArray(arr) => arr.clone(),
            _ => {
                return Err(ProviderError::Configuration {
                    message: "Unsupported embedding input format".to_string(),
                })
            }
        };

        let cohere_request = json!({
            "model": model,
            "texts": input,
            "input_type": "search_document",
        });

        let url = format!("{}/embed", self.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&cohere_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let cohere_response: serde_json::Value = response.json().await?;

        // Convert Cohere response to OpenAI format
        let embeddings = cohere_response["embeddings"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .enumerate()
            .map(|(i, embedding)| {
                let embedding_vec = embedding["values"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect::<Vec<f32>>();

                crate::models::Embedding {
                    object: "embedding".to_string(),
                    embedding: embedding_vec,
                    index: i as u32,
                }
            })
            .collect();

        let embedding_response = EmbeddingResponse {
            object: "list".to_string(),
            data: embeddings,
            model,
            usage: crate::models::Usage {
                prompt_tokens: cohere_response["meta"]["billed_units"]["input_tokens"]
                    .as_u64()
                    .unwrap_or(0) as u32,
                completion_tokens: 0,
                total_tokens: cohere_response["meta"]["billed_units"]["input_tokens"]
                    .as_u64()
                    .unwrap_or(0) as u32,
            },
        };

        Ok(embedding_response)
    }

    async fn image_generation(
        &self,
        _request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Image generation not supported by Cohere".to_string(),
        })
    }

    async fn audio_transcription(
        &self,
        _request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Audio transcription not supported by Cohere".to_string(),
        })
    }

    async fn text_to_speech(
        &self,
        _request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Text-to-speech not supported by Cohere".to_string(),
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
        let start = Instant::now();

        let url = format!("{}/models", self.base_url);
        let headers = self.build_headers();

        let response = self.client.get(&url).headers(headers).send().await;

        let latency_ms = start.elapsed().as_millis() as u64;

        match response {
            Ok(resp) if resp.status().is_success() => Ok(ProviderHealth {
                status: HealthStatus::Healthy,
                latency_ms: Some(latency_ms),
                error_rate: 0.0,
                last_check: chrono::Utc::now(),
                details: HashMap::new(),
            }),
            Ok(resp) => {
                let mut details = HashMap::new();
                details.insert(
                    "status_code".to_string(),
                    resp.status().as_u16().to_string(),
                );

                Ok(ProviderHealth {
                    status: HealthStatus::Degraded,
                    latency_ms: Some(latency_ms),
                    error_rate: 1.0,
                    last_check: chrono::Utc::now(),
                    details,
                })
            }
            Err(e) => {
                let mut details = HashMap::new();
                details.insert("error".to_string(), e.to_string());

                Ok(ProviderHealth {
                    status: HealthStatus::Unhealthy,
                    latency_ms: Some(latency_ms),
                    error_rate: 1.0,
                    last_check: chrono::Utc::now(),
                    details,
                })
            }
        }
    }
}
