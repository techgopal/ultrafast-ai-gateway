use crate::error::ProviderError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
    ImageRequest, ImageResponse, SpeechRequest, SpeechResponse, StreamChunk,
};
use crate::providers::{HealthStatus, Provider, ProviderConfig, ProviderHealth, StreamResult};
use async_stream::stream;

use super::http_client::{map_error_response, AuthStrategy, HttpProviderClient};

use std::collections::HashMap;
use std::time::Instant;

pub struct GroqProvider {
    http: HttpProviderClient,
    config: ProviderConfig,
}

impl GroqProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        let http = HttpProviderClient::new(
            config.timeout,
            config.base_url.clone(),
            "https://api.groq.com/openai/v1",
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
}

#[async_trait::async_trait]
impl Provider for GroqProvider {
    fn name(&self) -> &str {
        "groq"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        true
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "llama3-8b-8192".to_string(),
            "llama3-70b-8192".to_string(),
            "mixtral-8x7b-32768".to_string(),
            "gemma-7b-it".to_string(),
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
        _request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Embeddings not supported by Groq".to_string(),
        })
    }

    async fn image_generation(
        &self,
        _request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Image generation not supported by Groq".to_string(),
        })
    }

    async fn audio_transcription(
        &self,
        _request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Audio transcription not supported by Groq".to_string(),
        })
    }

    async fn text_to_speech(
        &self,
        _request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Text-to-speech not supported by Groq".to_string(),
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
