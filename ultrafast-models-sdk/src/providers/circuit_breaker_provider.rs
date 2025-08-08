use crate::circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerError, CircuitState,
};
use crate::error::ProviderError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
    ImageRequest, ImageResponse, SpeechRequest, SpeechResponse,
};
use crate::providers::{Provider, ProviderHealth, StreamResult};
use std::sync::Arc;

/// Wrapper that adds circuit breaker functionality to any provider
pub struct CircuitBreakerProvider {
    inner: Arc<dyn Provider>,
    circuit_breaker: CircuitBreaker,
}

impl CircuitBreakerProvider {
    pub fn new(provider: Arc<dyn Provider>, config: CircuitBreakerConfig) -> Self {
        let circuit_breaker =
            CircuitBreaker::new(format!("{}_circuit_breaker", provider.name()), config);

        Self {
            inner: provider,
            circuit_breaker,
        }
    }

    pub fn with_default_config(provider: Arc<dyn Provider>) -> Self {
        Self::new(provider, CircuitBreakerConfig::default())
    }

    pub async fn get_circuit_state(&self) -> CircuitState {
        self.circuit_breaker.get_state().await
    }

    pub async fn force_open(&self) {
        self.circuit_breaker.force_open().await;
    }

    pub async fn force_closed(&self) {
        self.circuit_breaker.force_closed().await;
    }

    pub async fn get_circuit_breaker_metrics(
        &self,
    ) -> Result<
        crate::circuit_breaker::CircuitBreakerMetrics,
        crate::circuit_breaker::CircuitBreakerError,
    > {
        Ok(self.circuit_breaker.get_metrics().await)
    }

    async fn handle_circuit_breaker_error(&self, error: CircuitBreakerError) -> ProviderError {
        match error {
            CircuitBreakerError::Open => {
                tracing::warn!("Provider {} circuit breaker is OPEN", self.inner.name());
                ProviderError::ServiceUnavailable
            }
            CircuitBreakerError::Timeout => {
                tracing::warn!("Provider {} operation timed out", self.inner.name());
                ProviderError::Timeout
            }
        }
    }
}

#[async_trait::async_trait]
impl Provider for CircuitBreakerProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn supports_streaming(&self) -> bool {
        self.inner.supports_streaming()
    }

    fn supports_function_calling(&self) -> bool {
        self.inner.supports_function_calling()
    }

    fn supported_models(&self) -> Vec<String> {
        self.inner.supported_models()
    }

    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let inner = self.inner.clone();
        let operation = || async move { inner.chat_completion(request).await };

        match self.circuit_breaker.call(operation).await {
            Ok(response) => Ok(response),
            Err(cb_error) => Err(self.handle_circuit_breaker_error(cb_error).await),
        }
    }

    async fn stream_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<StreamResult, ProviderError> {
        // For streaming, we check the circuit breaker state but don't wrap the stream
        // The stream itself will handle individual chunk failures
        let state = self.circuit_breaker.get_state().await;
        if state == CircuitState::Open {
            return Err(ProviderError::ServiceUnavailable);
        }

        // Attempt to start the stream
        let inner = self.inner.clone();
        let operation = || async move { inner.stream_chat_completion(request).await };

        match self.circuit_breaker.call(operation).await {
            Ok(stream) => Ok(stream),
            Err(cb_error) => Err(self.handle_circuit_breaker_error(cb_error).await),
        }
    }

    async fn embedding(
        &self,
        request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        let inner = self.inner.clone();
        let operation = || async move { inner.embedding(request).await };

        match self.circuit_breaker.call(operation).await {
            Ok(response) => Ok(response),
            Err(cb_error) => Err(self.handle_circuit_breaker_error(cb_error).await),
        }
    }

    async fn image_generation(
        &self,
        request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        let inner = self.inner.clone();
        let operation = || async move { inner.image_generation(request).await };

        match self.circuit_breaker.call(operation).await {
            Ok(response) => Ok(response),
            Err(cb_error) => Err(self.handle_circuit_breaker_error(cb_error).await),
        }
    }

    async fn audio_transcription(
        &self,
        request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        let inner = self.inner.clone();
        let operation = || async move { inner.audio_transcription(request).await };

        match self.circuit_breaker.call(operation).await {
            Ok(response) => Ok(response),
            Err(cb_error) => Err(self.handle_circuit_breaker_error(cb_error).await),
        }
    }

    async fn text_to_speech(
        &self,
        request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        let inner = self.inner.clone();
        let operation = || async move { inner.text_to_speech(request).await };

        match self.circuit_breaker.call(operation).await {
            Ok(response) => Ok(response),
            Err(cb_error) => Err(self.handle_circuit_breaker_error(cb_error).await),
        }
    }

    async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
        let inner = self.inner.clone();
        let operation = || async move { inner.health_check().await };

        match self.circuit_breaker.call(operation).await {
            Ok(health) => Ok(health),
            Err(cb_error) => Err(self.handle_circuit_breaker_error(cb_error).await),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Message;
    use crate::providers::{HealthStatus, ProviderHealth};
    use std::collections::HashMap;
    use std::time::Duration;

    // Mock provider for testing
    struct MockProvider {
        name: String,
        should_fail: bool,
        delay: Duration,
    }

    impl MockProvider {
        fn new(name: String, should_fail: bool, delay: Duration) -> Self {
            Self {
                name,
                should_fail,
                delay,
            }
        }
    }

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        fn supports_function_calling(&self) -> bool {
            false
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["test-model".to_string()]
        }

        async fn chat_completion(
            &self,
            _request: ChatRequest,
        ) -> Result<ChatResponse, ProviderError> {
            tokio::time::sleep(self.delay).await;

            if self.should_fail {
                Err(ProviderError::ServiceUnavailable)
            } else {
                Ok(ChatResponse {
                    id: "test-id".to_string(),
                    object: "chat.completion".to_string(),
                    created: 1234567890,
                    model: "test-model".to_string(),
                    choices: vec![],
                    usage: None,
                    system_fingerprint: None,
                })
            }
        }

        async fn stream_chat_completion(
            &self,
            _request: ChatRequest,
        ) -> Result<StreamResult, ProviderError> {
            Err(ProviderError::Configuration {
                message: "Streaming not supported by mock provider".to_string(),
            })
        }

        async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
            if self.should_fail {
                Err(ProviderError::ServiceUnavailable)
            } else {
                Ok(ProviderHealth {
                    status: HealthStatus::Healthy,
                    latency_ms: Some(10),
                    last_check: chrono::Utc::now(),
                    details: HashMap::new(),
                    error_rate: 0.0,
                })
            }
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_provider_success() {
        let mock_provider = Arc::new(MockProvider::new(
            "test".to_string(),
            false,
            Duration::from_millis(10),
        ));

        let cb_config = CircuitBreakerConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_millis(100),
            request_timeout: Duration::from_millis(50),
            half_open_max_calls: 1,
        };

        let cb_provider = CircuitBreakerProvider::new(mock_provider, cb_config);

        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: vec![Message::user("test")],
            ..Default::default()
        };

        let result = cb_provider.chat_completion(request).await;
        assert!(result.is_ok());
        assert_eq!(cb_provider.get_circuit_state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_provider_failure() {
        let mock_provider = Arc::new(MockProvider::new(
            "test".to_string(),
            true,
            Duration::from_millis(10),
        ));

        let cb_config = CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(100),
            request_timeout: Duration::from_millis(50),
            half_open_max_calls: 1,
        };

        let cb_provider = CircuitBreakerProvider::new(mock_provider, cb_config);

        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: vec![Message::user("test")],
            ..Default::default()
        };

        // First failure should open the circuit
        let result = cb_provider.chat_completion(request.clone()).await;
        assert!(result.is_err());
        assert_eq!(cb_provider.get_circuit_state().await, CircuitState::Open);

        // Second call should be blocked by circuit breaker
        let result = cb_provider.chat_completion(request).await;
        assert!(result.is_err());
        if let Err(ProviderError::ServiceUnavailable) = result {
            // Expected error
        } else {
            panic!("Expected ServiceUnavailable error");
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_provider_timeout() {
        let mock_provider = Arc::new(MockProvider::new(
            "test".to_string(),
            false,
            Duration::from_millis(100), // Longer than timeout
        ));

        let cb_config = CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(100),
            request_timeout: Duration::from_millis(50),
            half_open_max_calls: 1,
        };

        let cb_provider = CircuitBreakerProvider::new(mock_provider, cb_config);

        let request = ChatRequest {
            model: "test-model".to_string(),
            messages: vec![Message::user("test")],
            ..Default::default()
        };

        let result = cb_provider.chat_completion(request).await;
        assert!(result.is_err());
        if let Err(ProviderError::Timeout) = result {
            // Expected error
        } else {
            panic!("Expected Timeout error");
        }

        assert_eq!(cb_provider.get_circuit_state().await, CircuitState::Open);
    }
}
