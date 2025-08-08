use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
// Removed unused uuid import
use crate::gateway_error::GatewayError;

#[derive(Debug, Clone)]
pub struct ConnectionManager {
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
}

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub user_id: String,
    pub session_id: String,
    pub connected_at: std::time::Instant,
    pub permissions: Vec<String>,
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_connection(
        &self,
        connection_id: String,
        user_id: String,
        session_id: String,
        permissions: Vec<String>,
    ) -> Result<(), GatewayError> {
        let mut connections = self.connections.write().await;
        let info = ConnectionInfo {
            user_id,
            session_id,
            connected_at: std::time::Instant::now(),
            permissions,
        };
        connections.insert(connection_id, info);
        Ok(())
    }

    pub async fn remove_connection(&self, connection_id: &str) -> Result<(), GatewayError> {
        let mut connections = self.connections.write().await;
        connections.remove(connection_id);
        Ok(())
    }

    pub async fn get_connection(&self, connection_id: &str) -> Option<ConnectionInfo> {
        let connections = self.connections.read().await;
        connections.get(connection_id).cloned()
    }

    pub async fn get_user_connections(&self, user_id: &str) -> Vec<String> {
        let connections = self.connections.read().await;
        connections
            .iter()
            .filter(|(_, info)| info.user_id == user_id)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub async fn get_connection_count(&self) -> usize {
        let connections = self.connections.read().await;
        connections.len()
    }
}
