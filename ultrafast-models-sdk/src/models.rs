//! # AI Model Types and Structures
//!
//! This module defines the core data structures for interacting with AI models
//! across different providers. It includes request and response types for chat
//! completions, embeddings, image generation, audio processing, and more.
//!
//! ## Overview
//!
//! The models module provides:
//! - **Chat Completions**: Conversational AI model interactions
//! - **Embeddings**: Text embedding generation and processing
//! - **Image Generation**: AI-powered image creation
//! - **Audio Processing**: Speech-to-text and text-to-speech
//! - **Streaming Support**: Real-time response streaming
//! - **Tool Integration**: Function calling and tool usage
//!
//! ## Chat Completions
//!
//! The primary interface for conversational AI models:
//!
//! ```rust
//! use ultrafast_models_sdk::{ChatRequest, ChatResponse, Message, Role};
//!
//! let request = ChatRequest {
//!     model: "gpt-4".to_string(),
//!     messages: vec![
//!         Message::system("You are a helpful assistant."),
//!         Message::user("Hello, how are you?"),
//!     ],
//!     temperature: Some(0.7),
//!     max_tokens: Some(100),
//!     stream: Some(false),
//!     ..Default::default()
//! };
//!
//! // Response includes choices, usage statistics, and metadata
//! let response: ChatResponse = client.chat_completion(request).await?;
//! ```
//!
//! ## Message Types
//!
//! Different message roles for conversation context:
//!
//! - **System**: Instructions and context for the AI
//! - **User**: User input and questions
//! - **Assistant**: AI responses and completions
//! - **Tool**: Function call results and tool responses
//!
//! ```rust
//! let system_msg = Message::system("You are a helpful assistant.");
//! let user_msg = Message::user("What's the weather like?");
//! let assistant_msg = Message::assistant("I don't have access to real-time weather data.");
//! ```
//!
//! ## Embeddings
//!
//! Text embedding generation for semantic analysis:
//!
//! ```rust
//! use ultrafast_models_sdk::{EmbeddingRequest, EmbeddingResponse};
//!
//! let request = EmbeddingRequest {
//!     model: "text-embedding-ada-002".to_string(),
//!     input: EmbeddingInput::StringArray(vec![
//!         "Hello, world!".to_string(),
//!         "How are you?".to_string(),
//!     ]),
//!     ..Default::default()
//! };
//!
//! let response: EmbeddingResponse = client.embedding(request).await?;
//! ```
//!
//! ## Image Generation
//!
//! AI-powered image creation from text prompts:
//!
//! ```rust
//! use ultrafast_models_sdk::{ImageRequest, ImageResponse};
//!
//! let request = ImageRequest {
//!     prompt: "A beautiful sunset over mountains".to_string(),
//!     model: Some("dall-e-3".to_string()),
//!     n: Some(1),
//!     size: Some("1024x1024".to_string()),
//!     quality: Some("standard".to_string()),
//!     response_format: Some("url".to_string()),
//!     ..Default::default()
//! };
//!
//! let response: ImageResponse = client.image_generation(request).await?;
//! ```
//!
//! ## Audio Processing
//!
//! Speech-to-text and text-to-speech capabilities:
//!
//! ### Audio Transcription
//!
//! ```rust
//! use ultrafast_models_sdk::{AudioRequest, AudioResponse};
//!
//! let request = AudioRequest {
//!     file: audio_data, // Vec<u8> containing audio file
//!     model: "whisper-1".to_string(),
//!     language: Some("en".to_string()),
//!     prompt: Some("This is a conversation about technology.".to_string()),
//!     response_format: Some("text".to_string()),
//!     temperature: Some(0.0),
//! };
//!
//! let response: AudioResponse = client.audio_transcription(request).await?;
//! ```
//!
//! ### Text-to-Speech
//!
//! ```rust
//! use ultrafast_models_sdk::{SpeechRequest, SpeechResponse};
//!
//! let request = SpeechRequest {
//!     model: "tts-1".to_string(),
//!     input: "Hello, this is a test of text-to-speech.".to_string(),
//!     voice: "alloy".to_string(),
//!     response_format: Some("mp3".to_string()),
//!     speed: Some(1.0),
//! };
//!
//! let response: SpeechResponse = client.text_to_speech(request).await?;
//! ```
//!
//! ## Streaming Support
//!
//! Real-time response streaming for chat completions:
//!
//! ```rust
//! use ultrafast_models_sdk::{StreamChunk, StreamChoice};
//!
//! let mut request = ChatRequest {
//!     model: "gpt-4".to_string(),
//!     messages: vec![Message::user("Tell me a story.")],
//!     stream: Some(true),
//!     ..Default::default()
//! };
//!
//! let mut stream = client.stream_chat_completion(request).await?;
//!
//! while let Some(chunk) = stream.next().await {
//!     match chunk {
//!         Ok(StreamChunk { choices, .. }) => {
//!             for choice in choices {
//!                 if let Some(content) = choice.delta.content {
//!                     print!("{}", content);
//!                 }
//!             }
//!         }
//!         Err(e) => eprintln!("Error: {}", e),
//!     }
//! }
//! ```
//!
//! ## Tool Integration
//!
//! Function calling and tool usage capabilities:
//!
//! ```rust
//! use ultrafast_models_sdk::{Tool, Function, ToolChoice};
//!
//! let request = ChatRequest {
//!     model: "gpt-4".to_string(),
//!     messages: vec![Message::user("What's the weather in New York?")],
//!     tools: Some(vec![Tool {
//!         tool_type: "function".to_string(),
//!         function: Function {
//!             name: "get_weather".to_string(),
//!             description: Some("Get current weather for a location".to_string()),
//!             parameters: serde_json::json!({
//!                 "type": "object",
//!                 "properties": {
//!                     "location": {"type": "string"}
//!                 },
//!                 "required": ["location"]
//!             }),
//!         },
//!     }]),
//!     tool_choice: Some(ToolChoice::Auto),
//!     ..Default::default()
//! };
//! ```
//!
//! ## Usage Statistics
//!
//! Token usage tracking for cost and performance monitoring:
//!
//! ```rust
//! use ultrafast_models_sdk::Usage;
//!
//! let usage = Usage {
//!     prompt_tokens: 100,
//!     completion_tokens: 50,
//!     total_tokens: 150,
//! };
//!
//! println!("Cost: ${:.4}", (usage.total_tokens as f64 / 1000.0) * 0.03);
//! ```
//!
//! ## Provider Compatibility
//!
//! All models are designed to be compatible with multiple AI providers:
//!
//! - **OpenAI**: GPT-4, GPT-3.5, DALL-E, Whisper, TTS
//! - **Anthropic**: Claude-3, Claude-2, Claude Instant
//! - **Google**: Gemini Pro, Gemini Pro Vision, PaLM
//! - **Azure OpenAI**: Azure-hosted OpenAI models
//! - **Ollama**: Local and remote Ollama models
//! - **Custom Providers**: Extensible for any provider
//!
//! ## Serialization
//!
//! All models support JSON serialization with provider-specific optimizations:
//!
//! - **Optional Fields**: Skip serialization of None values
//! - **Custom Serialization**: Provider-specific field mapping
//! - **Validation**: Automatic request validation
//! - **Error Handling**: Comprehensive error responses

use serde::{Deserialize, Serialize};

/// Chat completion request.
///
/// Represents a request to generate a chat completion using an AI model.
/// Supports various parameters for controlling the generation process.
///
/// # Example
///
/// ```rust
/// let request = ChatRequest {
///     model: "gpt-4".to_string(),
///     messages: vec![Message::user("Hello, world!")],
///     temperature: Some(0.7),
///     max_tokens: Some(100),
///     stream: Some(false),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatRequest {
    /// The model to use for completion
    pub model: String,
    /// The messages to generate a response for
    pub messages: Vec<Message>,
    /// Controls randomness (0.0 to 2.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Maximum number of tokens to generate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether to stream the response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Tools available for the model to use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    /// How the model should use tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Controls diversity via nucleus sampling (0.0 to 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Reduces repetition of similar tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    /// Reduces repetition of similar topics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    /// Sequences that stop generation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// User identifier for tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// Chat completion response.
///
/// Contains the generated response, metadata, and usage statistics.
///
/// # Example
///
/// ```rust
/// let response = ChatResponse {
///     id: "chatcmpl-123".to_string(),
///     object: "chat.completion".to_string(),
///     created: 1677652288,
///     model: "gpt-4".to_string(),
///     choices: vec![Choice { /* ... */ }],
///     usage: Some(Usage { /* ... */ }),
///     system_fingerprint: Some("fp_44709d6fcb".to_string()),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    /// Unique identifier for the completion
    pub id: String,
    /// Object type (always "chat.completion")
    pub object: String,
    /// Unix timestamp of creation
    pub created: u64,
    /// Model used for completion
    pub model: String,
    /// Generated completions
    pub choices: Vec<Choice>,
    /// Token usage statistics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// System fingerprint for model version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
}

/// A message in a conversation.
///
/// Represents a single message with role, content, and optional metadata.
/// Supports different message types for conversation context.
///
/// # Example
///
/// ```rust
/// let user_msg = Message::user("Hello, how are you?");
/// let assistant_msg = Message::assistant("I'm doing well, thank you!");
/// let system_msg = Message::system("You are a helpful assistant.");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Role of the message sender
    pub role: Role,
    /// Content of the message
    pub content: String,
    /// Optional name for the message sender
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Tool calls made by the assistant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// ID of the tool call being responded to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Create a user message.
    ///
    /// # Arguments
    ///
    /// * `content` - The message content
    ///
    /// # Returns
    ///
    /// Returns a new user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message.
    ///
    /// # Arguments
    ///
    /// * `content` - The message content
    ///
    /// # Returns
    ///
    /// Returns a new assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create a system message.
    ///
    /// # Arguments
    ///
    /// * `content` - The message content
    ///
    /// # Returns
    ///
    /// Returns a new system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

/// Role of a message in a conversation.
///
/// Defines the type of message sender in a conversation context.
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System instructions and context
    System,
    /// User input and questions
    User,
    /// AI assistant responses
    Assistant,
    /// Tool function results
    Tool,
}

/// A generated completion choice.
///
/// Represents a single completion option with message and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    /// Index of the choice in the response
    pub index: u32,
    /// The generated message
    pub message: Message,
    /// Reason why generation stopped
    pub finish_reason: Option<String>,
    /// Log probability of the choice
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: Function,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    None,
    Auto,
    Required,
    Specific { function: FunctionChoice },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionChoice {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: EmbeddingInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

impl Default for EmbeddingRequest {
    fn default() -> Self {
        Self {
            model: "text-embedding-ada-002".to_string(),
            input: EmbeddingInput::String("".to_string()),
            encoding_format: None,
            dimensions: None,
            user: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    String(String),
    StringArray(Vec<String>),
    TokenArray(Vec<u32>),
    TokenArrayArray(Vec<Vec<u32>>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub object: String,
    pub data: Vec<Embedding>,
    pub model: String,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    pub object: String,
    pub embedding: Vec<f32>,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRequest {
    pub prompt: String,
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResponse {
    pub created: u64,
    pub data: Vec<ImageData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b64_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRequest {
    pub file: Vec<u8>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioResponse {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<Word>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<Segment>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub word: String,
    pub start: f32,
    pub end: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id: u32,
    pub seek: u32,
    pub start: f32,
    pub end: f32,
    pub text: String,
    pub tokens: Vec<u32>,
    pub temperature: f32,
    pub avg_logprob: f32,
    pub compression_ratio: f32,
    pub no_speech_prob: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechRequest {
    pub model: String,
    pub input: String,
    pub voice: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechResponse {
    pub audio: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChoice {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaToolCall {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<DeltaFunction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}
