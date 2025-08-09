use crate::error::ProviderError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
    ImageRequest, ImageResponse, SpeechRequest, SpeechResponse, StreamChunk,
};
use crate::providers::{HealthStatus, Provider, ProviderConfig, ProviderHealth, StreamResult};
use async_stream::stream;
use std::collections::HashMap;
use std::time::Instant;

use super::http_client::{map_error_response, AuthStrategy, HttpProviderClient};

/// OpenRouter provider implementation (OpenAI-compatible API)
pub struct OpenRouterProvider {
    client: HttpProviderClient,
    config: ProviderConfig,
}

impl OpenRouterProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        // Allow custom headers like HTTP-Referer, X-Title to be passed via config.headers
        let client = HttpProviderClient::new(
            config.timeout,
            config.base_url.clone(),
            "https://openrouter.ai/api/v1",
            &config.headers,
            AuthStrategy::Bearer {
                token: config.api_key.clone(),
            },
        )?;

        Ok(Self { client, config })
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
impl Provider for OpenRouterProvider {
    fn name(&self) -> &str {
        "openrouter"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        true
    }

    fn supported_models(&self) -> Vec<String> {
        // Leave generic; OpenRouter aggregates many models. Users can override mapping.
        vec![
            "openrouter/gpt-4o".to_string(),
            "openrouter/gpt-4-turbo".to_string(),
        ]
    }

    async fn chat_completion(
        &self,
        mut request: ChatRequest,
    ) -> Result<ChatResponse, ProviderError> {
        request.model = self.map_model(&request.model);
        let chat_response: ChatResponse =
            self.client.post_json("/chat/completions", &request).await?;
        Ok(chat_response)
    }

    async fn stream_chat_completion(
        &self,
        mut request: ChatRequest,
    ) -> Result<StreamResult, ProviderError> {
        request.model = self.map_model(&request.model);
        request.stream = Some(true);

        let response = self
            .client
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
        mut request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        request.model = self.map_model(&request.model);
        // OpenRouter expects OpenAI-style embeddings endpoint, but some upstream models may not support it
        let resp = self.client.post_json_raw("/embeddings", &request).await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(map_error_response(resp).await);
        }
        let text = resp.text().await?;
        match serde_json::from_str::<EmbeddingResponse>(&text) {
            Ok(er) => Ok(er),
            Err(_) => Err(ProviderError::Api {
                code: status.as_u16(),
                message: text,
            }),
        }
    }

    async fn image_generation(
        &self,
        mut request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        if let Some(ref model) = request.model {
            request.model = Some(self.map_model(model));
        }

        // Fall back to 405 mapping if not supported
        let resp = self
            .client
            .post_json_raw("/images/generations", &request)
            .await?;
        if resp.status().as_u16() == 405 {
            return Err(ProviderError::Configuration {
                message: "Image generation not supported by OpenRouter for selected model"
                    .to_string(),
            });
        }
        let status = resp.status();
        if !status.is_success() {
            return Err(map_error_response(resp).await);
        }
        let text = resp.text().await?;
        match serde_json::from_str::<ImageResponse>(&text) {
            Ok(er) => Ok(er),
            Err(_) => Err(ProviderError::Api {
                code: status.as_u16(),
                message: text,
            }),
        }
    }

    async fn audio_transcription(
        &self,
        mut request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        request.model = self.map_model(&request.model);

        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(request.file)
                    .file_name("audio.mp3")
                    .mime_str("audio/mpeg")?,
            )
            .text("model", request.model);

        let form = if let Some(language) = request.language {
            form.text("language", language)
        } else {
            form
        };

        let form = if let Some(prompt) = request.prompt {
            form.text("prompt", prompt)
        } else {
            form
        };

        let response = self
            .client
            .post_multipart("/audio/transcriptions", form)
            .await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        let audio_response: AudioResponse = response.json().await?;
        Ok(audio_response)
    }

    async fn text_to_speech(
        &self,
        mut request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        request.model = self.map_model(&request.model);

        let response = self.client.post_json_raw("/audio/speech", &request).await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|ct| ct.to_str().ok())
            .unwrap_or("audio/mpeg")
            .to_string();

        let audio_bytes = response.bytes().await?;

        Ok(SpeechResponse {
            audio: audio_bytes.to_vec(),
            content_type,
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
        let start = Instant::now();
        let response = self.client.get_json::<serde_json::Value>("/models").await;
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
