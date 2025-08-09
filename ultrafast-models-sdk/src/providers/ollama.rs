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

pub struct OllamaProvider {
    http: HttpProviderClient,
    config: ProviderConfig,
}

impl OllamaProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        let http = HttpProviderClient::new(
            config.timeout,
            config.base_url.clone(),
            "http://localhost:11434",
            &config.headers,
            AuthStrategy::None,
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
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown API error")
                        .to_string();

                    match status.as_u16() {
                        404 => ProviderError::ModelNotFound {
                            model: "unknown".to_string(),
                        },
                        500 => ProviderError::ServiceUnavailable,
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
impl Provider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        false // Ollama doesn't support function calling yet
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "llama2".to_string(),
            "llama2:7b".to_string(),
            "llama2:13b".to_string(),
            "llama2:70b".to_string(),
            "codellama".to_string(),
            "codellama:7b".to_string(),
            "codellama:13b".to_string(),
            "codellama:34b".to_string(),
            "mistral".to_string(),
            "mistral:7b".to_string(),
            "llama2-uncensored".to_string(),
            "neural-chat".to_string(),
            "vicuna".to_string(),
            "orca-mini".to_string(),
            "llama2-uncensored".to_string(),
        ]
    }

    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let model = self.map_model(&request.model);

        // Convert OpenAI format to Ollama format
        let ollama_request = json!({
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
            "stream": false,
            "options": {
                "temperature": request.temperature.unwrap_or(0.7),
                "num_predict": request.max_tokens,
            }
        });

        let ollama_response: serde_json::Value =
            self.http.post_json("/api/chat", &ollama_request).await?;

        // Convert Ollama response to OpenAI format
        let chat_response = ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp() as u64,
            model,
            choices: vec![crate::models::Choice {
                index: 0,
                message: crate::models::Message {
                    role: crate::models::Role::Assistant,
                    content: ollama_response["message"]["content"]
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
                prompt_tokens: ollama_response["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
                completion_tokens: ollama_response["eval_count"].as_u64().unwrap_or(0) as u32,
                total_tokens: (ollama_response["prompt_eval_count"].as_u64().unwrap_or(0)
                    + ollama_response["eval_count"].as_u64().unwrap_or(0))
                    as u32,
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

        let ollama_request = json!({
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
            "stream": true,
            "options": {
                "temperature": request.temperature.unwrap_or(0.7),
                "num_predict": request.max_tokens,
            }
        });

        let response = self
            .http
            .post_json_raw("/api/chat", &ollama_request)
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

                            if !line.is_empty() {
                                // Parse Ollama streaming format and convert to OpenAI format
                                if let Ok(ollama_chunk) = serde_json::from_str::<serde_json::Value>(&line) {
                                    if let Some(message) = ollama_chunk.get("message") {
                                        if let Some(content) = message["content"].as_str() {
                                            let stream_chunk = StreamChunk {
                                                id: uuid::Uuid::new_v4().to_string(),
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
                                                    finish_reason: if ollama_chunk["done"].as_bool().unwrap_or(false) {
                                                        Some("stop".to_string())
                                                    } else {
                                                        None
                                                    },
                                                }],
                                            };
                                            yield Ok(stream_chunk);
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
        request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        let model = self.map_model(&request.model);

        let input = match &request.input {
            crate::models::EmbeddingInput::String(s) => s.clone(),
            _ => {
                return Err(ProviderError::Configuration {
                    message: "Ollama embeddings only support single string input".to_string(),
                })
            }
        };

        let ollama_request = json!({
            "model": model,
            "prompt": input,
        });

        let ollama_response: serde_json::Value = self
            .http
            .post_json("/api/embeddings", &ollama_request)
            .await?;

        // Convert Ollama response to OpenAI format
        let embedding_vec = ollama_response["embedding"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect::<Vec<f32>>();

        let embedding_response = EmbeddingResponse {
            object: "list".to_string(),
            data: vec![crate::models::Embedding {
                object: "embedding".to_string(),
                embedding: embedding_vec,
                index: 0,
            }],
            model,
            usage: crate::models::Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        };

        Ok(embedding_response)
    }

    async fn image_generation(
        &self,
        _request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Image generation not supported by Ollama".to_string(),
        })
    }

    async fn audio_transcription(
        &self,
        _request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Audio transcription not supported by Ollama".to_string(),
        })
    }

    async fn text_to_speech(
        &self,
        _request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Text-to-speech not supported by Ollama".to_string(),
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
        let start = Instant::now();

        let response = self.http.get_json::<serde_json::Value>("/api/tags").await;

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
