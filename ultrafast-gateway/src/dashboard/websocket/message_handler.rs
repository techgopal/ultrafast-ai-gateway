use crate::dashboard::websocket::rate_limiter::WebSocketRateLimiter;
use crate::dashboard::websocket::subscription_manager::SubscriptionManager;
use crate::dashboard::websocket::{ClientMessage, DashboardMessage, MessageType};
use crate::gateway_error::GatewayError;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct MessageHandler {
    subscription_manager: Arc<SubscriptionManager>,
    rate_limiter: Arc<WebSocketRateLimiter>,
}

impl MessageHandler {
    pub fn new(
        subscription_manager: Arc<SubscriptionManager>,
        rate_limiter: Arc<WebSocketRateLimiter>,
    ) -> Self {
        Self {
            subscription_manager,
            rate_limiter,
        }
    }

    pub async fn handle_message(
        &self,
        message: ClientMessage,
        user_id: &str,
        connection_id: &str,
        response_tx: &mpsc::Sender<DashboardMessage>,
    ) -> Result<(), GatewayError> {
        // Check rate limit
        if !self.rate_limiter.check_rate_limit(user_id).await? {
            return Err(GatewayError::RateLimit {
                message: "WebSocket rate limit exceeded".to_string(),
            });
        }

        match message.message_type {
            crate::dashboard::websocket::ClientMessageType::Subscribe => {
                self.handle_subscribe(message.data, user_id, connection_id, response_tx)
                    .await
            }
            crate::dashboard::websocket::ClientMessageType::Unsubscribe => {
                self.handle_unsubscribe(message.data, user_id, connection_id, response_tx)
                    .await
            }
            crate::dashboard::websocket::ClientMessageType::Ping => {
                self.handle_ping(response_tx).await
            }
            crate::dashboard::websocket::ClientMessageType::RequestUpdate => {
                self.handle_request_update(message.data, user_id, response_tx)
                    .await
            }
        }
    }

    async fn handle_subscribe(
        &self,
        data: serde_json::Value,
        user_id: &str,
        connection_id: &str,
        response_tx: &mpsc::Sender<DashboardMessage>,
    ) -> Result<(), GatewayError> {
        if let Some(topic) = data.get("topic").and_then(|t| t.as_str()) {
            self.subscription_manager
                .subscribe(user_id, connection_id, topic.to_string())
                .await?;

            let response = DashboardMessage {
                message_type: MessageType::Subscribe,
                data: serde_json::json!({
                    "success": true,
                    "topic": topic
                }),
                timestamp: chrono::Utc::now().timestamp(),
                user_id: Some(user_id.to_string()),
                topic: Some(topic.to_string()),
            };

            response_tx
                .send(response)
                .await
                .map_err(|_| GatewayError::Internal {
                    message: "Failed to send subscription response".to_string(),
                })?;
        }

        Ok(())
    }

    async fn handle_unsubscribe(
        &self,
        data: serde_json::Value,
        user_id: &str,
        connection_id: &str,
        response_tx: &mpsc::Sender<DashboardMessage>,
    ) -> Result<(), GatewayError> {
        if let Some(topic) = data.get("topic").and_then(|t| t.as_str()) {
            self.subscription_manager
                .unsubscribe(user_id, connection_id, topic)
                .await?;

            let response = DashboardMessage {
                message_type: MessageType::Unsubscribe,
                data: serde_json::json!({
                    "success": true,
                    "topic": topic
                }),
                timestamp: chrono::Utc::now().timestamp(),
                user_id: Some(user_id.to_string()),
                topic: Some(topic.to_string()),
            };

            response_tx
                .send(response)
                .await
                .map_err(|_| GatewayError::Internal {
                    message: "Failed to send unsubscription response".to_string(),
                })?;
        }

        Ok(())
    }

    async fn handle_ping(
        &self,
        response_tx: &mpsc::Sender<DashboardMessage>,
    ) -> Result<(), GatewayError> {
        let response = DashboardMessage {
            message_type: MessageType::Pong,
            data: serde_json::json!({
                "timestamp": chrono::Utc::now().timestamp()
            }),
            timestamp: chrono::Utc::now().timestamp(),
            user_id: None,
            topic: None,
        };

        response_tx
            .send(response)
            .await
            .map_err(|_| GatewayError::Internal {
                message: "Failed to send pong response".to_string(),
            })?;

        Ok(())
    }

    async fn handle_request_update(
        &self,
        _data: serde_json::Value,
        user_id: &str,
        response_tx: &mpsc::Sender<DashboardMessage>,
    ) -> Result<(), GatewayError> {
        // Fetch current metrics and send update
        let metrics = crate::metrics::get_aggregated_metrics().await;

        let response = DashboardMessage {
            message_type: MessageType::Update,
            data: serde_json::to_value(metrics)?,
            timestamp: chrono::Utc::now().timestamp(),
            user_id: Some(user_id.to_string()),
            topic: None,
        };

        response_tx
            .send(response)
            .await
            .map_err(|_| GatewayError::Internal {
                message: "Failed to send update response".to_string(),
            })?;

        Ok(())
    }
}
