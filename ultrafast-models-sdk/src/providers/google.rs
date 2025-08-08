use crate::error::ProviderError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
    ImageRequest, ImageResponse, SpeechRequest, SpeechResponse, StreamChunk,
};
use crate::providers::{HealthStatus, Provider, ProviderConfig, ProviderHealth, StreamResult};
use async_stream::stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

pub struct GoogleVertexAIProvider {
    client: Client,
    config: ProviderConfig,
    base_url: String,
    #[allow(dead_code)]
    project_id: String,
    location: String,
}

impl GoogleVertexAIProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| ProviderError::Configuration {
                message: format!("Failed to create HTTP client: {e}"),
            })?;

        let project_id = config.headers.get("project-id").cloned().ok_or_else(|| {
            ProviderError::Configuration {
                message: "project-id is required for Google Vertex AI".to_string(),
            }
        })?;

        let location = config
            .headers
            .get("location")
            .cloned()
            .unwrap_or_else(|| "us-central1".to_string());

        let base_url = config.base_url.clone().unwrap_or_else(|| {
            format!("https://{location}-aiplatform.googleapis.com/v1/projects/{project_id}")
        });

        Ok(Self {
            client,
            config,
            base_url,
            project_id,
            location,
        })
    }

    fn build_url(&self, endpoint: &str) -> String {
        format!(
            "{}/locations/{}/publishers/google/models/{}:predict",
            self.base_url, self.location, endpoint
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
            .unwrap_or_else(|| {
                // Map common model names to Vertex AI equivalents
                match model {
                    "gpt-4" | "gpt-3.5-turbo" => "chat-bison".to_string(),
                    "text-embedding-ada-002" => "textembedding-gecko".to_string(),
                    _ => model.to_string(),
                }
            })
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
impl Provider for GoogleVertexAIProvider {
    fn name(&self) -> &str {
        "google-vertex-ai"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_function_calling(&self) -> bool {
        false
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "chat-bison".to_string(),
            "chat-bison-32k".to_string(),
            "text-bison".to_string(),
            "text-bison-32k".to_string(),
            "gemini-pro".to_string(),
            "gemini-pro-vision".to_string(),
            "textembedding-gecko".to_string(),
            "textembedding-gecko-multilingual".to_string(),
        ]
    }

    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let model = self.map_model(&request.model);
        let url = self.build_url(&model);
        let headers = self.build_headers();

        // Convert OpenAI format to Vertex AI format
        let vertex_request = self.convert_to_vertex_format(request);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&vertex_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let vertex_response: VertexAIResponse = response.json().await?;
        let chat_response = self.convert_from_vertex_format(vertex_response);
        Ok(chat_response)
    }

    async fn stream_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<StreamResult, ProviderError> {
        let model = self.map_model(&request.model);
        let url = format!(
            "{}/locations/{}/publishers/google/models/{}:streamGenerateContent",
            self.base_url, self.location, model
        );
        let headers = self.build_headers();

        // Convert to Vertex AI streaming format
        let vertex_request = self.convert_to_vertex_streaming_format(request);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&vertex_request)
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
                                // Try to parse as Vertex AI streaming response and convert to OpenAI format
                                match serde_json::from_str::<serde_json::Value>(&line) {
                                    Ok(vertex_chunk) => {
                                        if let Some(candidates) = vertex_chunk.get("candidates")
                                            .and_then(|c| c.as_array()) {
                                            for candidate in candidates {
                                                if let Some(content) = candidate.get("content")
                                                    .and_then(|c| c.get("parts"))
                                                    .and_then(|p| p.as_array())
                                                    .and_then(|parts| parts.first())
                                                    .and_then(|part| part.get("text"))
                                                    .and_then(|t| t.as_str()) {

                                                    let stream_chunk = StreamChunk {
                                                        id: "vertex-stream".to_string(),
                                                        object: "chat.completion.chunk".to_string(),
                                                        created: chrono::Utc::now().timestamp() as u64,
                                                        model: "chat-bison".to_string(),
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
        let url = self.build_url(&model);
        let headers = self.build_headers();

        // Convert to Vertex AI embedding format
        let vertex_embedding_request = VertexAIEmbeddingRequest {
            instances: vec![VertexAIEmbeddingInstance {
                content: match request.input {
                    crate::models::EmbeddingInput::String(s) => s,
                    _ => {
                        return Err(ProviderError::Configuration {
                            message:
                                "Only string input is supported for Google Vertex AI embeddings"
                                    .to_string(),
                        })
                    }
                },
            }],
        };

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&vertex_embedding_request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let vertex_response: VertexAIEmbeddingResponse = response.json().await?;

        // Convert back to OpenAI format
        let embedding_response = EmbeddingResponse {
            object: "list".to_string(),
            data: vertex_response
                .predictions
                .into_iter()
                .map(|pred| crate::models::Embedding {
                    object: "embedding".to_string(),
                    embedding: pred.embeddings.values,
                    index: 0,
                })
                .collect(),
            model: request.model.clone(),
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
            message: "Google Vertex AI does not support image generation via this API".to_string(),
        })
    }

    async fn audio_transcription(
        &self,
        _request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Google Vertex AI does not support audio transcription via this API"
                .to_string(),
        })
    }

    async fn text_to_speech(
        &self,
        _request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Google Vertex AI does not support text-to-speech via this API".to_string(),
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
        let start = Instant::now();

        // Use a simple models list request for health check
        let url = format!(
            "{}/locations/{}/publishers/google/models",
            self.base_url, self.location
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

impl GoogleVertexAIProvider {
    fn convert_to_vertex_streaming_format(&self, request: ChatRequest) -> VertexAIStreamRequest {
        let contents = request
            .messages
            .into_iter()
            .map(|msg| {
                VertexAIContent {
                    role: match msg.role {
                        crate::models::Role::System => "user".to_string(), // Vertex AI doesn't have system role
                        crate::models::Role::User => "user".to_string(),
                        crate::models::Role::Assistant => "model".to_string(),
                        crate::models::Role::Tool => "user".to_string(),
                    },
                    parts: vec![VertexAIPart { text: msg.content }],
                }
            })
            .collect();

        let generation_config = VertexAIGenerationConfig {
            temperature: request.temperature,
            max_output_tokens: request.max_tokens.map(|t| t as i32),
            top_p: request.top_p,
            top_k: None,
        };

        VertexAIStreamRequest {
            contents,
            generation_config: Some(generation_config),
        }
    }

    fn convert_to_vertex_format(&self, request: ChatRequest) -> VertexAIRequest {
        let messages = request
            .messages
            .into_iter()
            .map(|msg| VertexAIMessage {
                author: match msg.role {
                    crate::models::Role::System => "system".to_string(),
                    crate::models::Role::User => "user".to_string(),
                    crate::models::Role::Assistant => "assistant".to_string(),
                    crate::models::Role::Tool => "user".to_string(),
                },
                content: msg.content,
            })
            .collect();

        let parameters = VertexAIParameters {
            temperature: request.temperature.unwrap_or(0.7),
            max_output_tokens: request.max_tokens.unwrap_or(1024) as i32,
            top_p: request.top_p,
            top_k: None,
        };

        VertexAIRequest {
            instances: vec![VertexAIInstance { messages }],
            parameters: Some(parameters),
        }
    }

    fn convert_from_vertex_format(&self, response: VertexAIResponse) -> ChatResponse {
        let choices = response
            .predictions
            .into_iter()
            .flat_map(|pred| {
                pred.candidates
                    .into_iter()
                    .map(|candidate| crate::models::Choice {
                        index: 0,
                        message: crate::models::Message {
                            role: crate::models::Role::Assistant,
                            content: candidate.content,
                            name: None,
                            tool_calls: None,
                            tool_call_id: None,
                        },
                        finish_reason: Some("stop".to_string()),
                        logprobs: None,
                    })
            })
            .collect();

        ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp() as u64,
            model: "chat-bison".to_string(),
            choices,
            usage: None,
            system_fingerprint: None,
        }
    }
}

// Vertex AI specific data structures
#[derive(Debug, Serialize, Deserialize)]
struct VertexAIRequest {
    instances: Vec<VertexAIInstance>,
    parameters: Option<VertexAIParameters>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIInstance {
    messages: Vec<VertexAIMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIMessage {
    author: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIParameters {
    temperature: f32,
    max_output_tokens: i32,
    top_p: Option<f32>,
    top_k: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIResponse {
    predictions: Vec<VertexAIPrediction>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIPrediction {
    candidates: Vec<VertexAICandidate>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAICandidate {
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIEmbeddingRequest {
    instances: Vec<VertexAIEmbeddingInstance>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIEmbeddingInstance {
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIEmbeddingResponse {
    predictions: Vec<VertexAIEmbeddingPrediction>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIEmbeddingPrediction {
    embeddings: VertexAIEmbeddings,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIEmbeddings {
    values: Vec<f32>,
}

// Streaming-specific structures
#[derive(Debug, Serialize, Deserialize)]
struct VertexAIStreamRequest {
    contents: Vec<VertexAIContent>,
    generation_config: Option<VertexAIGenerationConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIContent {
    role: String,
    parts: Vec<VertexAIPart>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIPart {
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VertexAIGenerationConfig {
    temperature: Option<f32>,
    max_output_tokens: Option<i32>,
    top_p: Option<f32>,
    top_k: Option<i32>,
}
