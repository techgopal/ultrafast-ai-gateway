// WebSocket Real-time Dashboard Updates
// High-performance WebSocket system for live dashboard updates

// Removed problematic import
use crate::gateway_error::GatewayError;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, RwLock};
use uuid::Uuid;

pub mod connection_manager;
pub mod message_handler;
pub mod rate_limiter;
pub mod subscription_manager;

/// Dashboard update for WebSocket broadcasting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardUpdate {
    pub update_type: UpdateType,
    pub data: serde_json::Value,
    pub timestamp: i64,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpdateType {
    MetricsUpdate,
    ProviderStatusChange,
    NewAlert,
    ConfigurationChange,
    UserAction,
}

/// WebSocket manager for handling real-time dashboard connections
pub struct WebSocketManager {
    connections: Arc<RwLock<HashMap<String, DashboardConnection>>>,
    broadcast_tx: broadcast::Sender<DashboardMessage>,
    subscription_manager: Arc<subscription_manager::SubscriptionManager>,
    rate_limiter: Arc<rate_limiter::WebSocketRateLimiter>,
    metrics: Arc<RwLock<WebSocketMetrics>>,
    config: WebSocketConfig,
}

impl Default for WebSocketManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSocketManager {
    pub fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(1000);

        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
            subscription_manager: Arc::new(subscription_manager::SubscriptionManager::new()),
            rate_limiter: Arc::new(rate_limiter::WebSocketRateLimiter::new()),
            metrics: Arc::new(RwLock::new(WebSocketMetrics::default())),
            config: WebSocketConfig::default(),
        }
    }

    pub fn with_config(config: WebSocketConfig) -> Self {
        let (broadcast_tx, _) = broadcast::channel(config.broadcast_buffer_size);

        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
            subscription_manager: Arc::new(subscription_manager::SubscriptionManager::new()),
            rate_limiter: Arc::new(rate_limiter::WebSocketRateLimiter::new()),
            metrics: Arc::new(RwLock::new(WebSocketMetrics::default())),
            config,
        }
    }

    /// Handle new WebSocket connection
    pub async fn handle_connection(
        &self,
        ws: WebSocketUpgrade,
        user_id: String,
        session_id: String,
        query: DashboardWebSocketQuery,
    ) -> Response {
        let manager = self.clone();

        ws.on_upgrade(move |socket| async move {
            if let Err(e) = manager
                .handle_socket(socket, user_id, session_id, query)
                .await
            {
                tracing::error!("WebSocket connection error: {}", e);
            }
        })
    }

    async fn handle_socket(
        &self,
        socket: WebSocket,
        user_id: String,
        session_id: String,
        query: DashboardWebSocketQuery,
    ) -> Result<(), GatewayError> {
        let connection_id = Uuid::new_v4().to_string();

        // Create connection
        let connection = DashboardConnection {
            id: connection_id.clone(),
            user_id: user_id.clone(),
            session_id: session_id.clone(),
            connected_at: Instant::now(),
            last_activity: Instant::now(),
            subscriptions: Vec::new(),
            permissions: query.permissions.unwrap_or_default(),
            metadata: query.metadata.unwrap_or_default(),
        };

        // Store connection
        {
            let mut connections = self.connections.write().await;
            connections.insert(connection_id.clone(), connection);
        }

        // Update metrics
        self.update_connection_metrics(1).await;

        tracing::info!(
            "New WebSocket connection: {} for user: {}",
            connection_id,
            user_id
        );

        // Split socket
        let (mut sender, mut receiver) = socket.split();

        // Create channels for communication
        let (tx, mut rx) = mpsc::channel::<DashboardMessage>(100);

        // Subscribe to broadcast channel
        let mut broadcast_rx = self.broadcast_tx.subscribe();

        // Spawn task to handle outgoing messages
        let connection_id_clone = connection_id.clone();
        let user_id_clone = user_id.clone();
        let subscription_manager = self.subscription_manager.clone();
        let outgoing_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Handle messages from the specific connection channel
                    msg = rx.recv() => {
                        match msg {
                            Some(dashboard_msg) => {
                                let ws_msg = Message::Text(serde_json::to_string(&dashboard_msg).unwrap_or_default().into());
                                if sender.send(ws_msg).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                    // Handle broadcast messages
                    msg = broadcast_rx.recv() => {
                        match msg {
                            Ok(dashboard_msg) => {
                                // Check if user should receive this message
                                if subscription_manager.should_receive_message(&user_id_clone, &dashboard_msg).await {
                                    let ws_msg = Message::Text(serde_json::to_string(&dashboard_msg).unwrap_or_default().into());
                                    if sender.send(ws_msg).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                tracing::warn!("WebSocket broadcast lagged for connection: {}", connection_id_clone);
                                continue;
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        });

        // Handle incoming messages
        let message_handler = message_handler::MessageHandler::new(
            self.subscription_manager.clone(),
            self.rate_limiter.clone(),
        );

        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    // Update last activity
                    self.update_connection_activity(&connection_id).await;

                    // Parse and handle message
                    match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(client_msg) => {
                            if let Err(e) = message_handler
                                .handle_message(client_msg, &user_id, &connection_id, &tx)
                                .await
                            {
                                tracing::error!("Error handling WebSocket message: {}", e);

                                // Send error response
                                let error_msg = DashboardMessage {
                                    message_type: MessageType::Error,
                                    data: serde_json::json!({
                                        "error": e.to_string()
                                    }),
                                    timestamp: chrono::Utc::now().timestamp(),
                                    user_id: Some(user_id.clone()),
                                    topic: None,
                                };

                                if tx.send(error_msg).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Invalid WebSocket message format: {}", e);
                        }
                    }
                }
                Ok(Message::Binary(_)) => {
                    tracing::debug!("Received binary message (not supported)");
                }
                Ok(Message::Ping(_data)) => {
                    // Respond to ping
                    if tx
                        .send(DashboardMessage {
                            message_type: MessageType::Pong,
                            data: serde_json::Value::Null,
                            timestamp: chrono::Utc::now().timestamp(),
                            user_id: Some(user_id.clone()),
                            topic: None,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Pong(_)) => {
                    // Update connection activity on pong
                    self.update_connection_activity(&connection_id).await;
                }
                Ok(Message::Close(_)) => {
                    tracing::info!("WebSocket connection closed by client: {}", connection_id);
                    break;
                }
                Err(e) => {
                    tracing::error!("WebSocket error: {}", e);
                    break;
                }
            }
        }

        // Cleanup
        outgoing_task.abort();
        self.cleanup_connection(&connection_id).await;

        Ok(())
    }

    /// Broadcast update to all connected clients
    pub async fn broadcast_update(&self, update: DashboardUpdate) -> Result<(), GatewayError> {
        let message = DashboardMessage {
            message_type: MessageType::Update,
            data: serde_json::to_value(&update)?,
            timestamp: chrono::Utc::now().timestamp(),
            user_id: update.user_id.clone(),
            topic: None,
        };

        // Check if there are any active connections before broadcasting
        let connection_count = self.get_connection_count().await;
        if connection_count == 0 {
            // No active connections, don't broadcast
            tracing::debug!("No active connections, skipping broadcast");
            return Ok(());
        }

        // Send to broadcast channel
        if let Err(e) = self.broadcast_tx.send(message) {
            tracing::warn!("Failed to broadcast message: {}", e);
            // Don't return error for broadcast failures, just log
            return Ok(());
        }

        // Update metrics
        self.update_broadcast_metrics().await;

        Ok(())
    }

    pub async fn broadcast_metrics_update(
        &self,
        metrics: serde_json::Value,
    ) -> Result<(), GatewayError> {
        let update = DashboardUpdate {
            update_type: UpdateType::MetricsUpdate,
            data: metrics,
            timestamp: chrono::Utc::now().timestamp(),
            user_id: None,
        };
        self.broadcast_update(update).await
    }

    pub async fn broadcast_provider_status(
        &self,
        provider: &str,
        status: &str,
    ) -> Result<(), GatewayError> {
        let update = DashboardUpdate {
            update_type: UpdateType::ProviderStatusChange,
            data: json!({
                "provider": provider,
                "status": status,
                "timestamp": chrono::Utc::now().timestamp()
            }),
            timestamp: chrono::Utc::now().timestamp(),
            user_id: None,
        };
        self.broadcast_update(update).await
    }

    pub async fn broadcast_alert(
        &self,
        alert_type: &str,
        message: &str,
        severity: &str,
    ) -> Result<(), GatewayError> {
        let update = DashboardUpdate {
            update_type: UpdateType::NewAlert,
            data: json!({
                "type": alert_type,
                "message": message,
                "severity": severity,
                "timestamp": chrono::Utc::now().timestamp()
            }),
            timestamp: chrono::Utc::now().timestamp(),
            user_id: None,
        };
        self.broadcast_update(update).await
    }

    /// Send targeted message to specific user
    pub async fn send_to_user(
        &self,
        user_id: &str,
        message: DashboardMessage,
    ) -> Result<(), GatewayError> {
        let connections = self.connections.read().await;
        let user_connections: Vec<_> = connections
            .values()
            .filter(|conn| conn.user_id == user_id)
            .collect();

        if user_connections.is_empty() {
            return Err(GatewayError::InvalidRequest {
                message: format!("No active connections for user: {user_id}"),
            });
        }

        // For targeted messages, we'd need individual channels per connection
        // This is a simplified implementation
        if let Err(e) = self.broadcast_tx.send(message) {
            tracing::error!("Failed to send targeted message: {}", e);
            return Err(GatewayError::Internal {
                message: "Failed to send message".to_string(),
            });
        }

        Ok(())
    }

    /// Get connection statistics
    pub async fn get_metrics(&self) -> WebSocketMetrics {
        let metrics = self.metrics.read().await;
        metrics.clone()
    }

    /// Get active connections count
    pub async fn get_connection_count(&self) -> usize {
        let connections = self.connections.read().await;
        connections.len()
    }

    /// Get connections for a specific user
    pub async fn get_user_connections(&self, user_id: &str) -> Vec<String> {
        let connections = self.connections.read().await;
        connections
            .values()
            .filter(|conn| conn.user_id == user_id)
            .map(|conn| conn.id.clone())
            .collect()
    }

    async fn update_connection_activity(&self, connection_id: &str) {
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.get_mut(connection_id) {
            connection.last_activity = Instant::now();
        }
    }

    async fn cleanup_connection(&self, connection_id: &str) {
        // Remove from connections
        {
            let mut connections = self.connections.write().await;
            connections.remove(connection_id);
        }

        // Clean up subscriptions
        self.subscription_manager
            .cleanup_connection_subscriptions(connection_id)
            .await;

        // Update metrics
        self.update_connection_metrics(-1).await;

        tracing::info!("Cleaned up WebSocket connection: {}", connection_id);
    }

    async fn update_connection_metrics(&self, delta: i32) {
        let mut metrics = self.metrics.write().await;
        if delta > 0 {
            metrics.total_connections += delta as u64;
            metrics.active_connections += delta as u64;
        } else {
            metrics.active_connections = metrics.active_connections.saturating_sub((-delta) as u64);
        }
    }

    async fn update_broadcast_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.messages_sent += 1;
    }

    /// Start background tasks for connection management
    pub async fn start_background_tasks(&self) {
        let manager = self.clone();

        // Connection cleanup task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));

            loop {
                interval.tick().await;
                manager.cleanup_stale_connections().await;
            }
        });

        // Metrics update task
        let manager = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));

            loop {
                interval.tick().await;
                manager.update_metrics_snapshot().await;
            }
        });
    }

    async fn cleanup_stale_connections(&self) {
        let stale_threshold = Duration::from_secs(self.config.connection_timeout_seconds);
        let mut stale_connections = Vec::new();

        {
            let connections = self.connections.read().await;
            let now = Instant::now();

            for (id, connection) in connections.iter() {
                if now.duration_since(connection.last_activity) > stale_threshold {
                    stale_connections.push(id.clone());
                }
            }
        }

        for connection_id in stale_connections {
            tracing::info!("Cleaning up stale WebSocket connection: {}", connection_id);
            self.cleanup_connection(&connection_id).await;
        }
    }

    async fn update_metrics_snapshot(&self) {
        let mut metrics = self.metrics.write().await;
        let connections = self.connections.read().await;

        metrics.active_connections = connections.len() as u64;
        metrics.last_updated = Instant::now();

        // Calculate additional metrics
        let total_subscriptions: usize = connections
            .values()
            .map(|conn| conn.subscriptions.len())
            .sum();

        metrics.total_subscriptions = total_subscriptions as u64;
    }
}

impl Clone for WebSocketManager {
    fn clone(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            broadcast_tx: self.broadcast_tx.clone(),
            subscription_manager: self.subscription_manager.clone(),
            rate_limiter: self.rate_limiter.clone(),
            metrics: self.metrics.clone(),
            config: self.config.clone(),
        }
    }
}

/// WebSocket connection information
#[derive(Debug, Clone)]
pub struct DashboardConnection {
    pub id: String,
    pub user_id: String,
    pub session_id: String,
    pub connected_at: Instant,
    pub last_activity: Instant,
    pub subscriptions: Vec<String>,
    pub permissions: Vec<String>,
    pub metadata: HashMap<String, String>,
}

/// WebSocket configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    pub enabled: bool,
    pub max_connections: usize,
    pub connection_timeout_seconds: u64,
    pub message_buffer_size: usize,
    pub broadcast_buffer_size: usize,
    pub rate_limit: WebSocketRateLimit,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_connections: 1000,
            connection_timeout_seconds: 300, // 5 minutes
            message_buffer_size: 100,
            broadcast_buffer_size: 1000,
            rate_limit: WebSocketRateLimit::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketRateLimit {
    pub messages_per_minute: u32,
    pub burst_size: u32,
}

impl Default for WebSocketRateLimit {
    fn default() -> Self {
        Self {
            messages_per_minute: 60,
            burst_size: 10,
        }
    }
}

/// WebSocket metrics
#[derive(Debug, Clone, Serialize)]
pub struct WebSocketMetrics {
    pub active_connections: u64,
    pub total_connections: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub total_subscriptions: u64,
    pub connection_errors: u64,
    #[serde(skip)]
    pub last_updated: Instant,
}

impl Default for WebSocketMetrics {
    fn default() -> Self {
        Self {
            active_connections: 0,
            total_connections: 0,
            messages_sent: 0,
            messages_received: 0,
            total_subscriptions: 0,
            connection_errors: 0,
            last_updated: Instant::now(),
        }
    }
}

/// Dashboard WebSocket message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardMessage {
    #[serde(rename = "type")]
    pub message_type: MessageType,
    pub data: serde_json::Value,
    pub timestamp: i64,
    pub user_id: Option<String>,
    pub topic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    Update,
    Subscribe,
    Unsubscribe,
    Ping,
    Pong,
    Error,
    Heartbeat,
    ConfigChange,
}

/// Client message format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMessage {
    #[serde(rename = "type")]
    pub message_type: ClientMessageType,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessageType {
    Subscribe,
    Unsubscribe,
    Ping,
    RequestUpdate,
}

/// WebSocket query parameters
#[derive(Debug, Clone, Deserialize)]
pub struct DashboardWebSocketQuery {
    pub permissions: Option<Vec<String>>,
    pub metadata: Option<HashMap<String, String>>,
}
