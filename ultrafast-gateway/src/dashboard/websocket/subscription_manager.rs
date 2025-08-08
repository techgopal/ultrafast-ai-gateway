use crate::dashboard::websocket::DashboardMessage;
use crate::gateway_error::GatewayError;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct SubscriptionManager {
    subscriptions: Arc<RwLock<HashMap<String, UserSubscriptions>>>,
}

#[derive(Debug, Clone, Default)]
struct UserSubscriptions {
    topics: HashSet<String>,
    connections: HashMap<String, HashSet<String>>, // connection_id -> topics
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SubscriptionManager {
    pub fn new() -> Self {
        Self {
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn subscribe(
        &self,
        user_id: &str,
        connection_id: &str,
        topic: String,
    ) -> Result<(), GatewayError> {
        let mut subscriptions = self.subscriptions.write().await;
        let user_subs = subscriptions.entry(user_id.to_string()).or_default();

        user_subs.topics.insert(topic.clone());
        user_subs
            .connections
            .entry(connection_id.to_string())
            .or_default()
            .insert(topic);

        tracing::debug!(
            "User {} subscribed to topic via connection {}",
            user_id,
            connection_id
        );
        Ok(())
    }

    pub async fn unsubscribe(
        &self,
        user_id: &str,
        connection_id: &str,
        topic: &str,
    ) -> Result<(), GatewayError> {
        let mut subscriptions = self.subscriptions.write().await;
        if let Some(user_subs) = subscriptions.get_mut(user_id) {
            if let Some(conn_topics) = user_subs.connections.get_mut(connection_id) {
                conn_topics.remove(topic);
                if conn_topics.is_empty() {
                    user_subs.connections.remove(connection_id);
                }
            }

            // Check if any connection still has this topic
            let still_subscribed = user_subs
                .connections
                .values()
                .any(|topics| topics.contains(topic));

            if !still_subscribed {
                user_subs.topics.remove(topic);
            }
        }

        tracing::debug!(
            "User {} unsubscribed from topic {} via connection {}",
            user_id,
            topic,
            connection_id
        );
        Ok(())
    }

    pub async fn cleanup_connection_subscriptions(&self, connection_id: &str) {
        let mut subscriptions = self.subscriptions.write().await;
        for (user_id, user_subs) in subscriptions.iter_mut() {
            if let Some(topics) = user_subs.connections.remove(connection_id) {
                // Remove topics that are no longer subscribed by any connection
                for topic in topics {
                    let still_subscribed = user_subs
                        .connections
                        .values()
                        .any(|conn_topics| conn_topics.contains(&topic));

                    if !still_subscribed {
                        user_subs.topics.remove(&topic);
                    }
                }

                tracing::debug!(
                    "Cleaned up subscriptions for connection {} of user {}",
                    connection_id,
                    user_id
                );
            }
        }
    }

    pub async fn should_receive_message(&self, user_id: &str, message: &DashboardMessage) -> bool {
        let subscriptions = self.subscriptions.read().await;
        if let Some(user_subs) = subscriptions.get(user_id) {
            // Check if user is subscribed to the message topic
            if let Some(topic) = &message.topic {
                user_subs.topics.contains(topic)
            } else {
                // If no topic specified, check if user has any subscriptions
                !user_subs.topics.is_empty()
            }
        } else {
            false
        }
    }

    pub async fn get_subscribed_users(&self, topic: &str) -> Vec<String> {
        let subscriptions = self.subscriptions.read().await;
        subscriptions
            .iter()
            .filter(|(_, user_subs)| user_subs.topics.contains(topic))
            .map(|(user_id, _)| user_id.clone())
            .collect()
    }

    pub async fn get_user_subscriptions(&self, user_id: &str) -> Vec<String> {
        let subscriptions = self.subscriptions.read().await;
        if let Some(user_subs) = subscriptions.get(user_id) {
            user_subs.topics.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }
}
