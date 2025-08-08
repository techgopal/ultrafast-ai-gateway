//! # HTTP Request Handlers Module
//!
//! This module contains all HTTP request handlers for the Ultrafast Gateway API.
//! It provides endpoints for chat completions, embeddings, image generation,
//! audio processing, and administrative functions.
//!
//! ## Overview
//!
//! The handlers module provides:
//! - **Core API Endpoints**: Chat completions, embeddings, image generation
//! - **Streaming Support**: Server-sent events for real-time responses
//! - **Caching Integration**: Automatic response caching and retrieval
//! - **Metrics Collection**: Performance tracking and cost calculation
//! - **Admin Endpoints**: Health checks, metrics, and configuration
//! - **Dashboard Support**: Real-time monitoring dashboard
//!
//! ## API Endpoints
//!
//! ### Core API Endpoints
//!
//! - `POST /v1/chat/completions` - Chat completion API with streaming support
//! - `POST /v1/embeddings` - Text embedding generation
//! - `POST /v1/images/generations` - Image generation from text prompts
//! - `POST /v1/audio/transcriptions` - Audio transcription
//! - `POST /v1/audio/speech` - Text-to-speech conversion
//! - `GET /v1/models` - List available models
//!
//! ### Admin Endpoints
//!
//! - `GET /health` - Service health check
//! - `GET /metrics` - Performance metrics in JSON format
//! - `GET /metrics/prometheus` - Prometheus-compatible metrics
//! - `GET /admin/providers` - Provider status and health
//! - `GET /admin/config` - Current configuration status
//! - `GET /admin/circuit-breaker` - Circuit breaker metrics
//!
//! ### Dashboard Endpoints
//!
//! - `GET /dashboard` - Real-time monitoring dashboard
//! - `GET /dashboard.js` - Dashboard JavaScript
//! - `GET /dashboard.css` - Dashboard stylesheets
//! - `GET /ws/dashboard` - WebSocket for real-time updates
//!
//! ## Request Flow
//!
//! Each request follows this general flow:
//!
//! 1. **Authentication**: Validate API key or JWT token
//! 2. **Rate Limiting**: Check user and provider rate limits
//! 3. **Caching**: Check for cached responses
//! 4. **Provider Selection**: Route to appropriate provider
//! 5. **Request Processing**: Execute the request
//! 6. **Response Caching**: Cache successful responses
//! 7. **Metrics Collection**: Track performance and costs
//!
//! ## Streaming Support
//!
//! The gateway supports streaming responses for chat completions:
//!
//! - **Server-Sent Events**: Real-time token streaming
//! - **Chunked Responses**: Progressive response delivery
//! - **Error Handling**: Graceful error propagation
//! - **Backpressure**: Automatic flow control
//!
//! ## Caching Strategy
//!
//! Responses are cached based on:
//!
//! - **Request Hash**: Unique cache keys for each request
//! - **TTL Calculation**: Dynamic TTL based on response time
//! - **Cache Invalidation**: Automatic expiration and cleanup
//! - **Cache Miss Handling**: Fallback to provider requests
//!
//! ## Cost Tracking
//!
//! The gateway tracks costs for each provider:
//!
//! - **Token Counting**: Input and output token tracking
//! - **Provider Pricing**: Per-provider cost calculation
//! - **Cost Aggregation**: Total cost tracking
//! - **Cost Reporting**: Cost metrics in responses
//!
//! ## Error Handling
//!
//! All handlers include comprehensive error handling:
//!
//! - **Validation Errors**: Request validation and sanitization
//! - **Provider Errors**: Graceful provider error handling
//! - **Rate Limit Errors**: Proper rate limit responses
//! - **Cache Errors**: Fallback on cache failures
//! - **Network Errors**: Retry logic and timeouts
//!
//! ## Performance Optimization
//!
//! The handlers include several performance optimizations:
//!
//! - **JSON Optimization**: Request payload optimization
//! - **Response Streaming**: Efficient streaming responses
//! - **Connection Pooling**: Reusable HTTP connections
//! - **Memory Management**: Efficient memory usage
//! - **Concurrent Processing**: Async request handling

use crate::dashboard;
use crate::dashboard::websocket::{DashboardWebSocketQuery, WebSocketManager};
use crate::gateway_error::GatewayError;
use crate::server::AppState;
use axum::response::sse::{Event, Sse};
use axum::{
    body::Body,
    extract::{Query, State, WebSocketUpgrade},
    http::{Response, StatusCode},
    response::{Html, Json},
};
use futures::StreamExt;
use serde_json::{json, Value};
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;
use ultrafast_models_sdk::error::ProviderError;
use ultrafast_models_sdk::models::{
    AudioRequest, AudioResponse, ChatRequest, EmbeddingRequest, EmbeddingResponse, ImageRequest,
    ImageResponse, SpeechRequest, SpeechResponse,
};

/// Handle chat completion requests with caching and streaming support.
///
/// This endpoint processes chat completion requests, supports both streaming
/// and non-streaming responses, and includes automatic caching and cost tracking.
///
/// # Arguments
///
/// * `state` - Application state containing client and cache manager
/// * `request` - Chat completion request with messages and parameters
///
/// # Returns
///
/// Returns a JSON response with the completion or a streaming response.
///
/// # Errors
///
/// Returns a GatewayError if:
/// - Request validation fails
/// - Provider selection fails
/// - Cache operations fail
/// - Provider request fails
///
/// # Example
///
/// ```bash
/// curl -X POST http://localhost:3000/v1/chat/completions \
///   -H "Content-Type: application/json" \
///   -H "Authorization: Bearer sk-..." \
///   -d '{
///     "model": "gpt-4",
///     "messages": [{"role": "user", "content": "Hello!"}],
///     "stream": false
///   }'
/// ```
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Result<Response<Body>, GatewayError> {
    // Check if this is a streaming request
    if request.stream.unwrap_or(false) {
        // Handle streaming requests with SSE
        return handle_streaming_chat_completions(State(state), Json(request)).await;
    }

    // Phase 4: Optimize request payload (request-side only); keep responses intact for compatibility
    let request_json = serde_json::to_value(&request)?;
    let optimized_request_json =
        crate::json_optimization::JsonOptimizer::optimize_request_payload(&request_json);
    let optimized_request: ChatRequest = serde_json::from_value(optimized_request_json)?;

    // Check cache first
    let cache_key = if !optimized_request.stream.unwrap_or(false) {
        Some(ultrafast_models_sdk::cache::CacheKeyBuilder::build_chat_key(&optimized_request))
    } else {
        None
    };

    if let Some(cache_key) = &cache_key {
        if let Some(cached_response) = state.cache_manager.get(cache_key).await {
            tracing::debug!("Cache hit for chat completion");
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&cached_response)?))
                .unwrap());
        }
    }

    // Route to appropriate provider using the client
    let start_time = std::time::Instant::now();
    let result = state
        .client
        .chat_completion(optimized_request.clone())
        .await;
    let latency = start_time.elapsed();

    // Extract provider and token information from response
    let (provider_name, input_tokens, output_tokens, cost_usd) = match &result {
        Ok(response) => {
            let provider = state.client.get_last_used_provider().await;
            let input_tokens = response.usage.as_ref().map(|u| u.prompt_tokens);
            let output_tokens = response.usage.as_ref().map(|u| u.completion_tokens);

            // Calculate cost based on provider and token counts
            let cost = if let (Some(provider_name), Some(input), Some(output)) =
                (provider.as_ref(), input_tokens, output_tokens)
            {
                // Simple cost calculation based on provider
                let cost = match provider_name.as_str() {
                    "anthropic" => {
                        // Claude-3 pricing: $0.015 per 1K input tokens, $0.075 per 1K output tokens
                        let input_cost = (input as f64 / 1000.0) * 0.015;
                        let output_cost = (output as f64 / 1000.0) * 0.075;
                        input_cost + output_cost
                    }
                    "openai" => {
                        // GPT-4 pricing: $0.03 per 1K input tokens, $0.06 per 1K output tokens
                        let input_cost = (input as f64 / 1000.0) * 0.03;
                        let output_cost = (output as f64 / 1000.0) * 0.06;
                        input_cost + output_cost
                    }
                    "google-vertex-ai" => {
                        // Google Vertex AI pricing (includes Gemini models)
                        // Gemini Pro: $0.0005 per 1K input tokens, $0.0015 per 1K output tokens
                        // Gemini Pro Vision: $0.0025 per 1K input tokens, $0.0075 per 1K output tokens
                        // Using Gemini Pro pricing as default
                        let input_cost = (input as f64 / 1000.0) * 0.0005;
                        let output_cost = (output as f64 / 1000.0) * 0.0015;
                        input_cost + output_cost
                    }
                    "gemini" => {
                        // Gemini API pricing (direct API, not Vertex AI)
                        // Gemini 1.5 Pro: $0.0035 per 1M input tokens, $0.0105 per 1M output tokens
                        // Gemini 1.5 Flash: $0.000075 per 1M input tokens, $0.0003 per 1M output tokens
                        // Using Gemini 1.5 Pro pricing as default
                        let input_cost = (input as f64 / 1_000_000.0) * 0.0035;
                        let output_cost = (output as f64 / 1_000_000.0) * 0.0105;
                        input_cost + output_cost
                    }
                    "ollama" => {
                        // Ollama is free
                        0.0
                    }
                    _ => {
                        // Default cost for unknown providers
                        0.0
                    }
                };
                Some(cost)
            } else {
                None
            };

            (provider, input_tokens, output_tokens, cost)
        }
        Err(_) => (None, None, None, None),
    };

    // Update metrics with real data
    crate::metrics::record_request(
        crate::metrics::RequestMetricsBuilder::new(
            "POST".to_string(),
            "/v1/chat/completions".to_string(),
            if result.is_ok() { 200 } else { 500 },
            latency,
        )
        .provider(provider_name.unwrap_or_default())
        .model(optimized_request.model.clone())
        .input_tokens(input_tokens.unwrap_or_default())
        .output_tokens(output_tokens.unwrap_or_default())
        .cost_usd(cost_usd.unwrap_or_default())
        .user_id(optimized_request.user.clone().unwrap_or_default())
        .build(),
    )
    .await;

    match result {
        Ok(response) => {
            // Cache successful response
            if let Some(cache_key) = &cache_key {
                let ttl = determine_cache_ttl(&optimized_request, latency);
                state
                    .cache_manager
                    .set(cache_key, serde_json::to_value(&response)?, Some(ttl))
                    .await;
            }

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&response)?))
                .unwrap())
        }
        Err(e) => {
            tracing::error!("Provider error: {}", e);
            Err(GatewayError::Provider(
                ultrafast_models_sdk::error::ProviderError::ServiceUnavailable,
            ))
        }
    }
}

async fn handle_streaming_chat_completions(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Result<Response<Body>, GatewayError> {
    // Phase 4: Optimize request payload
    let request_json = serde_json::to_value(&request)?;
    let optimized_request_json =
        crate::json_optimization::JsonOptimizer::optimize_request_payload(&request_json);
    let optimized_request: ChatRequest = serde_json::from_value(optimized_request_json)?;

    // Route to appropriate provider using the client
    let start_time = std::time::Instant::now();
    let stream_result = state
        .client
        .stream_chat_completion(optimized_request.clone())
        .await;
    let latency = start_time.elapsed();

    match stream_result {
        Ok(stream) => {
            // Create a channel for streaming events
            let (tx, rx) = tokio::sync::mpsc::channel::<String>(100);

            // Spawn a task to handle the stream
            let mut stream = stream;
            tokio::spawn(async move {
                let mut total_tokens = 0;
                let mut content = String::new();

                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            // Convert StreamChunk to SSE format
                            let event_data = serde_json::to_string(&chunk).unwrap_or_default();
                            let sse_event = format!("data: {event_data}\n\n");

                            // Track content for metrics
                            if let Some(choice) = chunk.choices.first() {
                                if let Some(text) = &choice.delta.content {
                                    content.push_str(text);
                                }
                            }

                            // Track tokens (StreamChunk doesn't have usage field)
                            total_tokens = content.len() as u32;

                            if (tx.send(sse_event).await).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Stream error: {}", e);
                            let error_event = format!("data: {{\"error\": \"{e}\"}}\n\n");
                            let _ = tx.send(error_event).await;
                            break;
                        }
                    }
                }

                // Send final event
                let final_event = "data: [DONE]\n\n";
                let _ = tx.send(final_event.to_string()).await;

                // Update metrics
                let provider = state.client.get_last_used_provider().await;
                crate::metrics::record_request(
                    crate::metrics::RequestMetricsBuilder::new(
                        "POST".to_string(),
                        "/v1/chat/completions".to_string(),
                        200,
                        latency,
                    )
                    .provider(provider.unwrap_or_default())
                    .model(optimized_request.model.clone())
                    .input_tokens(total_tokens)
                    .output_tokens(total_tokens)
                    .cost_usd(0.0) // Cost calculation would be done differently for streaming
                    .user_id(optimized_request.user.clone().unwrap_or_default())
                    .build(),
                )
                .await;
            });

            // For now, let's use a simpler approach with a custom stream
            let body = Body::from_stream(async_stream::stream! {
                let mut rx = rx;
                while let Some(event) = rx.recv().await {
                    yield Ok::<axum::body::Bytes, std::io::Error>(event.into());
                }
            });

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .header("connection", "keep-alive")
                .body(body)
                .unwrap())
        }
        Err(e) => {
            tracing::error!("Stream initialization error: {}", e);
            Err(GatewayError::Provider(
                ultrafast_models_sdk::error::ProviderError::ServiceUnavailable,
            ))
        }
    }
}

pub async fn stream_chat_completions(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, GatewayError> {
    // Phase 4: Optimize request payload
    let request_json = serde_json::to_value(&request)?;
    let optimized_request_json =
        crate::json_optimization::JsonOptimizer::optimize_request_payload(&request_json);
    let optimized_request: ChatRequest = serde_json::from_value(optimized_request_json)?;

    // Route to appropriate provider using the client
    let start_time = std::time::Instant::now();
    let stream_result = state
        .client
        .stream_chat_completion(optimized_request.clone())
        .await;
    let latency = start_time.elapsed();

    match stream_result {
        Ok(stream) => {
            // Create a channel for streaming events
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(100);

            // Spawn a task to handle the stream
            let mut stream = stream;
            tokio::spawn(async move {
                let mut total_tokens = 0;
                let mut content = String::new();

                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            // Convert StreamChunk to SSE Event
                            let event_data = serde_json::to_string(&chunk).unwrap_or_default();
                            let event = Event::default().data(event_data);

                            // Track content for metrics
                            if let Some(choice) = chunk.choices.first() {
                                if let Some(text) = &choice.delta.content {
                                    content.push_str(text);
                                }
                            }

                            // Track tokens (StreamChunk doesn't have usage field)
                            // We'll track content length instead
                            total_tokens = content.len() as u32;

                            if (tx.send(Ok(event)).await).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Stream error: {}", e);
                            let error_event =
                                Event::default().data(format!("{{\"error\": \"{e}\"}}"));
                            let _ = tx.send(Ok(error_event)).await;
                            break;
                        }
                    }
                }

                // Send final event
                let final_event = Event::default().data("[DONE]");
                let _ = tx.send(Ok(final_event)).await;

                // Update metrics
                let provider = state.client.get_last_used_provider().await;
                crate::metrics::record_request(
                    crate::metrics::RequestMetricsBuilder::new(
                        "POST".to_string(),
                        "/v1/chat/completions".to_string(),
                        200,
                        latency,
                    )
                    .provider(provider.unwrap_or_default())
                    .model(optimized_request.model.clone())
                    .input_tokens(total_tokens)
                    .output_tokens(total_tokens)
                    .cost_usd(0.0) // Cost calculation would be done differently for streaming
                    .user_id(optimized_request.user.clone().unwrap_or_default())
                    .build(),
                )
                .await;
            });

            Ok(Sse::new(ReceiverStream::new(rx)))
        }
        Err(e) => {
            tracing::error!("Stream initialization error: {}", e);
            Err(GatewayError::Provider(
                ultrafast_models_sdk::error::ProviderError::ServiceUnavailable,
            ))
        }
    }
}

pub async fn completions(
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> Result<Json<Value>, GatewayError> {
    // Convert legacy completions format to chat completions format
    let chat_request = convert_legacy_completion_to_chat(request)?;

    // Use the existing chat completions logic
    let response = state.client.chat_completion(chat_request).await?;

    // Convert chat response back to legacy completions format
    let legacy_response = convert_chat_to_legacy_completion(response)?;

    Ok(Json(legacy_response))
}

pub async fn embeddings(
    State(state): State<AppState>,
    Json(request): Json<EmbeddingRequest>,
) -> Result<Json<EmbeddingResponse>, GatewayError> {
    // Route to appropriate provider using the client
    let start_time = std::time::Instant::now();
    let result = state.client.embedding(request).await;
    let latency = start_time.elapsed();

    // Record metrics
    crate::metrics::record_request(
        crate::metrics::RequestMetricsBuilder::new(
            "POST".to_string(),
            "/v1/embeddings".to_string(),
            200,
            latency,
        )
        .provider(
            state
                .client
                .get_last_used_provider()
                .await
                .unwrap_or_default(),
        )
        .build(),
    )
    .await;

    match result {
        Ok(response) => Ok(Json(response)),
        Err(e) => Err(GatewayError::Provider(ProviderError::Configuration {
            message: format!("Embedding request failed: {e}"),
        })),
    }
}

pub async fn image_generations(
    State(state): State<AppState>,
    Json(request): Json<ImageRequest>,
) -> Result<Json<ImageResponse>, GatewayError> {
    // Route to appropriate provider using the client
    let start_time = std::time::Instant::now();
    let result = state.client.image_generation(request).await;
    let latency = start_time.elapsed();

    // Record metrics
    crate::metrics::record_request(
        crate::metrics::RequestMetricsBuilder::new(
            "POST".to_string(),
            "/v1/images/generations".to_string(),
            200,
            latency,
        )
        .provider(
            state
                .client
                .get_last_used_provider()
                .await
                .unwrap_or_default(),
        )
        .build(),
    )
    .await;

    match result {
        Ok(response) => Ok(Json(response)),
        Err(e) => Err(GatewayError::Provider(ProviderError::Configuration {
            message: format!("Image generation request failed: {e}"),
        })),
    }
}

pub async fn audio_transcriptions(
    State(state): State<AppState>,
    Json(request): Json<AudioRequest>,
) -> Result<Json<AudioResponse>, GatewayError> {
    // Route to appropriate provider using the client
    let start_time = std::time::Instant::now();
    let result = state.client.audio_transcription(request).await;
    let latency = start_time.elapsed();

    // Record metrics
    crate::metrics::record_request(
        crate::metrics::RequestMetricsBuilder::new(
            "POST".to_string(),
            "/v1/audio/transcriptions".to_string(),
            200,
            latency,
        )
        .provider(
            state
                .client
                .get_last_used_provider()
                .await
                .unwrap_or_default(),
        )
        .build(),
    )
    .await;

    match result {
        Ok(response) => Ok(Json(response)),
        Err(e) => Err(GatewayError::Provider(ProviderError::Configuration {
            message: format!("Audio transcription request failed: {e}"),
        })),
    }
}

pub async fn text_to_speech(
    State(state): State<AppState>,
    Json(request): Json<SpeechRequest>,
) -> Result<Json<SpeechResponse>, GatewayError> {
    // Route to appropriate provider using the client
    let start_time = std::time::Instant::now();
    let result = state.client.text_to_speech(request).await;
    let latency = start_time.elapsed();

    // Record metrics
    crate::metrics::record_request(
        crate::metrics::RequestMetricsBuilder::new(
            "POST".to_string(),
            "/v1/audio/speech".to_string(),
            200,
            latency,
        )
        .provider(
            state
                .client
                .get_last_used_provider()
                .await
                .unwrap_or_default(),
        )
        .build(),
    )
    .await;

    match result {
        Ok(response) => Ok(Json(response)),
        Err(e) => Err(GatewayError::Provider(ProviderError::Configuration {
            message: format!("Text-to-speech request failed: {e}"),
        })),
    }
}

pub async fn list_models(State(state): State<AppState>) -> Result<Json<Value>, GatewayError> {
    let mut all_models = Vec::new();

    // Get models from all configured providers
    for (provider_name, provider_config) in &state.config.providers {
        if provider_config.enabled {
            // Create a list of supported models for each provider
            let models = match provider_name.as_str() {
                "openai" => vec![
                    "gpt-4",
                    "gpt-4-turbo",
                    "gpt-4-turbo-preview",
                    "gpt-3.5-turbo",
                    "gpt-3.5-turbo-16k",
                    "text-embedding-ada-002",
                    "text-embedding-3-small",
                    "text-embedding-3-large",
                    "dall-e-2",
                    "dall-e-3",
                    "whisper-1",
                    "tts-1",
                    "tts-1-hd",
                ],
                "anthropic" => vec![
                    "claude-opus-4-20250514",
                    "claude-sonnet-4-20250514",
                    "claude-3-7-sonnet-20250219",
                    "claude-3-5-sonnet-20241022",
                    "claude-3-5-haiku-20241022",
                    "claude-3-5-sonnet-20240620",
                    "claude-3-haiku-20240307",
                    "claude-3",
                    "claude",
                ],
                "azure-openai" => vec![
                    "gpt-4",
                    "gpt-4-turbo",
                    "gpt-35-turbo",
                    "text-embedding-ada-002",
                    "dall-e-3",
                ],
                "google-vertex-ai" => vec![
                    "chat-bison",
                    "text-bison",
                    "gemini-pro",
                    "textembedding-gecko",
                ],
                "gemini" => vec![
                    "gemini-1.5-pro",
                    "gemini-1.5-pro-latest",
                    "gemini-1.5-flash",
                    "gemini-1.5-flash-latest",
                    "gemini-1.0-pro",
                    "gemini-1.0-pro-vision",
                    "text-embedding-004",
                ],
                _ => vec![],
            };

            for model in models {
                all_models.push(json!({
                    "id": model,
                    "object": "model",
                    "created": 1677610602,
                    "owned_by": provider_name,
                    "provider": provider_name
                }));
            }
        }
    }

    let response = json!({
        "object": "list",
        "data": all_models
    });

    Ok(Json(response))
}

pub async fn health_check() -> Result<Json<Value>, GatewayError> {
    let response = json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION"),
        "uptime": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });

    Ok(Json(response))
}

pub async fn metrics(State(state): State<AppState>) -> Result<Json<Value>, GatewayError> {
    let metrics = crate::metrics::get_aggregated_metrics().await;

    // Convert metrics to JSON for easier manipulation
    let mut provider_stats = serde_json::to_value(&metrics.provider_stats)?;
    let model_stats = serde_json::to_value(&metrics.model_stats)?;

    // If there are no provider stats from metrics (no requests made yet),
    // include configured providers from the config
    if provider_stats.as_object().unwrap().is_empty() {
        let configured_providers: serde_json::Map<String, serde_json::Value> = state
            .config
            .providers
            .iter()
            .map(|(name, config)| {
                (
                    name.clone(),
                    json!({
                        "requests": 0,
                        "successful_requests": 0,
                        "failed_requests": 0,
                        "average_latency_ms": 0.0,
                        "p95_latency_ms": 0.0,
                        "total_cost_usd": 0.0,
                        "uptime_percentage": 100.0,
                        "error_rate": 0.0,
                        "last_request": null,
                        "enabled": config.enabled,
                        "base_url": config.base_url,
                        "timeout": config.timeout.as_secs()
                    }),
                )
            })
            .collect();
        provider_stats = serde_json::Value::Object(configured_providers);
    }

    // Transform data to match dashboard expectations
    let dashboard_metrics = json!({
        "total_requests": metrics.total_requests,
        "average_latency_ms": metrics.average_latency_ms,
        "error_rate": metrics.error_rate,
        "requests_per_minute": metrics.requests_per_minute,
        "active_connections": metrics.active_connections,
        "total_cost_usd": metrics.total_cost_usd,
        "total_tokens": metrics.total_tokens,
        "uptime_seconds": metrics.uptime_seconds,
        "provider_stats": provider_stats,
        "model_stats": model_stats,
        "error_stats": {
            "error_types": metrics.error_stats.error_types,
            "total_errors": metrics.error_stats.total_errors,
            "error_rate": metrics.error_stats.error_rate,
            "most_common_error": metrics.error_stats.most_common_error
        },
        "cache_stats": metrics.cache_stats,
        "p50_latency_ms": metrics.p50_latency_ms,
        "p90_latency_ms": metrics.p90_latency_ms,
        "p95_latency_ms": metrics.p95_latency_ms,
        "p99_latency_ms": metrics.p99_latency_ms,
        "successful_requests": metrics.successful_requests,
        "failed_requests": metrics.failed_requests
    });

    // Broadcast metrics update to WebSocket clients
    if let Some(ws_manager) = &state.websocket_manager {
        if let Err(e) = ws_manager
            .broadcast_metrics_update(dashboard_metrics.clone())
            .await
        {
            tracing::warn!("Failed to broadcast metrics update: {}", e);
        }
    }

    Ok(Json(dashboard_metrics))
}

pub async fn prometheus_metrics(
    State(_state): State<AppState>,
) -> Result<Response<Body>, GatewayError> {
    let prometheus_metrics = crate::metrics::get_prometheus_metrics().await;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
        .body(Body::from(prometheus_metrics))
        .unwrap())
}

pub async fn list_providers(State(state): State<AppState>) -> Result<Json<Value>, GatewayError> {
    let providers: Vec<Value> = state
        .config
        .providers
        .iter()
        .map(|(name, config)| {
            json!({
                "name": name,
                "enabled": config.enabled,
                "base_url": config.base_url,
                "timeout": config.timeout.as_secs(),
                "models": Vec::<String>::new() // Could be populated with actual supported models
            })
        })
        .collect();

    Ok(Json(json!({
        "providers": providers
    })))
}

pub async fn get_config(State(state): State<AppState>) -> Result<Json<Value>, GatewayError> {
    // Return a sanitized version of the config (without sensitive data)
    let sanitized_config = json!({
        "server": {
            "host": state.config.server.host,
            "port": state.config.server.port,
            "timeout": state.config.server.timeout.as_secs()
        },
        "providers": state.config.providers.keys().collect::<Vec<_>>(),
        "routing": {
            "strategy": state.config.routing.strategy,
            "health_check_interval": state.config.routing.health_check_interval.as_secs()
        },
        "cache": {
            "enabled": state.config.cache.enabled,
            "backend": state.config.cache.backend
        },
        "metrics": {
            "enabled": state.config.metrics.enabled
        }
    });

    Ok(Json(sanitized_config))
}

pub async fn get_circuit_breaker_metrics(
    State(state): State<AppState>,
) -> Result<Json<Value>, GatewayError> {
    let cb_metrics = state.client.get_circuit_breaker_metrics().await;
    let health_status = state.client.get_provider_health_status().await;

    let mut metrics_data = json!({});

    for (provider_id, metrics) in cb_metrics {
        let health = health_status.get(&provider_id).unwrap_or(&true);
        metrics_data[provider_id] = json!({
            "state": format!("{:?}", metrics.state),
            "failure_count": metrics.failure_count,
            "success_count": metrics.success_count,
            "last_failure_time": metrics.last_failure_time.map(|t| t.elapsed().as_secs()),
            "last_success_time": metrics.last_success_time.map(|t| t.elapsed().as_secs()),
            "is_healthy": health
        });
    }

    Ok(Json(metrics_data))
}

pub async fn dashboard(State(_state): State<AppState>) -> Result<Html<String>, GatewayError> {
    dashboard::dashboard().await
}

pub async fn dashboard_js() -> Result<Response<Body>, GatewayError> {
    let js_content = include_str!("../dashboard/static/js/dashboard.js");
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/javascript")
        .header("Cache-Control", "public, max-age=3600")
        .body(Body::from(js_content))
        .unwrap())
}

pub async fn dashboard_css() -> Result<Response<Body>, GatewayError> {
    let css_content = include_str!("../dashboard/static/css/dashboard.css");
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/css")
        .header("Cache-Control", "public, max-age=3600")
        .body(Body::from(css_content))
        .unwrap())
}

pub async fn dashboard_websocket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<DashboardWebSocketQuery>,
) -> Response<Body> {
    // For now, use a default user ID. In production, extract from auth
    let user_id = "default_user".to_string();
    let session_id = uuid::Uuid::new_v4().to_string();

    // Get or create WebSocket manager from state
    if let Some(ws_manager) = state.websocket_manager.as_ref() {
        ws_manager
            .handle_connection(ws, user_id, session_id, query)
            .await
    } else {
        // Create a temporary manager if none exists
        let ws_manager = WebSocketManager::new();
        ws_manager
            .handle_connection(ws, user_id, session_id, query)
            .await
    }
}

// Helper functions
#[allow(dead_code)]
fn estimate_tokens(request: &ChatRequest) -> u32 {
    let mut total_tokens = 0;

    for message in &request.messages {
        // Rough estimation: 1 token ≈ 4 characters
        total_tokens += message.content.len() as u32 / 4;
    }

    // Add buffer for system messages and formatting
    total_tokens += 50;

    total_tokens
}

fn determine_cache_ttl(request: &ChatRequest, latency: std::time::Duration) -> std::time::Duration {
    // Dynamic TTL based on request characteristics and performance
    let base_ttl = std::time::Duration::from_secs(3600); // 1 hour base

    // Adjust based on latency (faster responses get longer cache)
    let latency_factor = if latency < std::time::Duration::from_millis(100) {
        2.0
    } else if latency < std::time::Duration::from_millis(500) {
        1.5
    } else {
        1.0
    };

    // Adjust based on model (more expensive models get longer cache)
    let model_factor = if request.model.contains("gpt-4") || request.model.contains("claude-3") {
        1.5
    } else {
        1.0
    };

    // Adjust based on temperature (lower temperature = more deterministic = longer cache)
    let temp_factor = if let Some(temp) = request.temperature {
        if temp < 0.3 {
            1.5
        } else if temp < 0.7 {
            1.0
        } else {
            0.5
        }
    } else {
        1.0
    };

    let adjusted_ttl = base_ttl.mul_f64(latency_factor * model_factor * temp_factor);

    // Clamp between 5 minutes and 24 hours
    std::cmp::min(
        std::cmp::max(adjusted_ttl, std::time::Duration::from_secs(300)),
        std::time::Duration::from_secs(86400),
    )
}

// Helper functions for legacy completions conversion
fn convert_legacy_completion_to_chat(request: Value) -> Result<ChatRequest, GatewayError> {
    let model = request
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidRequest {
            message: "Model is required".to_string(),
        })?;

    let prompt = request
        .get("prompt")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidRequest {
            message: "Prompt is required".to_string(),
        })?;

    let max_tokens = request
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(1000) as u32;

    let temperature = request
        .get("temperature")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.7);

    let messages = vec![ultrafast_models_sdk::models::Message::user(prompt)];

    Ok(ChatRequest {
        model: model.to_string(),
        messages,
        max_tokens: Some(max_tokens),
        temperature: Some(temperature as f32),
        top_p: request
            .get("top_p")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32),
        frequency_penalty: request
            .get("frequency_penalty")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32),
        presence_penalty: request
            .get("presence_penalty")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32),
        stop: request.get("stop").and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        }),
        user: request
            .get("user")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        stream: Some(false),
        tools: None,
        tool_choice: None,
    })
}

fn convert_chat_to_legacy_completion(
    response: ultrafast_models_sdk::models::ChatResponse,
) -> Result<Value, GatewayError> {
    let choice = response.choices.first().ok_or_else(|| {
        GatewayError::Provider(ProviderError::Configuration {
            message: "No choices in response".to_string(),
        })
    })?;

    let text = choice.message.content.clone();

    Ok(json!({
        "id": response.id,
        "object": "text_completion",
        "created": response.created,
        "model": response.model,
        "choices": [{
            "text": text,
            "index": choice.index,
            "logprobs": choice.logprobs,
            "finish_reason": choice.finish_reason
        }],
        "usage": response.usage
    }))
}
