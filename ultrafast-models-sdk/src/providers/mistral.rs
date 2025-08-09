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

pub struct MistralProvider {
    http: HttpProviderClient,
    config: ProviderConfig,
}

impl MistralProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        let http = HttpProviderClient::new(
            config.timeout,
            config.base_url.clone(),
            "https://api.mistral.ai/v1",
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
impl Provider for MistralProvider {
    fn name(&self) -> &str {
        "mistral"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        true
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "mistral-tiny".to_string(),
            "mistral-small".to_string(),
            "mistral-medium".to_string(),
            "mistral-large".to_string(),
            "mistral-large-latest".to_string(),
            "mistral-embed".to_string(),
        ]
    }

    async fn chat_completion(
        &self,
        mut request: ChatRequest,
    ) -> Result<ChatResponse, ProviderError> {
        request.model = self.map_model(&request.model);

        let chat_response: ChatResponse =
            self.http.post_json("/chat/completions", &request).await?;
        Ok(chat_response)
    }

    async fn stream_chat_completion(
        &self,
        mut request: ChatRequest,
    ) -> Result<StreamResult, ProviderError> {
        request.model = self.map_model(&request.model);
        request.stream = Some(true);

        let response = self
            .http
            .post_json_raw("/chat/completions", &request)
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

        let mistral_request = json!({
            "model": model,
            "input": input,
            "encoding_format": request.encoding_format.unwrap_or_else(|| "float".to_string()),
        });

        let mistral_response: serde_json::Value =
            self.http.post_json("/embeddings", &mistral_request).await?;

        // Convert Mistral response to OpenAI format
        let embeddings = mistral_response["data"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .enumerate()
            .map(|(i, embedding)| {
                let embedding_vec = embedding["embedding"]
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
                prompt_tokens: mistral_response["usage"]["prompt_tokens"]
                    .as_u64()
                    .unwrap_or(0) as u32,
                completion_tokens: 0,
                total_tokens: mistral_response["usage"]["total_tokens"]
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
            message: "Image generation not supported by Mistral".to_string(),
        })
    }

    async fn audio_transcription(
        &self,
        _request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Audio transcription not supported by Mistral".to_string(),
        })
    }

    async fn text_to_speech(
        &self,
        _request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Text-to-speech not supported by Mistral".to_string(),
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
