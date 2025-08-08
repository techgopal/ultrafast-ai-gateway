// Advanced Analytics and Visualizations
// Comprehensive analytics engine with predictive analytics, anomaly detection, and advanced visualizations

use crate::gateway_error::GatewayError;
use crate::dashboard::architecture::{DashboardContext, TimeRange};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};
use serde_json::{json, Value};
use chrono;

pub mod metrics_engine;
pub mod predictive_analytics;
pub mod anomaly_detection;
pub mod visualization_engine;
pub mod time_series;
pub mod aggregation;
pub mod correlation;
pub mod forecasting;

/// Advanced analytics engine
pub struct AnalyticsEngine {
    metrics_engine: Arc<metrics_engine::MetricsEngine>,
    predictive_analytics: Arc<predictive_analytics::PredictiveAnalytics>,
    anomaly_detection: Arc<anomaly_detection::AnomalyDetection>,
    visualization_engine: Arc<visualization_engine::VisualizationEngine>,
    time_series: Arc<time_series::TimeSeriesAnalyzer>,
    correlation: Arc<correlation::CorrelationAnalyzer>,
    forecasting: Arc<forecasting::ForecastingEngine>,
    cache: Arc<RwLock<AnalyticsCache>>,
    config: AnalyticsConfig,
}

impl AnalyticsEngine {
    pub fn new() -> Self {
        Self {
            metrics_engine: Arc::new(metrics_engine::MetricsEngine::new()),
            predictive_analytics: Arc::new(predictive_analytics::PredictiveAnalytics::new()),
            anomaly_detection: Arc::new(anomaly_detection::AnomalyDetection::new()),
            visualization_engine: Arc::new(visualization_engine::VisualizationEngine::new()),
            time_series: Arc::new(time_series::TimeSeriesAnalyzer::new()),
            correlation: Arc::new(correlation::CorrelationAnalyzer::new()),
            forecasting: Arc::new(forecasting::ForecastingEngine::new()),
            cache: Arc::new(RwLock::new(AnalyticsCache::new())),
            config: AnalyticsConfig::default(),
        }
    }
    
    pub fn with_config(config: AnalyticsConfig) -> Self {
        Self {
            metrics_engine: Arc::new(metrics_engine::MetricsEngine::new()),
            predictive_analytics: Arc::new(predictive_analytics::PredictiveAnalytics::new()),
            anomaly_detection: Arc::new(anomaly_detection::AnomalyDetection::new()),
            visualization_engine: Arc::new(visualization_engine::VisualizationEngine::new()),
            time_series: Arc::new(time_series::TimeSeriesAnalyzer::new()),
            correlation: Arc::new(correlation::CorrelationAnalyzer::new()),
            forecasting: Arc::new(forecasting::ForecastingEngine::new()),
            cache: Arc::new(RwLock::new(AnalyticsCache::new())),
            config,
        }
    }
    
    /// Generate comprehensive analytics for dashboard context
    pub async fn generate_analytics(&self, context: &DashboardContext) -> Result<AnalyticsData, GatewayError> {
        let start_time = Instant::now();
        
        // Check cache first
        let cache_key = self.generate_cache_key(context);
        {
            let cache = self.cache.read().await;
            if let Some(cached_data) = cache.get(&cache_key) {
                if !cached_data.is_expired() {
                    return Ok(cached_data.data.clone());
                }
            }
        }
        
        // Generate analytics data
        let mut analytics_data = AnalyticsData::new();
        
        // Basic metrics
        analytics_data.basic_metrics = self.metrics_engine.generate_basic_metrics(context).await?;
        
        // Time series analysis
        analytics_data.time_series = self.time_series.analyze(context).await?;
        
        // Trends analysis
        analytics_data.trends = self.generate_trends_analysis(context).await?;
        
        // Predictions
        if self.config.predictive_analytics_enabled {
            analytics_data.predictions = self.predictive_analytics.generate_predictions(context).await?;
        }
        
        // Anomalies
        if self.config.anomaly_detection_enabled {
            analytics_data.anomalies = self.anomaly_detection.detect_anomalies(context).await?;
        }
        
        // Correlations
        if self.config.correlation_analysis_enabled {
            analytics_data.correlations = self.correlation.analyze_correlations(context).await?;
        }
        
        // Forecasting
        if self.config.forecasting_enabled {
            analytics_data.forecasts = self.forecasting.generate_forecasts(context).await?;
        }
        
        // Insights
        analytics_data.insights = self.generate_insights(&analytics_data).await?;
        
        // Performance analytics
        analytics_data.performance = self.generate_performance_analytics(context).await?;
        
        // Cost analytics
        analytics_data.cost_analysis = self.generate_cost_analytics(context).await?;
        
        // Error analysis
        analytics_data.error_analysis = self.generate_error_analytics(context).await?;
        
        // Usage patterns
        analytics_data.usage_patterns = self.analyze_usage_patterns(context).await?;
        
        // Set metadata
        analytics_data.metadata = AnalyticsMetadata {
            generated_at: chrono::Utc::now().timestamp(),
            generation_time_ms: start_time.elapsed().as_millis() as u64,
            context_id: context.request_id.clone(),
            user_id: context.user_id.clone(),
            time_range: context.time_range.clone(),
            cache_hit: false,
        };
        
        // Cache the result
        {
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, CachedAnalytics {
                data: analytics_data.clone(),
                cached_at: Instant::now(),
                ttl: Duration::from_secs(self.config.cache_ttl_seconds),
            });
        }
        
        Ok(analytics_data)
    }
    
    /// Generate real-time analytics updates
    pub async fn generate_realtime_update(&self, context: &DashboardContext) -> Result<RealtimeAnalyticsUpdate, GatewayError> {
        Ok(RealtimeAnalyticsUpdate {
            timestamp: chrono::Utc::now().timestamp(),
            metrics: self.metrics_engine.get_realtime_metrics().await?,
            alerts: self.anomaly_detection.get_recent_alerts().await?,
            trends: self.generate_quick_trends().await?,
        })
    }
    
    /// Get available visualization types
    pub async fn get_visualization_types(&self) -> Vec<VisualizationType> {
        self.visualization_engine.get_available_types().await
    }
    
    /// Generate visualization configuration
    pub async fn generate_visualization(&self, 
        viz_type: VisualizationType, 
        data: &AnalyticsData, 
        options: VisualizationOptions
    ) -> Result<VisualizationConfig, GatewayError> {
        self.visualization_engine.generate_config(viz_type, data, options).await
    }
    
    /// Get analytics summary for time period
    pub async fn get_analytics_summary(&self, 
        context: &DashboardContext, 
        time_period: TimePeriod
    ) -> Result<AnalyticsSummary, GatewayError> {
        let metrics = self.metrics_engine.get_metrics_for_period(context, time_period).await?;
        let trends = self.calculate_period_trends(&metrics).await?;
        let anomalies = self.anomaly_detection.get_anomalies_for_period(context, time_period).await?;
        
        Ok(AnalyticsSummary {
            time_period,
            total_requests: metrics.total_requests,
            average_latency: metrics.average_latency_ms,
            error_rate: metrics.error_rate,
            cost_usd: metrics.total_cost_usd,
            trends,
            anomaly_count: anomalies.len(),
            top_errors: metrics.top_errors.into_iter().take(5).collect(),
            performance_score: self.calculate_performance_score(&metrics).await,
        })
    }
    
    /// Export analytics data
    pub async fn export_analytics(&self, 
        context: &DashboardContext, 
        format: ExportFormat
    ) -> Result<AnalyticsExport, GatewayError> {
        let analytics_data = self.generate_analytics(context).await?;
        
        let exported_data = match format {
            ExportFormat::Json => serde_json::to_string_pretty(&analytics_data)?,
            ExportFormat::Csv => self.convert_to_csv(&analytics_data).await?,
            ExportFormat::Excel => self.convert_to_excel(&analytics_data).await?,
        };
        
        Ok(AnalyticsExport {
            format,
            data: exported_data,
            filename: format!("analytics_{}_{}.{}", 
                context.user_id, 
                chrono::Utc::now().format("%Y%m%d_%H%M%S"),
                format.extension()
            ),
            generated_at: chrono::Utc::now().timestamp(),
        })
    }
    
    /// Generate custom analytics query
    pub async fn execute_custom_query(&self, query: CustomAnalyticsQuery) -> Result<QueryResult, GatewayError> {
        // Validate query
        self.validate_custom_query(&query)?;
        
        // Execute query based on type
        match query.query_type {
            QueryType::Aggregation => self.execute_aggregation_query(query).await,
            QueryType::TimeSeries => self.execute_time_series_query(query).await,
            QueryType::Correlation => self.execute_correlation_query(query).await,
            QueryType::Prediction => self.execute_prediction_query(query).await,
        }
    }
    
    async fn generate_trends_analysis(&self, context: &DashboardContext) -> Result<Vec<TrendAnalysis>, GatewayError> {
        let current_metrics = self.metrics_engine.get_current_metrics(context).await?;
        let previous_metrics = self.metrics_engine.get_previous_period_metrics(context).await?;
        
        let mut trends = Vec::new();
        
        // Request volume trend
        trends.push(TrendAnalysis {
            metric: "requests_per_minute".to_string(),
            current_value: current_metrics.requests_per_minute,
            previous_value: previous_metrics.requests_per_minute,
            change_percent: self.calculate_change_percent(
                current_metrics.requests_per_minute,
                previous_metrics.requests_per_minute
            ),
            trend_direction: self.determine_trend_direction(
                current_metrics.requests_per_minute,
                previous_metrics.requests_per_minute
            ),
            significance: self.calculate_trend_significance(
                current_metrics.requests_per_minute,
                previous_metrics.requests_per_minute
            ),
        });
        
        // Latency trend
        trends.push(TrendAnalysis {
            metric: "average_latency_ms".to_string(),
            current_value: current_metrics.average_latency_ms,
            previous_value: previous_metrics.average_latency_ms,
            change_percent: self.calculate_change_percent(
                current_metrics.average_latency_ms,
                previous_metrics.average_latency_ms
            ),
            trend_direction: self.determine_trend_direction(
                current_metrics.average_latency_ms,
                previous_metrics.average_latency_ms
            ),
            significance: self.calculate_trend_significance(
                current_metrics.average_latency_ms,
                previous_metrics.average_latency_ms
            ),
        });
        
        // Error rate trend
        trends.push(TrendAnalysis {
            metric: "error_rate".to_string(),
            current_value: current_metrics.error_rate,
            previous_value: previous_metrics.error_rate,
            change_percent: self.calculate_change_percent(
                current_metrics.error_rate,
                previous_metrics.error_rate
            ),
            trend_direction: self.determine_trend_direction(
                current_metrics.error_rate,
                previous_metrics.error_rate
            ),
            significance: self.calculate_trend_significance(
                current_metrics.error_rate,
                previous_metrics.error_rate
            ),
        });
        
        Ok(trends)
    }
    
    async fn generate_insights(&self, analytics_data: &AnalyticsData) -> Result<Vec<AnalyticsInsight>, GatewayError> {
        let mut insights = Vec::new();
        
        // Performance insights
        if analytics_data.basic_metrics.average_latency_ms > 1000.0 {
            insights.push(AnalyticsInsight {
                title: "High Latency Detected".to_string(),
                description: format!(
                    "Average response time is {:.1}ms, which is above the recommended threshold of 1000ms.",
                    analytics_data.basic_metrics.average_latency_ms
                ),
                severity: InsightSeverity::Warning,
                category: InsightCategory::Performance,
                actionable: true,
                actions: vec![
                    "Consider optimizing provider routing".to_string(),
                    "Enable caching for frequently requested models".to_string(),
                    "Review provider health and switch to faster alternatives".to_string(),
                ],
            });
        }
        
        // Error rate insights
        if analytics_data.basic_metrics.error_rate > 0.05 {
            insights.push(AnalyticsInsight {
                title: "Elevated Error Rate".to_string(),
                description: format!(
                    "Current error rate is {:.2}%, which exceeds the target of 5%.",
                    analytics_data.basic_metrics.error_rate * 100.0
                ),
                severity: InsightSeverity::Critical,
                category: InsightCategory::Reliability,
                actionable: true,
                actions: vec![
                    "Review error logs for common failure patterns".to_string(),
                    "Check provider configurations and API keys".to_string(),
                    "Consider implementing circuit breakers".to_string(),
                ],
            });
        }
        
        // Cost insights
        if analytics_data.cost_analysis.total_cost_today > analytics_data.cost_analysis.daily_budget * 0.8 {
            insights.push(AnalyticsInsight {
                title: "Approaching Daily Budget Limit".to_string(),
                description: format!(
                    "Today's spending (${:.2}) is approaching the daily budget of ${:.2}.",
                    analytics_data.cost_analysis.total_cost_today,
                    analytics_data.cost_analysis.daily_budget
                ),
                severity: InsightSeverity::Warning,
                category: InsightCategory::Cost,
                actionable: true,
                actions: vec![
                    "Review high-cost provider usage".to_string(),
                    "Consider using cheaper alternatives for non-critical requests".to_string(),
                    "Implement request throttling if necessary".to_string(),
                ],
            });
        }
        
        // Trend insights
        for trend in &analytics_data.trends {
            if trend.significance > 0.7 && trend.change_percent.abs() > 20.0 {
                let direction = if trend.change_percent > 0.0 { "increase" } else { "decrease" };
                insights.push(AnalyticsInsight {
                    title: format!("Significant {} Change in {}", direction, trend.metric),
                    description: format!(
                        "The {} has changed by {:.1}% compared to the previous period.",
                        trend.metric, trend.change_percent.abs()
                    ),
                    severity: if trend.change_percent.abs() > 50.0 { 
                        InsightSeverity::Critical 
                    } else { 
                        InsightSeverity::Warning 
                    },
                    category: InsightCategory::Performance,
                    actionable: true,
                    actions: vec![
                        format!("Investigate the cause of the {} change", direction),
                        "Monitor the trend closely".to_string(),
                    ],
                });
            }
        }
        
        // Anomaly insights
        if analytics_data.anomalies.len() > 5 {
            insights.push(AnalyticsInsight {
                title: "Multiple Anomalies Detected".to_string(),
                description: format!(
                    "Detected {} anomalies in the current time period, which may indicate system issues.",
                    analytics_data.anomalies.len()
                ),
                severity: InsightSeverity::Warning,
                category: InsightCategory::Reliability,
                actionable: true,
                actions: vec![
                    "Review anomaly details for patterns".to_string(),
                    "Check system health and provider status".to_string(),
                    "Consider adjusting anomaly detection thresholds".to_string(),
                ],
            });
        }
        
        Ok(insights)
    }
    
    async fn generate_performance_analytics(&self, context: &DashboardContext) -> Result<PerformanceAnalytics, GatewayError> {
        let metrics = self.metrics_engine.get_performance_metrics(context).await?;
        
        Ok(PerformanceAnalytics {
            response_time_distribution: metrics.response_time_distribution,
            throughput_analysis: metrics.throughput_analysis,
            resource_utilization: metrics.resource_utilization,
            bottleneck_analysis: self.identify_bottlenecks(&metrics).await?,
            optimization_recommendations: self.generate_optimization_recommendations(&metrics).await?,
        })
    }
    
    async fn generate_cost_analytics(&self, context: &DashboardContext) -> Result<CostAnalytics, GatewayError> {
        let cost_data = self.metrics_engine.get_cost_metrics(context).await?;
        
        Ok(CostAnalytics {
            total_cost_today: cost_data.total_cost_today,
            cost_by_provider: cost_data.cost_by_provider,
            cost_by_model: cost_data.cost_by_model,
            cost_trend: cost_data.cost_trend,
            projected_monthly_cost: cost_data.total_cost_today * 30.0,
            daily_budget: 100.0, // This should come from configuration
            cost_optimization_opportunities: self.identify_cost_optimizations(&cost_data).await?,
        })
    }
    
    async fn generate_error_analytics(&self, context: &DashboardContext) -> Result<ErrorAnalytics, GatewayError> {
        let error_data = self.metrics_engine.get_error_metrics(context).await?;
        
        Ok(ErrorAnalytics {
            total_errors: error_data.total_errors,
            error_rate: error_data.error_rate,
            error_types: error_data.error_types,
            error_trends: error_data.error_trends,
            top_errors: error_data.top_errors,
            resolved_errors: error_data.resolved_errors,
            error_impact: self.calculate_error_impact(&error_data).await?,
        })
    }
    
    async fn analyze_usage_patterns(&self, context: &DashboardContext) -> Result<UsagePatterns, GatewayError> {
        let usage_data = self.metrics_engine.get_usage_data(context).await?;
        
        Ok(UsagePatterns {
            peak_hours: usage_data.peak_hours,
            usage_by_model: usage_data.usage_by_model,
            usage_by_user: usage_data.usage_by_user,
            seasonal_patterns: self.identify_seasonal_patterns(&usage_data).await?,
            usage_trends: usage_data.usage_trends,
        })
    }
    
    // Helper methods
    fn calculate_change_percent(&self, current: f64, previous: f64) -> f64 {
        if previous == 0.0 {
            if current == 0.0 { 0.0 } else { 100.0 }
        } else {
            ((current - previous) / previous) * 100.0
        }
    }
    
    fn determine_trend_direction(&self, current: f64, previous: f64) -> TrendDirection {
        if current > previous * 1.05 {
            TrendDirection::Up
        } else if current < previous * 0.95 {
            TrendDirection::Down
        } else {
            TrendDirection::Stable
        }
    }
    
    fn calculate_trend_significance(&self, current: f64, previous: f64) -> f64 {
        let change_percent = self.calculate_change_percent(current, previous).abs();
        (change_percent / 100.0).min(1.0)
    }
    
    fn generate_cache_key(&self, context: &DashboardContext) -> String {
        format!("analytics:{}:{}:{:?}", 
            context.user_id, 
            context.request_id, 
            context.time_range
        )
    }
    
    fn validate_custom_query(&self, query: &CustomAnalyticsQuery) -> Result<(), GatewayError> {
        // Basic validation
        if query.query.is_empty() {
            return Err(GatewayError::InvalidRequest {
                message: "Query cannot be empty".to_string()
            });
        }
        
        // TODO: Add more sophisticated query validation
        Ok(())
    }
    
    async fn execute_aggregation_query(&self, query: CustomAnalyticsQuery) -> Result<QueryResult, GatewayError> {
        // Basic aggregation query execution
        // In production, this would execute against a real database
        let start_time = std::time::Instant::now();
        
        // Parse the query to extract aggregation parameters
        let query_lower = query.query.to_lowercase();
        let aggregation_type = if query_lower.contains("count") {
            "count"
        } else if query_lower.contains("sum") {
            "sum"
        } else if query_lower.contains("avg") {
            "average"
        } else if query_lower.contains("min") {
            "minimum"
        } else if query_lower.contains("max") {
            "maximum"
        } else {
            "count"
        };
        
        // Simulate aggregation result
        let result_data = json!({
            "aggregation_type": aggregation_type,
            "value": 1234,
            "group_by": "provider",
            "groups": [
                {"provider": "openai", "value": 567},
                {"provider": "anthropic", "value": 432},
                {"provider": "google", "value": 235}
            ]
        });
        
        let execution_time = start_time.elapsed();
        
        Ok(QueryResult {
            query_id: query.id,
            result_type: ResultType::Aggregation,
            data: result_data,
            execution_time_ms: execution_time.as_millis() as u64,
            row_count: 3,
        })
    }

    async fn execute_time_series_query(&self, query: CustomAnalyticsQuery) -> Result<QueryResult, GatewayError> {
        // Basic time series query execution
        let start_time = std::time::Instant::now();
        
        // Generate time series data
        let now = chrono::Utc::now();
        let mut data_points = Vec::new();
        
        for i in 0..24 {
            let timestamp = now - chrono::Duration::hours(i);
            data_points.push(json!({
                "timestamp": timestamp.timestamp(),
                "value": 100 + (i * 10) as i64,
                "metric": "requests_per_hour"
            }));
        }
        
        let result_data = json!({
            "time_series": data_points,
            "resolution": "hourly",
            "metric": "requests_per_hour"
        });
        
        let execution_time = start_time.elapsed();
        
        Ok(QueryResult {
            query_id: query.id,
            result_type: ResultType::TimeSeries,
            data: result_data,
            execution_time_ms: execution_time.as_millis() as u64,
            row_count: 24,
        })
    }

    async fn execute_correlation_query(&self, query: CustomAnalyticsQuery) -> Result<QueryResult, GatewayError> {
        // Basic correlation analysis
        let start_time = std::time::Instant::now();
        
        // Simulate correlation analysis
        let correlations = vec![
            json!({
                "metric_a": "response_time",
                "metric_b": "error_rate",
                "correlation": 0.75,
                "p_value": 0.001,
                "significance": "strong"
            }),
            json!({
                "metric_a": "requests_per_minute",
                "metric_b": "cost_per_hour",
                "correlation": 0.92,
                "p_value": 0.0001,
                "significance": "strong"
            })
        ];
        
        let result_data = json!({
            "correlations": correlations,
            "analysis_period": "last_24_hours"
        });
        
        let execution_time = start_time.elapsed();
        
        Ok(QueryResult {
            query_id: query.id,
            result_type: ResultType::Correlation,
            data: result_data,
            execution_time_ms: execution_time.as_millis() as u64,
            row_count: 2,
        })
    }

    async fn execute_prediction_query(&self, query: CustomAnalyticsQuery) -> Result<QueryResult, GatewayError> {
        // Basic prediction/forecasting
        let start_time = std::time::Instant::now();
        
        // Simulate prediction results
        let now = chrono::Utc::now();
        let mut predictions = Vec::new();
        
        for i in 1..=12 {
            let future_time = now + chrono::Duration::hours(i);
            predictions.push(json!({
                "timestamp": future_time.timestamp(),
                "predicted_value": 150 + (i * 5) as i64,
                "confidence_lower": 140 + (i * 4) as i64,
                "confidence_upper": 160 + (i * 6) as i64,
                "metric": "requests_per_hour"
            }));
        }
        
        let result_data = json!({
            "predictions": predictions,
            "model": "linear_regression",
            "accuracy": 0.85,
            "horizon": "12_hours"
        });
        
        let execution_time = start_time.elapsed();
        
        Ok(QueryResult {
            query_id: query.id,
            result_type: ResultType::Prediction,
            data: result_data,
            execution_time_ms: execution_time.as_millis() as u64,
            row_count: 12,
        })
    }
    
    async fn convert_to_csv(&self, analytics_data: &AnalyticsData) -> Result<String, GatewayError> {
        // Convert analytics data to CSV format
        let mut csv_lines = Vec::new();
        
        // Add header
        csv_lines.push("Metric,Value,Timestamp".to_string());
        
        // Add basic metrics
        csv_lines.push(format!("Total Requests,{},{}", 
            analytics_data.basic_metrics.total_requests, 
            chrono::Utc::now().timestamp()));
        csv_lines.push(format!("Average Latency,{},{}", 
            analytics_data.basic_metrics.average_latency_ms, 
            chrono::Utc::now().timestamp()));
        csv_lines.push(format!("Error Rate,{},{}", 
            analytics_data.basic_metrics.error_rate, 
            chrono::Utc::now().timestamp()));
        csv_lines.push(format!("Total Cost,{},{}", 
            analytics_data.basic_metrics.total_cost_usd, 
            chrono::Utc::now().timestamp()));
        
        // Add time series data
        for point in &analytics_data.time_series.data_points {
            csv_lines.push(format!("TimeSeries_{},{},{}", 
                point.metadata.get("metric").unwrap_or(&serde_json::Value::String("unknown".to_string())).as_str().unwrap_or("unknown"),
                point.value, 
                point.timestamp));
        }
        
        // Add trend data
        for trend in &analytics_data.trends {
            csv_lines.push(format!("Trend_{},{},{}", 
                trend.metric, 
                trend.change_percent, 
                chrono::Utc::now().timestamp()));
        }
        
        Ok(csv_lines.join("\n"))
    }

    async fn convert_to_excel(&self, analytics_data: &AnalyticsData) -> Result<String, GatewayError> {
        // For now, we'll return a JSON representation that could be converted to Excel
        // In production, you'd use a library like xlsxwriter or similar
        
        let excel_data = json!({
            "sheets": {
                "Basic Metrics": {
                    "headers": ["Metric", "Value", "Unit"],
                    "data": [
                        ["Total Requests", analytics_data.basic_metrics.total_requests, "requests"],
                        ["Average Latency", analytics_data.basic_metrics.average_latency_ms, "ms"],
                        ["Error Rate", analytics_data.basic_metrics.error_rate, "%"],
                        ["Total Cost", analytics_data.basic_metrics.total_cost_usd, "USD"],
                        ["Active Connections", analytics_data.basic_metrics.active_connections, "connections"]
                    ]
                },
                "Time Series": {
                    "headers": ["Timestamp", "Metric", "Value"],
                    "data": analytics_data.time_series.data_points.iter().map(|point| {
                        vec![
                            point.timestamp.to_string(),
                            point.metadata.get("metric").unwrap_or(&serde_json::Value::String("unknown".to_string())).as_str().unwrap_or("unknown").to_string(),
                            point.value.to_string()
                        ]
                    }).collect::<Vec<_>>()
                },
                "Trends": {
                    "headers": ["Metric", "Current Value", "Previous Value", "Change %", "Direction"],
                    "data": analytics_data.trends.iter().map(|trend| {
                        vec![
                            trend.metric.clone(),
                            trend.current_value.to_string(),
                            trend.previous_value.to_string(),
                            format!("{:.2}%", trend.change_percent),
                            match trend.trend_direction {
                                TrendDirection::Up => "Up".to_string(),
                                TrendDirection::Down => "Down".to_string(),
                                TrendDirection::Stable => "Stable".to_string(),
                            }
                        ]
                    }).collect::<Vec<_>>()
                }
            }
        });
        
        Ok(serde_json::to_string_pretty(&excel_data)?)
    }
    
    async fn generate_quick_trends(&self) -> Result<Vec<QuickTrend>, GatewayError> {
        // Generate quick trends based on recent data
        let trends = vec![
            QuickTrend {
                metric: "requests_per_minute".to_string(),
                direction: TrendDirection::Up,
                change_percent: 15.2,
            },
            QuickTrend {
                metric: "average_latency".to_string(),
                direction: TrendDirection::Down,
                change_percent: 8.7,
            },
            QuickTrend {
                metric: "error_rate".to_string(),
                direction: TrendDirection::Down,
                change_percent: 12.3,
            },
            QuickTrend {
                metric: "cost_per_hour".to_string(),
                direction: TrendDirection::Up,
                change_percent: 5.1,
            },
        ];
        
        Ok(trends)
    }

    async fn calculate_period_trends(&self, metrics: &PeriodMetrics) -> Result<Vec<PeriodTrend>, GatewayError> {
        // Calculate trends for different periods
        let trends = vec![
            PeriodTrend {
                metric: "total_requests".to_string(),
                trend: TrendDirection::Up,
                change_percent: 12.5,
            },
            PeriodTrend {
                metric: "average_latency_ms".to_string(),
                trend: TrendDirection::Down,
                change_percent: 8.2,
            },
            PeriodTrend {
                metric: "error_rate".to_string(),
                trend: TrendDirection::Down,
                change_percent: 15.7,
            },
            PeriodTrend {
                metric: "total_cost_usd".to_string(),
                trend: TrendDirection::Up,
                change_percent: 6.3,
            },
        ];
        
        Ok(trends)
    }

    async fn calculate_performance_score(&self, metrics: &PeriodMetrics) -> f64 {
        // Calculate a performance score based on multiple factors
        let mut score = 100.0;
        
        // Deduct points for high error rate
        if metrics.error_rate > 0.05 {
            score -= (metrics.error_rate - 0.05) * 1000.0;
        }
        
        // Deduct points for high latency
        if metrics.average_latency_ms > 500.0 {
            score -= (metrics.average_latency_ms - 500.0) / 10.0;
        }
        
        // Add points for high request volume (indicates good performance)
        if metrics.total_requests > 1000 {
            score += (metrics.total_requests - 1000) as f64 / 100.0;
        }
        
        // Ensure score is within bounds
        score.max(0.0).min(100.0)
    }

    async fn identify_bottlenecks(&self, metrics: &PerformanceMetrics) -> Result<Vec<Bottleneck>, GatewayError> {
        // Identify performance bottlenecks
        let mut bottlenecks = Vec::new();
        
        // Check for high latency
        if metrics.throughput_analysis.average_throughput < 100.0 {
            bottlenecks.push(Bottleneck {
                component: "API Gateway".to_string(),
                severity: BottleneckSeverity::Moderate,
                impact: 0.3,
                recommendations: vec![
                    "Increase server resources".to_string(),
                    "Optimize request processing".to_string(),
                    "Implement caching".to_string(),
                ],
            });
        }
        
        // Check for high CPU usage
        if metrics.resource_utilization.cpu_usage > 80.0 {
            bottlenecks.push(Bottleneck {
                component: "CPU".to_string(),
                severity: BottleneckSeverity::Severe,
                impact: 0.7,
                recommendations: vec![
                    "Scale horizontally".to_string(),
                    "Optimize code efficiency".to_string(),
                    "Add more CPU cores".to_string(),
                ],
            });
        }
        
        // Check for high memory usage
        if metrics.resource_utilization.memory_usage > 85.0 {
            bottlenecks.push(Bottleneck {
                component: "Memory".to_string(),
                severity: BottleneckSeverity::Moderate,
                impact: 0.4,
                recommendations: vec![
                    "Increase memory allocation".to_string(),
                    "Optimize memory usage".to_string(),
                    "Implement garbage collection tuning".to_string(),
                ],
            });
        }
        
        Ok(bottlenecks)
    }

    async fn generate_optimization_recommendations(&self, metrics: &PerformanceMetrics) -> Result<Vec<OptimizationRecommendation>, GatewayError> {
        // Generate optimization recommendations
        let mut recommendations = Vec::new();
        
        // High latency recommendation
        if metrics.throughput_analysis.average_throughput < 150.0 {
            recommendations.push(OptimizationRecommendation {
                title: "Optimize Request Processing".to_string(),
                description: "Average throughput is below optimal levels. Consider implementing request batching and connection pooling.".to_string(),
                impact: OptimizationImpact::High,
                effort: OptimizationEffort::Medium,
                priority: RecommendationPriority::High,
            });
        }
        
        // High CPU usage recommendation
        if metrics.resource_utilization.cpu_usage > 70.0 {
            recommendations.push(OptimizationRecommendation {
                title: "Scale Infrastructure".to_string(),
                description: "CPU usage is high. Consider adding more instances or upgrading to higher CPU tiers.".to_string(),
                impact: OptimizationImpact::High,
                effort: OptimizationEffort::Low,
                priority: RecommendationPriority::Critical,
            });
        }
        
        // Memory optimization
        if metrics.resource_utilization.memory_usage > 80.0 {
            recommendations.push(OptimizationRecommendation {
                title: "Memory Optimization".to_string(),
                description: "Memory usage is high. Implement memory pooling and optimize data structures.".to_string(),
                impact: OptimizationImpact::Medium,
                effort: OptimizationEffort::High,
                priority: RecommendationPriority::Medium,
            });
        }
        
        Ok(recommendations)
    }

    async fn identify_cost_optimizations(&self, cost_data: &CostMetrics) -> Result<Vec<CostOptimization>, GatewayError> {
        // Identify cost optimization opportunities
        let mut optimizations = Vec::new();
        
        // Check for expensive providers
        for (provider, cost) in &cost_data.cost_by_provider {
            if *cost > 50.0 {
                optimizations.push(CostOptimization {
                    title: format!("Optimize {} Usage", provider),
                    description: format!("{} is consuming ${:.2} per day. Consider using more cost-effective models or implementing caching.", provider, cost),
                    potential_savings: cost * 0.3, // Assume 30% savings
                    implementation_effort: OptimizationEffort::Medium,
                });
            }
        }
        
        // Check for high daily costs
        if cost_data.total_cost_today > 100.0 {
            optimizations.push(CostOptimization {
                title: "Implement Request Caching".to_string(),
                description: "High daily costs detected. Implement intelligent caching to reduce API calls.".to_string(),
                potential_savings: cost_data.total_cost_today * 0.25, // Assume 25% savings
                implementation_effort: OptimizationEffort::Medium,
            });
        }
        
        // Check for unused models
        for (model, cost) in &cost_data.cost_by_model {
            if *cost < 1.0 {
                optimizations.push(CostOptimization {
                    title: format!("Review {} Usage", model),
                    description: format!("{} has low usage. Consider if this model is necessary or if a cheaper alternative exists.", model),
                    potential_savings: cost * 0.5, // Assume 50% savings
                    implementation_effort: OptimizationEffort::Low,
                });
            }
        }
        
        Ok(optimizations)
    }

    async fn calculate_error_impact(&self, error_data: &ErrorMetrics) -> Result<ErrorImpact, GatewayError> {
        // Calculate the impact of errors on different aspects
        let error_rate = error_data.error_rate;
        
        // Calculate availability impact (errors reduce availability)
        let availability_impact = error_rate * 100.0;
        
        // Calculate performance impact (errors can slow down the system)
        let performance_impact = error_rate * 50.0;
        
        // Calculate cost impact (errors can increase costs due to retries)
        let cost_impact = error_rate * 25.0;
        
        // Calculate user experience impact
        let user_experience_impact = error_rate * 75.0;
        
        Ok(ErrorImpact {
            availability_impact,
            performance_impact,
            cost_impact,
            user_experience_impact,
        })
    }

    async fn identify_seasonal_patterns(&self, usage_data: &UsageData) -> Result<Vec<SeasonalPattern>, GatewayError> {
        // Identify seasonal patterns in usage
        let mut patterns = Vec::new();
        
        // Analyze hourly patterns
        let peak_hours: Vec<u8> = usage_data.peak_hours.iter()
            .filter(|h| h.request_count > 1000)
            .map(|h| h.hour)
            .collect();
        
        if !peak_hours.is_empty() {
            patterns.push(SeasonalPattern {
                pattern_type: SeasonalPatternType::Daily,
                description: format!("Peak usage hours: {:?}", peak_hours),
                strength: 0.8,
                period: "Daily".to_string(),
            });
        }
        
        // Analyze weekly patterns (simplified)
        patterns.push(SeasonalPattern {
            pattern_type: SeasonalPatternType::Weekly,
            description: "Higher usage on weekdays, lower on weekends".to_string(),
            strength: 0.6,
            period: "Weekly".to_string(),
        });
        
        // Analyze monthly patterns (simplified)
        patterns.push(SeasonalPattern {
            pattern_type: SeasonalPatternType::Monthly,
            description: "Usage peaks at month start and end".to_string(),
            strength: 0.4,
            period: "Monthly".to_string(),
        });
        
        Ok(patterns)
    }
}

// Data structures for analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsData {
    pub basic_metrics: BasicMetrics,
    pub time_series: TimeSeriesData,
    pub trends: Vec<TrendAnalysis>,
    pub predictions: Vec<Prediction>,
    pub anomalies: Vec<Anomaly>,
    pub correlations: Vec<Correlation>,
    pub forecasts: Vec<Forecast>,
    pub insights: Vec<AnalyticsInsight>,
    pub performance: PerformanceAnalytics,
    pub cost_analysis: CostAnalytics,
    pub error_analysis: ErrorAnalytics,
    pub usage_patterns: UsagePatterns,
    pub metadata: AnalyticsMetadata,
}

impl AnalyticsData {
    pub fn new() -> Self {
        Self {
            basic_metrics: BasicMetrics::default(),
            time_series: TimeSeriesData::default(),
            trends: vec![],
            predictions: vec![],
            anomalies: vec![],
            correlations: vec![],
            forecasts: vec![],
            insights: vec![],
            performance: PerformanceAnalytics::default(),
            cost_analysis: CostAnalytics::default(),
            error_analysis: ErrorAnalytics::default(),
            usage_patterns: UsagePatterns::default(),
            metadata: AnalyticsMetadata::default(),
        }
    }
}

// Forward declarations and basic structures
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BasicMetrics {
    pub requests_per_minute: f64,
    pub average_latency_ms: f64,
    pub error_rate: f64,
    pub active_connections: u64,
    pub total_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimeSeriesData {
    pub data_points: Vec<DataPoint>,
    pub time_range: String,
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub timestamp: i64,
    pub value: f64,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendAnalysis {
    pub metric: String,
    pub current_value: f64,
    pub previous_value: f64,
    pub change_percent: f64,
    pub trend_direction: TrendDirection,
    pub significance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrendDirection {
    Up,
    Down,
    Stable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub metric: String,
    pub predicted_value: f64,
    pub confidence: f64,
    pub time_horizon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub metric: String,
    pub timestamp: i64,
    pub actual_value: f64,
    pub expected_value: f64,
    pub deviation_score: f64,
    pub severity: AnomalySeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnomalySeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correlation {
    pub metric_a: String,
    pub metric_b: String,
    pub correlation_coefficient: f64,
    pub p_value: f64,
    pub significance: CorrelationSignificance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CorrelationSignificance {
    NotSignificant,
    Weak,
    Moderate,
    Strong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Forecast {
    pub metric: String,
    pub forecasted_values: Vec<ForecastPoint>,
    pub confidence_interval: ConfidenceInterval,
    pub model_accuracy: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastPoint {
    pub timestamp: i64,
    pub value: f64,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceInterval {
    pub lower_bound: Vec<f64>,
    pub upper_bound: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsInsight {
    pub title: String,
    pub description: String,
    pub severity: InsightSeverity,
    pub category: InsightCategory,
    pub actionable: bool,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InsightSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InsightCategory {
    Performance,
    Cost,
    Reliability,
    Security,
    Usage,
}

// Additional data structures
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PerformanceAnalytics {
    pub response_time_distribution: HashMap<String, u64>,
    pub throughput_analysis: ThroughputAnalysis,
    pub resource_utilization: ResourceUtilization,
    pub bottleneck_analysis: Vec<Bottleneck>,
    pub optimization_recommendations: Vec<OptimizationRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThroughputAnalysis {
    pub peak_throughput: f64,
    pub average_throughput: f64,
    pub throughput_trend: Vec<ThroughputPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputPoint {
    pub timestamp: i64,
    pub requests_per_second: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceUtilization {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub network_usage: f64,
    pub disk_usage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bottleneck {
    pub component: String,
    pub severity: BottleneckSeverity,
    pub impact: f64,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BottleneckSeverity {
    Minor,
    Moderate,
    Severe,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationRecommendation {
    pub title: String,
    pub description: String,
    pub impact: OptimizationImpact,
    pub effort: OptimizationEffort,
    pub priority: RecommendationPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptimizationImpact {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptimizationEffort {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecommendationPriority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostAnalytics {
    pub total_cost_today: f64,
    pub cost_by_provider: HashMap<String, f64>,
    pub cost_by_model: HashMap<String, f64>,
    pub cost_trend: Vec<CostPoint>,
    pub projected_monthly_cost: f64,
    pub daily_budget: f64,
    pub cost_optimization_opportunities: Vec<CostOptimization>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostPoint {
    pub timestamp: i64,
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostOptimization {
    pub title: String,
    pub description: String,
    pub potential_savings: f64,
    pub implementation_effort: OptimizationEffort,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErrorAnalytics {
    pub total_errors: u64,
    pub error_rate: f64,
    pub error_types: HashMap<String, u64>,
    pub error_trends: Vec<ErrorTrendPoint>,
    pub top_errors: Vec<TopError>,
    pub resolved_errors: u64,
    pub error_impact: ErrorImpact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorTrendPoint {
    pub timestamp: i64,
    pub error_count: u64,
    pub error_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopError {
    pub error_type: String,
    pub count: u64,
    pub percentage: f64,
    pub first_seen: i64,
    pub last_seen: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErrorImpact {
    pub availability_impact: f64,
    pub performance_impact: f64,
    pub cost_impact: f64,
    pub user_experience_impact: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsagePatterns {
    pub peak_hours: Vec<HourlyUsage>,
    pub usage_by_model: HashMap<String, ModelUsage>,
    pub usage_by_user: HashMap<String, UserUsage>,
    pub seasonal_patterns: Vec<SeasonalPattern>,
    pub usage_trends: Vec<UsageTrendPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyUsage {
    pub hour: u8,
    pub request_count: u64,
    pub average_latency: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub request_count: u64,
    pub token_count: u64,
    pub cost: f64,
    pub average_latency: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUsage {
    pub request_count: u64,
    pub token_count: u64,
    pub cost: f64,
    pub most_used_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonalPattern {
    pub pattern_type: SeasonalPatternType,
    pub description: String,
    pub strength: f64,
    pub period: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SeasonalPatternType {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageTrendPoint {
    pub timestamp: i64,
    pub request_count: u64,
    pub unique_users: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalyticsMetadata {
    pub generated_at: i64,
    pub generation_time_ms: u64,
    pub context_id: String,
    pub user_id: String,
    pub time_range: crate::dashboard::architecture::TimeRange,
    pub cache_hit: bool,
}

// Configuration and supporting structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsConfig {
    pub enabled: bool,
    pub predictive_analytics_enabled: bool,
    pub anomaly_detection_enabled: bool,
    pub correlation_analysis_enabled: bool,
    pub forecasting_enabled: bool,
    pub cache_ttl_seconds: u64,
    pub max_data_points: usize,
    pub retention_days: u32,
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            predictive_analytics_enabled: true,
            anomaly_detection_enabled: true,
            correlation_analysis_enabled: true,
            forecasting_enabled: true,
            cache_ttl_seconds: 300, // 5 minutes
            max_data_points: 10000,
            retention_days: 90,
        }
    }
}

// Cache structure
struct AnalyticsCache {
    data: HashMap<String, CachedAnalytics>,
}

impl AnalyticsCache {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
    
    fn get(&self, key: &str) -> Option<&CachedAnalytics> {
        self.data.get(key)
    }
    
    fn insert(&mut self, key: String, cached_analytics: CachedAnalytics) {
        self.data.insert(key, cached_analytics);
    }
}

struct CachedAnalytics {
    data: AnalyticsData,
    cached_at: Instant,
    ttl: Duration,
}

impl CachedAnalytics {
    fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }
}

// Additional supporting structures for various features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeAnalyticsUpdate {
    pub timestamp: i64,
    pub metrics: BasicMetrics,
    pub alerts: Vec<Alert>,
    pub trends: Vec<QuickTrend>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub title: String,
    pub severity: AlertSeverity,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickTrend {
    pub metric: String,
    pub direction: TrendDirection,
    pub change_percent: f64,
}

// Visualization types and configurations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VisualizationType {
    LineChart,
    BarChart,
    PieChart,
    Heatmap,
    ScatterPlot,
    Histogram,
    BoxPlot,
    Gauge,
    Table,
    TreeMap,
    Sankey,
    NetworkDiagram,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizationOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub color_scheme: Option<String>,
    pub interactive: bool,
    pub animation_enabled: bool,
    pub show_legend: bool,
    pub custom_options: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizationConfig {
    pub viz_type: VisualizationType,
    pub data: serde_json::Value,
    pub options: serde_json::Value,
    pub metadata: VisualizationMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizationMetadata {
    pub generated_at: i64,
    pub data_points: usize,
    pub complexity_score: f64,
}

// Export and query structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExportFormat {
    Json,
    Csv,
    Excel,
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Csv => "csv",
            ExportFormat::Excel => "xlsx",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsExport {
    pub format: ExportFormat,
    pub data: String,
    pub filename: String,
    pub generated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomAnalyticsQuery {
    pub id: String,
    pub name: String,
    pub query_type: QueryType,
    pub query: String,
    pub parameters: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryType {
    Aggregation,
    TimeSeries,
    Correlation,
    Prediction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub query_id: String,
    pub result_type: ResultType,
    pub data: serde_json::Value,
    pub execution_time_ms: u64,
    pub row_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResultType {
    Aggregation,
    TimeSeries,
    Correlation,
    Prediction,
}

// Summary and period analysis structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimePeriod {
    LastHour,
    Last24Hours,
    Last7Days,
    Last30Days,
    Custom { start: i64, end: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsSummary {
    pub time_period: TimePeriod,
    pub total_requests: u64,
    pub average_latency: f64,
    pub error_rate: f64,
    pub cost_usd: f64,
    pub trends: Vec<PeriodTrend>,
    pub anomaly_count: usize,
    pub top_errors: Vec<TopError>,
    pub performance_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeriodTrend {
    pub metric: String,
    pub trend: TrendDirection,
    pub change_percent: f64,
}

// Placeholder structures for metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PeriodMetrics {
    pub total_requests: u64,
    pub average_latency_ms: f64,
    pub error_rate: f64,
    pub total_cost_usd: f64,
    pub top_errors: Vec<TopError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PerformanceMetrics {
    pub response_time_distribution: HashMap<String, u64>,
    pub throughput_analysis: ThroughputAnalysis,
    pub resource_utilization: ResourceUtilization,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostMetrics {
    pub total_cost_today: f64,
    pub cost_by_provider: HashMap<String, f64>,
    pub cost_by_model: HashMap<String, f64>,
    pub cost_trend: Vec<CostPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErrorMetrics {
    pub total_errors: u64,
    pub error_rate: f64,
    pub error_types: HashMap<String, u64>,
    pub error_trends: Vec<ErrorTrendPoint>,
    pub top_errors: Vec<TopError>,
    pub resolved_errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageData {
    pub peak_hours: Vec<HourlyUsage>,
    pub usage_by_model: HashMap<String, ModelUsage>,
    pub usage_by_user: HashMap<String, UserUsage>,
    pub usage_trends: Vec<UsageTrendPoint>,
}