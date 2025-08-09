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

#[derive(Debug, Clone)]
pub struct CustomProviderConfig {
    pub chat_endpoint: String,
    pub embedding_endpoint: Option<String>,
    pub image_endpoint: Option<String>,
    pub audio_endpoint: Option<String>,
    pub speech_endpoint: Option<String>,
    pub request_format: RequestFormat,
    pub response_format: ResponseFormat,
    pub auth_type: AuthType,
}

#[derive(Debug, Clone)]
pub enum RequestFormat {
    OpenAI,
    Anthropic,
    Custom { template: String },
}

#[derive(Debug, Clone)]
pub enum ResponseFormat {
    OpenAI,
    Anthropic,
    Custom { template: String },
}

#[derive(Debug, Clone)]
pub enum AuthType {
    Bearer,
    ApiKey,
    Custom { header: String },
    None,
}

pub struct CustomProvider {
    http: HttpProviderClient,
    config: ProviderConfig,
    custom_config: CustomProviderConfig,
}

impl CustomProvider {
    pub fn new(
        config: ProviderConfig,
        custom_config: CustomProviderConfig,
    ) -> Result<Self, ProviderError> {
        let auth = match &custom_config.auth_type {
            AuthType::Bearer => AuthStrategy::Bearer {
                token: config.api_key.clone(),
            },
            AuthType::ApiKey => AuthStrategy::Header {
                name: "X-API-Key".to_string(),
                value: config.api_key.clone(),
            },
            AuthType::Custom { header } => AuthStrategy::Header {
                name: header.clone(),
                value: config.api_key.clone(),
            },
            AuthType::None => AuthStrategy::None,
        };

        let http = HttpProviderClient::new(
            config.timeout,
            config.base_url.clone(),
            "http://localhost:8080",
            &config.headers,
            auth,
        )?;

        Ok(Self {
            http,
            config,
            custom_config,
        })
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

    fn format_request(&self, request: &ChatRequest) -> Result<serde_json::Value, ProviderError> {
        match &self.custom_config.request_format {
            RequestFormat::OpenAI => Ok(json!({
                "model": self.map_model(&request.model),
                "messages": request.messages,
                "temperature": request.temperature,
                "max_tokens": request.max_tokens,
                "stream": request.stream,
            })),
            RequestFormat::Anthropic => {
                let messages = request
                    .messages
                    .iter()
                    .map(|msg| {
                        json!({
                            "role": match msg.role {
                                crate::models::Role::User => "user",
                                crate::models::Role::Assistant => "assistant",
                                crate::models::Role::System => "system",
                                crate::models::Role::Tool => "user",
                            },
                            "content": msg.content
                        })
                    })
                    .collect::<Vec<_>>();

                Ok(json!({
                    "model": self.map_model(&request.model),
                    "messages": messages,
                    "temperature": request.temperature,
                    "max_tokens": request.max_tokens,
                    "stream": request.stream,
                }))
            }
            RequestFormat::Custom { template } => {
                // Simple template substitution - in a real implementation, you'd want a proper templating engine
                let mut formatted = template.clone();
                formatted = formatted.replace("{{model}}", &self.map_model(&request.model));
                formatted = formatted.replace(
                    "{{temperature}}",
                    &request.temperature.unwrap_or(0.7).to_string(),
                );
                formatted = formatted.replace(
                    "{{max_tokens}}",
                    &request.max_tokens.unwrap_or(100).to_string(),
                );

                serde_json::from_str(&formatted).map_err(|e| ProviderError::Configuration {
                    message: format!("Invalid custom request template: {e}"),
                })
            }
        }
    }

    fn parse_response(&self, response: serde_json::Value) -> Result<ChatResponse, ProviderError> {
        match &self.custom_config.response_format {
            ResponseFormat::OpenAI => {
                let chat_response: ChatResponse =
                    serde_json::from_value(response).map_err(ProviderError::Serialization)?;
                Ok(chat_response)
            }
            ResponseFormat::Anthropic => {
                // Convert Anthropic format to OpenAI format
                let chat_response = ChatResponse {
                    id: response["id"].as_str().unwrap_or("").to_string(),
                    object: "chat.completion".to_string(),
                    created: chrono::Utc::now().timestamp() as u64,
                    model: response["model"].as_str().unwrap_or("").to_string(),
                    choices: vec![crate::models::Choice {
                        index: 0,
                        message: crate::models::Message {
                            role: crate::models::Role::Assistant,
                            content: response["content"][0]["text"]
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
                        prompt_tokens: response["usage"]["input_tokens"].as_u64().unwrap_or(0)
                            as u32,
                        completion_tokens: response["usage"]["output_tokens"].as_u64().unwrap_or(0)
                            as u32,
                        total_tokens: response["usage"]["input_tokens"].as_u64().unwrap_or(0)
                            as u32
                            + response["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
                    }),
                    system_fingerprint: None,
                };
                Ok(chat_response)
            }
            ResponseFormat::Custom { template } => {
                // Simple template parsing - in a real implementation, you'd want a proper templating engine
                let response_str =
                    serde_json::to_string(&response).map_err(ProviderError::Serialization)?;

                let mut formatted = template.clone();
                formatted = formatted.replace("{{response}}", &response_str);

                serde_json::from_str(&formatted).map_err(|e| ProviderError::Configuration {
                    message: format!("Invalid custom response template: {e}"),
                })
            }
        }
    }
}

#[async_trait::async_trait]
impl Provider for CustomProvider {
    fn name(&self) -> &str {
        "custom"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        false // Custom providers don't support function calling by default
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["custom-model".to_string()]
    }

    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let formatted_request = self.format_request(&request)?;

        let url = self.custom_config.chat_endpoint.to_string();
        let response_json: serde_json::Value =
            self.http.post_json(&url, &formatted_request).await?;
        let chat_response = self.parse_response(response_json)?;
        Ok(chat_response)
    }

    async fn stream_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<StreamResult, ProviderError> {
        let mut formatted_request = self.format_request(&request)?;
        formatted_request["stream"] = serde_json::Value::Bool(true);

        let url = self.custom_config.chat_endpoint.to_string();
        let response = self.http.post_json_raw(&url, &formatted_request).await?;
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

                                match serde_json::from_str::<StreamChunk>(json_str) {
                                    Ok(stream_chunk) => yield Ok(stream_chunk),
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
        request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        if let Some(embedding_endpoint) = &self.custom_config.embedding_endpoint {
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

            let embedding_request = json!({
                "model": model,
                "input": input,
            });

            let url = embedding_endpoint.to_string();
            let embedding_response: EmbeddingResponse =
                self.http.post_json(&url, &embedding_request).await?;
            Ok(embedding_response)
        } else {
            Err(ProviderError::Configuration {
                message: "Embeddings not supported by this custom provider".to_string(),
            })
        }
    }

    async fn image_generation(
        &self,
        _request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Image generation not supported by custom providers".to_string(),
        })
    }

    async fn audio_transcription(
        &self,
        _request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Audio transcription not supported by custom providers".to_string(),
        })
    }

    async fn text_to_speech(
        &self,
        _request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Text-to-speech not supported by custom providers".to_string(),
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
        let start = Instant::now();

        let response = self.http.get_json::<serde_json::Value>("/health").await;

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
