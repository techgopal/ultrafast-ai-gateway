use crate::error::ProviderError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
    ImageRequest, ImageResponse, SpeechRequest, SpeechResponse, StreamChunk,
};
use crate::providers::{HealthStatus, Provider, ProviderConfig, ProviderHealth, StreamResult};
use async_stream::stream;
use reqwest::Client;
use std::collections::HashMap;
use std::time::Instant;

pub struct AzureOpenAIProvider {
    client: Client,
    config: ProviderConfig,
    base_url: String,
    api_version: String,
}

impl AzureOpenAIProvider {
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
            .unwrap_or_else(|| "https://api.openai.azure.com".to_string());

        let api_version = config
            .headers
            .get("api-version")
            .cloned()
            .unwrap_or_else(|| "2024-02-15-preview".to_string());

        Ok(Self {
            client,
            config,
            base_url,
            api_version,
        })
    }

    fn build_url(&self, endpoint: &str, deployment_name: Option<&str>) -> String {
        let deployment = deployment_name.unwrap_or("gpt-35-turbo");
        format!(
            "{}/openai/deployments/{}/{}?api-version={}",
            self.base_url, deployment, endpoint, self.api_version
        )
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
impl Provider for AzureOpenAIProvider {
    fn name(&self) -> &str {
        "azure-openai"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        true
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "gpt-4".to_string(),
            "gpt-4-turbo".to_string(),
            "gpt-4-turbo-preview".to_string(),
            "gpt-35-turbo".to_string(),
            "gpt-35-turbo-16k".to_string(),
            "text-embedding-ada-002".to_string(),
            "text-embedding-3-small".to_string(),
            "text-embedding-3-large".to_string(),
            "dall-e-2".to_string(),
            "dall-e-3".to_string(),
            "whisper-1".to_string(),
            "tts-1".to_string(),
            "tts-1-hd".to_string(),
        ]
    }

    async fn chat_completion(
        &self,
        mut request: ChatRequest,
    ) -> Result<ChatResponse, ProviderError> {
        request.model = self.map_model(&request.model);
        let url = self.build_url("chat/completions", Some(&request.model));
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let chat_response: ChatResponse = response.json().await?;
        Ok(chat_response)
    }

    async fn stream_chat_completion(
        &self,
        mut request: ChatRequest,
    ) -> Result<StreamResult, ProviderError> {
        request.model = self.map_model(&request.model);
        request.stream = Some(true);

        let url = self.build_url("chat/completions", Some(&request.model));
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&request)
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
        let url = self.build_url("embeddings", Some(&request.model));
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let embedding_response: EmbeddingResponse = response.json().await?;
        Ok(embedding_response)
    }

    async fn image_generation(
        &self,
        mut request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        if let Some(ref model) = request.model {
            request.model = Some(self.map_model(model));
        }

        let model = request.model.as_deref().unwrap_or("dall-e-3");
        let url = self.build_url("images/generations", Some(model));
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let image_response: ImageResponse = response.json().await?;
        Ok(image_response)
    }

    async fn audio_transcription(
        &self,
        mut request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        request.model = self.map_model(&request.model);
        let url = self.build_url("audio/transcriptions", Some(&request.model));
        let headers = self.build_headers();

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
            .post(&url)
            .headers(headers)
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let audio_response: AudioResponse = response.json().await?;
        Ok(audio_response)
    }

    async fn text_to_speech(
        &self,
        mut request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        request.model = self.map_model(&request.model);
        let url = self.build_url("audio/speech", Some(&request.model));
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
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

        // Use a basic models list request instead of chat completion for health check
        let url = format!(
            "{}/openai/models?api-version={}",
            self.base_url, self.api_version
        );
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
