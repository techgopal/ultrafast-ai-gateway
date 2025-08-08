use crate::config::PluginConfig;
use crate::gateway_error::GatewayError;
use axum::{
    body::Body,
    http::{Request, Response},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use ultrafast_models_sdk::models::{
    ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
};
use uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEntry {
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_usd: f64,
    pub request_id: String,
}

#[derive(Clone, Debug)]
pub struct CostTrackingPlugin {
    name: String,
    enabled: bool,
    costs: Arc<RwLock<Vec<CostEntry>>>,
    provider_costs: HashMap<String, ProviderCost>,
}

#[derive(Debug, Clone)]
struct ProviderCost {
    input_cost_per_1k: f64,
    output_cost_per_1k: f64,
}

impl CostTrackingPlugin {
    pub fn new(config: &PluginConfig) -> Result<Self, GatewayError> {
        let mut provider_costs = HashMap::new();

        // Default costs (can be overridden in config)
        provider_costs.insert(
            "openai".to_string(),
            ProviderCost {
                input_cost_per_1k: 0.03,  // GPT-4 input cost per 1K tokens
                output_cost_per_1k: 0.06, // GPT-4 output cost per 1K tokens
            },
        );

        provider_costs.insert(
            "anthropic".to_string(),
            ProviderCost {
                input_cost_per_1k: 0.015,  // Claude-3 input cost per 1K tokens
                output_cost_per_1k: 0.075, // Claude-3 output cost per 1K tokens
            },
        );

        Ok(Self {
            name: config.name.clone(),
            enabled: config.enabled,
            costs: Arc::new(RwLock::new(Vec::new())),
            provider_costs,
        })
    }

    pub async fn add_cost(&self, entry: CostEntry) {
        let mut costs = self.costs.write().await;
        costs.push(entry);
    }

    pub async fn get_total_cost(
        &self,
        provider: Option<&str>,
        since: Option<DateTime<Utc>>,
    ) -> f64 {
        let costs = self.costs.read().await;
        costs
            .iter()
            .filter(|entry| {
                if let Some(provider_filter) = provider {
                    entry.provider == provider_filter
                } else {
                    true
                }
            })
            .filter(|entry| {
                if let Some(since_time) = since {
                    entry.timestamp >= since_time
                } else {
                    true
                }
            })
            .map(|entry| entry.cost_usd)
            .sum()
    }

    pub async fn get_cost_summary(&self) -> HashMap<String, f64> {
        let costs = self.costs.read().await;
        let mut summary = HashMap::new();

        for entry in costs.iter() {
            *summary.entry(entry.provider.clone()).or_insert(0.0) += entry.cost_usd;
        }

        summary
    }

    fn calculate_cost(&self, provider: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        if let Some(costs) = self.provider_costs.get(provider) {
            let input_cost = (input_tokens as f64 / 1000.0) * costs.input_cost_per_1k;
            let output_cost = (output_tokens as f64 / 1000.0) * costs.output_cost_per_1k;
            input_cost + output_cost
        } else {
            0.0 // Unknown provider
        }
    }
}

impl CostTrackingPlugin {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub async fn before_request(&self, _request: &mut Request<Body>) -> Result<(), GatewayError> {
        // Could track request start time here
        Ok(())
    }

    pub async fn after_response(&self, response: &mut Response<Body>) -> Result<(), GatewayError> {
        // Extract cost information from response headers
        if let Some(cost_header) = response.headers().get("x-cost-usd") {
            if let Ok(cost_str) = cost_header.to_str() {
                if let Ok(cost) = cost_str.parse::<f64>() {
                    // Store the cost for this request
                    let entry = CostEntry {
                        timestamp: Utc::now(),
                        provider: "unknown".to_string(), // Would be extracted from response
                        model: "unknown".to_string(),    // Would be extracted from response
                        input_tokens: 0,                 // Would be extracted from response
                        output_tokens: 0,                // Would be extracted from response
                        cost_usd: cost,
                        request_id: uuid::Uuid::new_v4().to_string(),
                    };

                    self.add_cost(entry).await;
                }
            }
        }

        // Extract token information from response headers
        if let Some(tokens_header) = response.headers().get("x-tokens-used") {
            if let Ok(tokens_str) = tokens_header.to_str() {
                // Parse token information (format: "input:100,output:50")
                for token_info in tokens_str.split(',') {
                    if let Some((token_type, count)) = token_info.split_once(':') {
                        if let Ok(count) = count.parse::<u32>() {
                            // Store token information for cost calculation
                            tracing::debug!("Token usage - {}: {}", token_type, count);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    // Handler-level cost tracking methods
    pub async fn track_chat_completion_cost(
        &self,
        request: &ChatRequest,
        response: &ChatResponse,
        provider: &str,
        request_id: String,
    ) -> Result<(), GatewayError> {
        if let Some(usage) = &response.usage {
            let cost = self.calculate_cost(provider, usage.prompt_tokens, usage.completion_tokens);

            let entry = CostEntry {
                timestamp: Utc::now(),
                provider: provider.to_string(),
                model: request.model.clone(),
                input_tokens: usage.prompt_tokens,
                output_tokens: usage.completion_tokens,
                cost_usd: cost,
                request_id,
            };

            self.add_cost(entry).await;
        }
        Ok(())
    }

    pub async fn track_embedding_cost(
        &self,
        request: &EmbeddingRequest,
        response: &EmbeddingResponse,
        provider: &str,
        request_id: String,
    ) -> Result<(), GatewayError> {
        let usage = &response.usage;
        // For embeddings, all tokens are "input" tokens
        let cost = self.calculate_cost(provider, usage.total_tokens, 0);

        let entry = CostEntry {
            timestamp: Utc::now(),
            provider: provider.to_string(),
            model: request.model.clone(),
            input_tokens: usage.total_tokens,
            output_tokens: 0,
            cost_usd: cost,
            request_id,
        };

        self.add_cost(entry).await;
        Ok(())
    }

    pub async fn estimate_request_cost(
        &self,
        provider: &str,
        model: &str,
        estimated_input_tokens: u32,
        estimated_output_tokens: u32,
    ) -> f64 {
        // Use model-specific costs if available
        let provider_cost = if let Some(costs) = self.provider_costs.get(provider) {
            costs
        } else {
            // Fallback to OpenAI costs for unknown providers
            self.provider_costs.get("openai").unwrap_or(&ProviderCost {
                input_cost_per_1k: 0.03,
                output_cost_per_1k: 0.06,
            })
        };

        // Adjust costs based on model (simplified mapping)
        let (input_multiplier, output_multiplier) = match model {
            m if m.contains("gpt-4") => (1.0, 1.0),
            m if m.contains("gpt-3.5") => (0.1, 0.1),
            m if m.contains("claude-3-opus") => (1.5, 2.5),
            m if m.contains("claude-3-sonnet") => (0.3, 1.5),
            m if m.contains("claude-3-haiku") => (0.025, 0.125),
            _ => (1.0, 1.0), // Default multiplier
        };

        let input_cost = (estimated_input_tokens as f64 / 1000.0)
            * provider_cost.input_cost_per_1k
            * input_multiplier;
        let output_cost = (estimated_output_tokens as f64 / 1000.0)
            * provider_cost.output_cost_per_1k
            * output_multiplier;

        input_cost + output_cost
    }

    pub async fn on_error(&self, _error: &GatewayError) -> Result<(), GatewayError> {
        // Could track failed request costs here
        Ok(())
    }
}
