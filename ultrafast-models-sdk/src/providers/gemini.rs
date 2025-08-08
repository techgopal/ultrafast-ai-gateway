use crate::error::ProviderError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
    ImageRequest, ImageResponse, Role, SpeechRequest, SpeechResponse, StreamChunk, Usage,
};
use crate::providers::{HealthStatus, Provider, ProviderConfig, ProviderHealth, StreamResult};
use async_stream::stream;
use reqwest::Client;
use std::collections::HashMap;
use std::time::Instant;
// use futures::StreamExt;

pub struct GeminiProvider {
    client: Client,
    config: ProviderConfig,
    base_url: String,
}

impl GeminiProvider {
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
            .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta".to_string());

        Ok(Self {
            client,
            config,
            base_url,
        })
    }

    fn build_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();

        headers.insert("Content-Type", "application/json".parse().unwrap());

        // Add API key if provided
        headers.insert("x-goog-api-key", self.config.api_key.parse().unwrap());

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
impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        false // Gemini doesn't support function calling yet
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "gemini-1.5-pro".to_string(),
            "gemini-1.5-pro-latest".to_string(),
            "gemini-1.5-flash".to_string(),
            "gemini-1.5-flash-latest".to_string(),
            "gemini-1.0-pro".to_string(),
            "gemini-1.0-pro-vision".to_string(),
        ]
    }

    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let model = self.map_model(&request.model);
        let url = format!("{}/models/{}:generateContent", self.base_url, model);
        let headers = self.build_headers();

        // Convert OpenAI format to Gemini format
        let gemini_request = self.convert_to_gemini_format(request);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&gemini_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let gemini_response: GeminiResponse = response.json().await?;
        let chat_response = self.convert_from_gemini_format(gemini_response);
        Ok(chat_response)
    }

    async fn stream_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<StreamResult, ProviderError> {
        let model = self.map_model(&request.model);
        let url = format!("{}/models/{}:streamGenerateContent", self.base_url, model);
        let headers = self.build_headers();

        // Convert to Gemini streaming format
        let gemini_request = self.convert_to_gemini_format(request);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&gemini_request)
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

                            if !line.is_empty() {
                                // Try to parse as Gemini streaming response and convert to OpenAI format
                                match serde_json::from_str::<serde_json::Value>(&line) {
                                    Ok(gemini_chunk) => {
                                        if let Some(candidates) = gemini_chunk.get("candidates")
                                            .and_then(|c| c.as_array()) {
                                            for candidate in candidates {
                                                if let Some(content) = candidate.get("content")
                                                    .and_then(|c| c.get("parts"))
                                                    .and_then(|p| p.as_array())
                                                    .and_then(|parts| parts.first())
                                                    .and_then(|part| part.get("text"))
                                                    .and_then(|t| t.as_str()) {

                                                    let stream_chunk = StreamChunk {
                                                        id: "gemini-stream".to_string(),
                                                        object: "chat.completion.chunk".to_string(),
                                                        created: chrono::Utc::now().timestamp() as u64,
                                                        model: model.clone(),
                                                        choices: vec![crate::models::StreamChoice {
                                                            index: 0,
                                                            delta: crate::models::Delta {
                                                                role: Some(Role::Assistant),
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
                                    Err(_) => {
                                        // Skip invalid JSON lines
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Stream error: {}", e);
                        break;
                    }
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
        let url = format!("{}/models/{}:embedContent", self.base_url, model);
        let headers = self.build_headers();

        // Convert to Gemini embedding format
        let gemini_request = self.convert_to_gemini_embedding_format(request);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&gemini_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let gemini_response: GeminiEmbeddingResponse = response.json().await?;
        let embedding_response = self.convert_from_gemini_embedding_format(gemini_response);
        Ok(embedding_response)
    }

    async fn image_generation(
        &self,
        _request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Image generation not supported by Gemini".to_string(),
        })
    }

    async fn audio_transcription(
        &self,
        _request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Audio transcription not supported by Gemini".to_string(),
        })
    }

    async fn text_to_speech(
        &self,
        _request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Text-to-speech not supported by Gemini".to_string(),
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
        let start = Instant::now();

        // Try to list models as a health check
        let url = format!("{}/models", self.base_url);
        let headers = self.build_headers();

        let response = self.client.get(&url).headers(headers).send().await?;

        let latency = start.elapsed();
        let is_healthy = response.status().is_success();

        let mut details = HashMap::new();
        details.insert("status".to_string(), response.status().to_string());

        Ok(ProviderHealth {
            status: if is_healthy {
                HealthStatus::Healthy
            } else {
                HealthStatus::Unhealthy
            },
            latency_ms: Some(latency.as_millis() as u64),
            error_rate: if is_healthy { 0.0 } else { 1.0 },
            last_check: chrono::Utc::now(),
            details,
        })
    }
}

impl GeminiProvider {
    fn convert_to_gemini_format(&self, request: ChatRequest) -> GeminiRequest {
        let mut contents = Vec::new();

        for message in &request.messages {
            let role = match message.role {
                Role::User => "user",
                Role::Assistant => "model",
                Role::System => "user", // Gemini doesn't have system messages, treat as user
                Role::Tool => "user",   // Gemini doesn't have tool messages, treat as user
            };

            let parts = vec![GeminiPart {
                text: message.content.clone(),
            }];

            contents.push(GeminiContent {
                role: role.to_string(),
                parts,
            });
        }

        let generation_config = GeminiGenerationConfig {
            temperature: request.temperature,
            max_output_tokens: request.max_tokens.map(|t| t as i32),
            top_p: request.top_p,
            top_k: None,
        };

        GeminiRequest {
            contents,
            generation_config: Some(generation_config),
        }
    }

    fn convert_from_gemini_format(&self, response: GeminiResponse) -> ChatResponse {
        let mut choices = Vec::new();

        for (index, candidate) in response.candidates.iter().enumerate() {
            let content = candidate
                .content
                .parts
                .iter()
                .map(|part| part.text.clone())
                .collect::<Vec<String>>()
                .join("");

            choices.push(crate::models::Choice {
                index: index as u32,
                message: crate::models::Message {
                    role: Role::Assistant,
                    content,
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            });
        }

        let usage = response.usage_metadata.map(|u| Usage {
            prompt_tokens: u.prompt_token_count,
            completion_tokens: u.candidates_token_count,
            total_tokens: u.total_token_count,
        });

        ChatResponse {
            id: "gemini-response".to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp() as u64,
            model: "gemini-1.5-pro".to_string(),
            choices,
            usage,
            system_fingerprint: None,
        }
    }

    fn convert_to_gemini_embedding_format(
        &self,
        request: EmbeddingRequest,
    ) -> GeminiEmbeddingRequest {
        let text = match &request.input {
            crate::models::EmbeddingInput::String(s) => s.clone(),
            crate::models::EmbeddingInput::StringArray(arr) => arr.join(" "),
            crate::models::EmbeddingInput::TokenArray(_) => "".to_string(), // Not supported by Gemini
            crate::models::EmbeddingInput::TokenArrayArray(_) => "".to_string(), // Not supported by Gemini
        };

        let content = GeminiEmbeddingContent {
            parts: vec![GeminiEmbeddingPart { text }],
        };

        GeminiEmbeddingRequest {
            content: Some(content),
        }
    }

    fn convert_from_gemini_embedding_format(
        &self,
        response: GeminiEmbeddingResponse,
    ) -> EmbeddingResponse {
        let embeddings = response.embedding.values;

        EmbeddingResponse {
            object: "list".to_string(),
            data: vec![crate::models::Embedding {
                object: "embedding".to_string(),
                embedding: embeddings,
                index: 0,
            }],
            model: "text-embedding-004".to_string(),
            usage: Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        }
    }
}

// Gemini API request/response structures
#[derive(serde::Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(serde::Serialize)]
struct GeminiGenerationConfig {
    temperature: Option<f32>,
    max_output_tokens: Option<i32>,
    top_p: Option<f32>,
    top_k: Option<i32>,
}

#[derive(serde::Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    usage_metadata: Option<GeminiUsage>,
}

#[derive(serde::Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct GeminiUsage {
    prompt_token_count: u32,
    candidates_token_count: u32,
    total_token_count: u32,
}

#[derive(serde::Serialize)]
struct GeminiEmbeddingRequest {
    content: Option<GeminiEmbeddingContent>,
}

#[derive(serde::Serialize)]
struct GeminiEmbeddingContent {
    parts: Vec<GeminiEmbeddingPart>,
}

#[derive(serde::Serialize)]
struct GeminiEmbeddingPart {
    text: String,
}

#[derive(serde::Deserialize)]
struct GeminiEmbeddingResponse {
    embedding: GeminiEmbedding,
}

#[derive(serde::Deserialize)]
struct GeminiEmbedding {
    values: Vec<f32>,
}
