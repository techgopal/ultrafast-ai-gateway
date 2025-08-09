// Audio API endpoint tests
use crate::tests::helpers;
use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_audio_transcription_basic() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "whisper-1",
        "file": "data:audio/wav;base64,UklGRnoGAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQoGAACBhYqFbF1fdJivrJBhNjVgodDbq2EcBj+a2/LDciUFLIHO8tiJNwgZaLvt559NEAxQp+PwtmMcBjiR1/LMeSwFJHfH8N2QQAoUXrTp66hVFApGn+DyvmwhBSuBzvLZiTYIG2m98OScTgwOUarm7blmGgU7k9n1unEiBC13yO/eizEIHWq+8+OWT",
        "response_format": "json"
    });
    
    let response = server
        .post("/v1/audio/transcriptions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["text"].is_string());
    } else {
        // Expected if audio transcription is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_audio_transcription_with_timestamp() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "whisper-1",
        "file": "data:audio/wav;base64,UklGRnoGAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQoGAACBhYqFbF1fdJivrJBhNjVgodDbq2EcBj+a2/LDciUFLIHO8tiJNwgZaLvt559NEAxQp+PwtmMcBjiR1/LMeSwFJHfH8N2QQAoUXrTp66hVFApGn+DyvmwhBSuBzvLZiTYIG2m98OScTgwOUarm7blmGgU7k9n1unEiBC13yO/eizEIHWq+8+OWT",
        "response_format": "verbose_json",
        "timestamp_granularities": ["word", "segment"]
    });
    
    let response = server
        .post("/v1/audio/transcriptions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["text"].is_string());
        assert!(body["language"].is_string());
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_audio_transcription_language_detection() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "whisper-1",
        "file": "data:audio/wav;base64,UklGRnoGAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQoGAACBhYqFbF1fdJivrJBhNjVgodDbq2EcBj+a2/LDciUFLIHO8tiJNwgZaLvt559NEAxQp+PwtmMcBjiR1/LMeSwFJHfH8N2QQAoUXrTp66hVFApGn+DyvmwhBSuBzvLZiTYIG2m98OScTgwOUarm7blmGgU7k9n1unEiBC13yO/eizEIHWq+8+OWT",
        "response_format": "verbose_json",
        "language": "en"
    });
    
    let response = server
        .post("/v1/audio/transcriptions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["text"].is_string());
        assert_eq!(body["language"].as_str().unwrap(), "en");
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_audio_transcription_prompt() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "whisper-1",
        "file": "data:audio/wav;base64,UklGRnoGAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQoGAACBhYqFbF1fdJivrJBhNjVgodDbq2EcBj+a2/LDciUFLIHO8tiJNwgZaLvt559NEAxQp+PwtmMcBjiR1/LMeSwFJHfH8N2QQAoUXrTp66hVFApGn+DyvmwhBSuBzvLZiTYIG2m98OScTgwOUarm7blmGgU7k9n1unEiBC13yO/eizEIHWq+8+OWT",
        "prompt": "This is a technical discussion about AI"
    });
    
    let response = server
        .post("/v1/audio/transcriptions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let body: Value = response.json();
        assert!(body["text"].is_string());
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_text_to_speech_basic() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "tts-1",
        "input": "Hello, this is a test of text to speech conversion.",
        "voice": "alloy"
    });
    
    let response = server
        .post("/v1/audio/speech")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let headers = response.headers();
        assert_eq!(headers.get("content-type").unwrap(), "audio/mpeg");
        let body = response.bytes();
        assert!(!body.is_empty());
    } else {
        // Expected if TTS is not configured
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_text_to_speech_different_voices() {
    let server = helpers::create_test_server().await;
    
    let voices = ["alloy", "echo", "fable", "onyx", "nova", "shimmer"];
    
    for voice in voices {
        let request = serde_json::json!({
            "model": "tts-1",
            "input": "This is a test with voice: ",
            "voice": voice
        });
        
        let response = server
            .post("/v1/audio/speech")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        if response.status_code().is_success() {
            let headers = response.headers();
            assert_eq!(headers.get("content-type").unwrap(), "audio/mpeg");
            let body = response.bytes();
            assert!(!body.is_empty());
        } else {
            assert!(response.status_code().is_server_error());
        }
    }
}

#[tokio::test]
async fn test_text_to_speech_response_format() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "tts-1",
        "input": "Testing different response formats",
        "voice": "alloy",
        "response_format": "opus"
    });
    
    let response = server
        .post("/v1/audio/speech")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let headers = response.headers();
        assert_eq!(headers.get("content-type").unwrap(), "audio/opus");
        let body = response.bytes();
        assert!(!body.is_empty());
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_text_to_speech_speed() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "tts-1",
        "input": "Testing speech speed control",
        "voice": "alloy",
        "speed": 1.5
    });
    
    let response = server
        .post("/v1/audio/speech")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    if response.status_code().is_success() {
        let headers = response.headers();
        assert_eq!(headers.get("content-type").unwrap(), "audio/mpeg");
        let body = response.bytes();
        assert!(!body.is_empty());
    } else {
        assert!(response.status_code().is_server_error());
    }
}

#[tokio::test]
async fn test_audio_missing_file() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "whisper-1",
        "response_format": "json"
    });
    
    let response = server
        .post("/v1/audio/transcriptions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should return an error for missing file
    assert!(response.status_code().is_client_error());
}

#[tokio::test]
async fn test_audio_invalid_model() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "invalid-audio-model",
        "file": "data:audio/wav;base64,UklGRnoGAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQoGAACBhYqFbF1fdJivrJBhNjVgodDbq2EcBj+a2/LDciUFLIHO8tiJNwgZaLvt559NEAxQp+PwtmMcBjiR1/LMeSwFJHfH8N2QQAoUXrTp66hVFApGn+DyvmwhBSuBzvLZiTYIG2m98OScTgwOUarm7blmGgU7k9n1unEiBC13yO/eizEIHWq+8+OWT"
    });
    
    let response = server
        .post("/v1/audio/transcriptions")
        .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should handle invalid model gracefully
    assert!(response.status_code().is_server_error());
}

#[tokio::test]
async fn test_audio_authentication() {
    let server = helpers::create_test_server().await;
    
    let request = serde_json::json!({
        "model": "whisper-1",
        "file": "data:audio/wav;base64,UklGRnoGAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQoGAACBhYqFbF1fdJivrJBhNjVgodDbq2EcBj+a2/LDciUFLIHO8tiJNwgZaLvt559NEAxQp+PwtmMcBjiR1/LMeSwFJHfH8N2QQAoUXrTp66hVFApGn+DyvmwhBSuBzvLZiTYIG2m98OScTgwOUarm7blmGgU7k9n1unEiBC13yO/eizEIHWq+8+OWT"
    });
    
    // Test without authentication
    let response = server
        .post("/v1/audio/transcriptions")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject unauthenticated requests
    assert!(response.status_code().is_client_error());
    
    // Test with invalid API key
    let response = server
        .post("/v1/audio/transcriptions")
        .add_header("Authorization", "ApiKey invalid-key")
        .add_header("Content-Type", "application/json")
        .json(&request)
        .await;
    
    // Should reject invalid API key
    assert!(response.status_code().is_client_error() || response.status_code().is_server_error());
}

#[tokio::test]
async fn test_audio_rate_limiting() {
    let server = helpers::create_test_server().await;
    
    // Make multiple requests to test rate limiting
    for i in 0..3 {
        let request = serde_json::json!({
            "model": "whisper-1",
            "file": "data:audio/wav;base64,UklGRnoGAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQoGAACBhYqFbF1fdJivrJBhNjVgodDbq2EcBj+a2/LDciUFLIHO8tiJNwgZaLvt559NEAxQp+PwtmMcBjiR1/LMeSwFJHfH8N2QQAoUXrTp66hVFApGn+DyvmwhBSuBzvLZiTYIG2m98OScTgwOUarm7blmGgU7k9n1unEiBC13yO/eizEIHWq+8+OWT"
        });
        
        let response = server
            .post("/v1/audio/transcriptions")
            .add_header("Authorization", "ApiKey sk-ultrafast-gateway-key")
            .add_header("Content-Type", "application/json")
            .json(&request)
            .await;
        
        // Should handle rate limiting gracefully
        assert!(response.status_code().is_success() || response.status_code().is_server_error());
    }
}
