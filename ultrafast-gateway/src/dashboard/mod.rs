//! # Dashboard Module
//!
//! This module provides a comprehensive real-time monitoring dashboard for the Ultrafast Gateway.
//! It includes WebSocket-based real-time updates, customizable themes, and extensive
//! monitoring capabilities.
//!
//! ## Overview
//!
//! The dashboard system provides:
//! - **Real-time Monitoring**: Live metrics and performance data
//! - **WebSocket Integration**: Real-time updates without polling
//! - **Customizable Themes**: Light, dark, and auto themes
//! - **Comprehensive Metrics**: Provider health, costs, errors, and performance
//! - **Interactive Components**: Dynamic charts and visualizations
//! - **Responsive Design**: Mobile-friendly dashboard interface
//!
//! ## Dashboard Features
//!
//! ### Real-time Metrics
//!
//! Live monitoring of gateway performance:
//! - **Request Throughput**: Requests per second/minute
//! - **Latency Tracking**: P50, P90, P95, P99 latencies
//! - **Error Rates**: Real-time error tracking
//! - **Cost Monitoring**: Live cost tracking and alerts
//!
//! ### Provider Health Monitoring
//!
//! Comprehensive provider status tracking:
//! - **Health Status**: Provider availability and performance
//! - **Circuit Breaker Status**: Circuit breaker state monitoring
//! - **Response Times**: Per-provider latency tracking
//! - **Error Rates**: Provider-specific error monitoring
//!
//! ### Cost Tracking Dashboard
//!
//! Detailed cost analysis and monitoring:
//! - **Real-time Costs**: Live cost tracking across providers
//! - **Cost Breakdown**: Per-provider and per-model costs
//! - **Budget Alerts**: Cost threshold notifications
//! - **Historical Analysis**: Cost trends and patterns
//!
//! ### Error Analytics
//!
//! Comprehensive error tracking and analysis:
//! - **Error Categorization**: Detailed error type tracking
//! - **Error Trends**: Historical error rate analysis
//! - **Provider Error Analysis**: Per-provider error tracking
//! - **Error Resolution**: Error handling and recovery metrics
//!
//! ### Cache Performance
//!
//! Cache effectiveness monitoring:
//! - **Hit Rates**: Cache hit/miss ratio tracking
//! - **Cache Latency**: Cache access time monitoring
//! - **Cache Size**: Memory usage and efficiency
//! - **Cache Eviction**: Cache cleanup and management
//!
//! ## WebSocket Integration
//!
//! The dashboard uses WebSocket connections for real-time updates:
//!
//! - **Live Updates**: Real-time metric updates without polling
//! - **Connection Management**: Automatic reconnection handling
//! - **Message Types**: Different update types for various metrics
//! - **Subscription System**: Selective metric subscription
//!
//! ## Configuration
//!
//! The dashboard can be configured via the `DashboardConfig`:
//!
//! ```rust
//! use ultrafast_gateway::dashboard::{DashboardConfig, DashboardTheme};
//!
//! let config = DashboardConfig {
//!     title: "My Gateway Dashboard".to_string(),
//!     refresh_interval_ms: 5000,
//!     theme: DashboardTheme::Dark,
//!     features: DashboardFeatures {
//!         real_time_metrics: true,
//!         provider_health: true,
//!         cost_tracking: true,
//!         error_analytics: true,
//!         cache_performance: true,
//!         user_management: false,
//!         system_health: true,
//!     },
//! };
//! ```
//!
//! ## Usage
//!
//! ```rust
//! use ultrafast_gateway::dashboard::render_dashboard;
//!
//! // Render the dashboard with custom configuration
//! let html = render_dashboard(config).await?;
//! ```
//!
//! ## Security Considerations
//!
//! The dashboard includes security features:
//! - **Authentication**: Dashboard access control
//! - **Rate Limiting**: WebSocket connection limits
//! - **Input Validation**: Secure data handling
//! - **CORS Configuration**: Cross-origin request handling

use crate::gateway_error::GatewayError;
use axum::response::Html;
use serde::{Deserialize, Serialize};

pub mod websocket;

/// Configuration for the dashboard system.
///
/// Defines the appearance, behavior, and features of the
/// monitoring dashboard. This configuration controls the
/// dashboard's theme, update frequency, and enabled features.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    /// Dashboard title displayed in the browser
    pub title: String,
    /// Refresh interval for real-time updates in milliseconds
    pub refresh_interval_ms: u64,
    /// Visual theme for the dashboard
    pub theme: DashboardTheme,
    /// Enabled dashboard features and components
    pub features: DashboardFeatures,
}

/// Visual theme options for the dashboard.
///
/// Controls the appearance and styling of the dashboard interface.
/// The Auto theme automatically adapts to the user's system preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DashboardTheme {
    /// Light theme with light backgrounds and dark text
    Light,
    /// Dark theme with dark backgrounds and light text
    Dark,
    /// Automatically adapts to system theme preferences
    Auto,
}

/// Dashboard features and component configuration.
///
/// Controls which dashboard components and features are enabled.
/// Disabling features can improve performance and reduce complexity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardFeatures {
    /// Enable real-time metrics and live updates
    pub real_time_metrics: bool,
    /// Enable provider health monitoring
    pub provider_health: bool,
    /// Enable cost tracking and analysis
    pub cost_tracking: bool,
    /// Enable error analytics and tracking
    pub error_analytics: bool,
    /// Enable cache performance monitoring
    pub cache_performance: bool,
    /// Enable user management features
    pub user_management: bool,
    /// Enable system health monitoring
    pub system_health: bool,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            title: "Ultrafast Gateway Dashboard".to_string(),
            refresh_interval_ms: 30000,
            theme: DashboardTheme::Auto,
            features: DashboardFeatures {
                real_time_metrics: true,
                provider_health: true,
                cost_tracking: true,
                error_analytics: true,
                cache_performance: true,
                user_management: false,
                system_health: true,
            },
        }
    }
}

/// Renders the dashboard HTML with the specified configuration.
///
/// This struct handles the generation of the dashboard HTML
/// including theme application, feature configuration, and
/// WebSocket integration setup.
pub struct DashboardRenderer {
    /// Dashboard configuration and settings
    config: DashboardConfig,
}

impl DashboardRenderer {
    pub fn new(config: DashboardConfig) -> Self {
        Self { config }
    }

    pub fn render(&self) -> Result<Html<String>, GatewayError> {
        let html = self.generate_html();
        Ok(Html(html))
    }

    fn generate_html(&self) -> String {
        let theme_class = match self.config.theme {
            DashboardTheme::Light => "light",
            DashboardTheme::Dark => "dark",
            DashboardTheme::Auto => "auto",
        };

        let websocket_enabled = self.config.features.real_time_metrics;
        let _websocket_url = if websocket_enabled {
            "ws://127.0.0.1:3000/ws/dashboard"
        } else {
            ""
        };

        format!(
            r#"<!DOCTYPE html>
<html lang="en" class="{}">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{}</title>
    
    <!-- Tailwind CSS -->
    <script src="https://cdn.tailwindcss.com"></script>
    <script>
        tailwind.config = {{
            darkMode: 'class',
            theme: {{
                extend: {{
                    colors: {{
                        primary: {{
                            50: '#eff6ff',
                            100: '#dbeafe',
                            200: '#bfdbfe',
                            300: '#93c5fd',
                            400: '#60a5fa',
                            500: '#3b82f6',
                            600: '#2563eb',
                            700: '#1d4ed8',
                            800: '#1e40af',
                            900: '#1e3a8a',
                        }},
                        success: {{
                            50: '#f0fdf4',
                            100: '#dcfce7',
                            200: '#bbf7d0',
                            300: '#86efac',
                            400: '#4ade80',
                            500: '#22c55e',
                            600: '#16a34a',
                            700: '#15803d',
                            800: '#166534',
                            900: '#14532d',
                        }},
                        warning: {{
                            50: '#fffbeb',
                            100: '#fef3c7',
                            200: '#fde68a',
                            300: '#fcd34d',
                            400: '#fbbf24',
                            500: '#f59e0b',
                            600: '#d97706',
                            700: '#b45309',
                            800: '#92400e',
                            900: '#78350f',
                        }},
                        danger: {{
                            50: '#fef2f2',
                            100: '#fee2e2',
                            200: '#fecaca',
                            300: '#fca5a5',
                            400: '#f87171',
                            500: '#ef4444',
                            600: '#dc2626',
                            700: '#b91c1c',
                            800: '#991b1b',
                            900: '#7f1d1d',
                        }}
                    }},
                    animation: {{
                        'pulse-slow': 'pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite',
                        'bounce-slow': 'bounce 2s infinite',
                        'fade-in': 'fadeIn 0.5s ease-in-out',
                        'slide-up': 'slideUp 0.3s ease-out',
                        'slide-down': 'slideDown 0.3s ease-out',
                        'scale-in': 'scaleIn 0.2s ease-out',
                    }},
                    keyframes: {{
                        fadeIn: {{
                            '0%': {{ opacity: '0' }},
                            '100%': {{ opacity: '1' }},
                        }},
                        slideUp: {{
                            '0%': {{ transform: 'translateY(10px)', opacity: '0' }},
                            '100%': {{ transform: 'translateY(0)', opacity: '1' }},
                        }},
                        slideDown: {{
                            '0%': {{ transform: 'translateY(-10px)', opacity: '0' }},
                            '100%': {{ transform: 'translateY(0)', opacity: '1' }},
                        }},
                        scaleIn: {{
                            '0%': {{ transform: 'scale(0.95)', opacity: '0' }},
                            '100%': {{ transform: 'scale(1)', opacity: '1' }},
                        }}
                    }}
                }}
            }}
        }}
    </script>
    
    <!-- Chart.js -->
    <script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.0/dist/chart.umd.js"></script>
    
    <!-- Font Awesome -->
    <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.4.0/css/all.min.css">
    
    <!-- Custom CSS -->
    <link rel="stylesheet" href="/dashboard.css">
    
    <style>
        .glass-effect {{
            background: rgba(255, 255, 255, 0.1);
            backdrop-filter: blur(10px);
            border: 1px solid rgba(255, 255, 255, 0.2);
        }}
        .dark .glass-effect {{
            background: rgba(0, 0, 0, 0.2);
            border: 1px solid rgba(255, 255, 255, 0.1);
        }}
        .metric-card {{
            transition: all 0.3s ease;
        }}
        .metric-card:hover {{
            transform: translateY(-2px);
            box-shadow: 0 10px 25px rgba(0, 0, 0, 0.1);
        }}
        .dark .metric-card:hover {{
            box-shadow: 0 10px 25px rgba(0, 0, 0, 0.3);
        }}
        .loading-spinner {{
            border: 2px solid #f3f4f6;
            border-top: 2px solid #3b82f6;
            border-radius: 50%;
            width: 20px;
            height: 20px;
            animation: spin 1s linear infinite;
        }}
        .dark .loading-spinner {{
            border: 2px solid #374151;
            border-top: 2px solid #3b82f6;
        }}
        @keyframes spin {{
            0% {{ transform: rotate(0deg); }}
            100% {{ transform: rotate(360deg); }}
        }}
        .toast {{
            position: fixed;
            top: 1rem;
            right: 1rem;
            z-index: 50;
            padding: 0.75rem 1rem;
            border-radius: 0.5rem;
            color: white;
            font-weight: 500;
            animation: slideIn 0.3s ease-out;
        }}
        .toast.success {{
            background: #22c55e;
        }}
        .toast.error {{
            background: #ef4444;
        }}
        .toast.warning {{
            background: #f59e0b;
        }}
        .toast.info {{
            background: #3b82f6;
        }}
        @keyframes slideIn {{
            from {{ transform: translateX(100%); opacity: 0; }}
            to {{ transform: translateX(0); opacity: 1; }}
        }}
    </style>
</head>
<body class="bg-gray-50 dark:bg-gray-900 text-gray-900 dark:text-gray-100 min-h-screen">
    <!-- Header -->
    <header class="sticky top-0 z-40 bg-white/80 dark:bg-gray-800/80 backdrop-blur-lg border-b border-gray-200 dark:border-gray-700">
        <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
            <div class="flex justify-between items-center py-4">
                <div class="flex items-center space-x-4">
                    <div class="flex-shrink-0">
                        <div class="w-10 h-10 bg-gradient-to-br from-primary-500 to-primary-600 rounded-lg flex items-center justify-center">
                            <i class="fas fa-bolt text-white text-xl"></i>
                        </div>
                    </div>
                    <div>
                        <h1 class="text-2xl font-bold bg-gradient-to-r from-primary-600 to-primary-400 bg-clip-text text-transparent">
                            Ultrafast Gateway
                        </h1>
                        <p class="text-sm text-gray-600 dark:text-gray-400">AI Gateway Dashboard</p>
                    </div>
                </div>
                <div class="flex items-center space-x-6">
                    <div class="flex items-center space-x-3">
                        <div class="flex items-center space-x-2">
                            <div class="w-2 h-2 bg-success-500 rounded-full animate-pulse"></div>
                            <span class="text-sm font-medium text-success-600 dark:text-success-400">Online</span>
                        </div>
                        <span class="text-xs text-gray-500 dark:text-gray-400">•</span>
                        <span class="text-sm text-gray-600 dark:text-gray-400" id="last-update">Just now</span>
                    </div>
                    <button id="theme-toggle" class="p-2 rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors">
                        <i class="fas fa-moon dark:hidden text-gray-600"></i>
                        <i class="fas fa-sun hidden dark:inline text-gray-300"></i>
                    </button>
                </div>
            </div>
        </div>
    </header>

    <!-- Main Content -->
    <main class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        <!-- Filters Section -->
        <div class="mb-8">
            <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
                <div class="flex flex-wrap items-center justify-between gap-4">
                    <div class="flex items-center space-x-4">
                        <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Filters</h3>
                        <button id="reset-filters" class="px-3 py-1 text-xs bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-300 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors">
                            <i class="fas fa-undo mr-1"></i>
                            Reset
                        </button>
                    </div>
                    <div class="flex flex-wrap items-center gap-4">
                        <!-- Time Range Filter -->
                        <div class="flex items-center space-x-2">
                            <label class="text-sm font-medium text-gray-700 dark:text-gray-300">Time Range:</label>
                            <select id="time-range-filter" class="px-3 py-1 text-sm bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-gray-100 rounded-lg border border-gray-300 dark:border-gray-600 focus:ring-2 focus:ring-primary-500 focus:border-transparent">
                                <option value="5m">Last 5 minutes</option>
                                <option value="15m">Last 15 minutes</option>
                                <option value="1h">Last hour</option>
                                <option value="6h">Last 6 hours</option>
                                <option value="24h" selected>Last 24 hours</option>
                                <option value="7d">Last 7 days</option>
                                <option value="30d">Last 30 days</option>
                            </select>
                        </div>
                        
                        <!-- Provider Filter -->
                        <div class="flex items-center space-x-2">
                            <label class="text-sm font-medium text-gray-700 dark:text-gray-300">Provider:</label>
                            <select id="provider-filter" class="px-3 py-1 text-sm bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-gray-100 rounded-lg border border-gray-300 dark:border-gray-600 focus:ring-2 focus:ring-primary-500 focus:border-transparent">
                                <option value="all">All Providers</option>
                                <option value="openai">OpenAI</option>
                                <option value="anthropic">Anthropic</option>
                                <option value="google">Google</option>
                                <option value="azure">Azure</option>
                            </select>
                        </div>
                        
                        <!-- Model Filter -->
                        <div class="flex items-center space-x-2">
                            <label class="text-sm font-medium text-gray-700 dark:text-gray-300">Model:</label>
                            <select id="model-filter" class="px-3 py-1 text-sm bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-gray-100 rounded-lg border border-gray-300 dark:border-gray-600 focus:ring-2 focus:ring-primary-500 focus:border-transparent">
                                <option value="all">All Models</option>
                                <option value="gpt-4">GPT-4</option>
                                <option value="gpt-3.5-turbo">GPT-3.5 Turbo</option>
                                <option value="claude-3">Claude 3</option>
                                <option value="gemini-pro">Gemini Pro</option>
                            </select>
                        </div>
                        
                        <!-- Status Filter -->
                        <div class="flex items-center space-x-2">
                            <label class="text-sm font-medium text-gray-700 dark:text-gray-300">Status:</label>
                            <div class="flex space-x-2">
                                <button id="filter-success" class="px-3 py-1 text-xs bg-success-100 dark:bg-success-900/30 text-success-700 dark:text-success-300 rounded-lg hover:bg-success-200 dark:hover:bg-success-900/50 transition-colors" data-active="true">
                                    <i class="fas fa-check-circle mr-1"></i>
                                    Success
                                </button>
                                <button id="filter-errors" class="px-3 py-1 text-xs bg-danger-100 dark:bg-danger-900/30 text-danger-700 dark:text-danger-300 rounded-lg hover:bg-danger-200 dark:hover:bg-danger-900/50 transition-colors" data-active="true">
                                    <i class="fas fa-exclamation-circle mr-1"></i>
                                    Errors
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
        
        <!-- Quick Stats Grid -->
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6 mb-8">
            <!-- Requests per Minute -->
            <div class="metric-card bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
                <div class="flex items-center justify-between">
                    <div>
                        <div class="text-3xl font-bold text-gray-900 dark:text-white mb-1" id="requests-per-minute">
                            <div class="loading-spinner"></div>
                        </div>
                        <div class="text-sm text-gray-600 dark:text-gray-400">Requests/min</div>
                    </div>
                    <div class="flex-shrink-0">
                        <div class="w-12 h-12 bg-primary-100 dark:bg-primary-900/30 rounded-lg flex items-center justify-center">
                            <i class="fas fa-chart-line text-primary-600 dark:text-primary-400 text-xl"></i>
                        </div>
                    </div>
                </div>
            </div>

            <!-- Average Latency -->
            <div class="metric-card bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
                <div class="flex items-center justify-between">
                    <div>
                        <div class="text-3xl font-bold text-gray-900 dark:text-white mb-1" id="avg-latency">
                            <div class="loading-spinner"></div>
                        </div>
                        <div class="text-sm text-gray-600 dark:text-gray-400">Avg Latency (ms)</div>
                    </div>
                    <div class="flex-shrink-0">
                        <div class="w-12 h-12 bg-success-100 dark:bg-success-900/30 rounded-lg flex items-center justify-center">
                            <i class="fas fa-tachometer-alt text-success-600 dark:text-success-400 text-xl"></i>
                        </div>
                    </div>
                </div>
            </div>

            <!-- Error Rate -->
            <div class="metric-card bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
                <div class="flex items-center justify-between">
                    <div>
                        <div class="text-3xl font-bold text-gray-900 dark:text-white mb-1" id="error-rate">
                            <div class="loading-spinner"></div>
                        </div>
                        <div class="text-sm text-gray-600 dark:text-gray-400">Error Rate</div>
                    </div>
                    <div class="flex-shrink-0">
                        <div class="w-12 h-12 bg-danger-100 dark:bg-danger-900/30 rounded-lg flex items-center justify-center">
                            <i class="fas fa-exclamation-triangle text-danger-600 dark:text-danger-400 text-xl"></i>
                        </div>
                    </div>
                </div>
            </div>

            <!-- Active Connections -->
            <div class="metric-card bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
                <div class="flex items-center justify-between">
                    <div>
                        <div class="text-3xl font-bold text-gray-900 dark:text-white mb-1" id="active-connections">
                            <div class="loading-spinner"></div>
                        </div>
                        <div class="text-sm text-gray-600 dark:text-gray-400">Active Connections</div>
                    </div>
                    <div class="flex-shrink-0">
                        <div class="w-12 h-12 bg-warning-100 dark:bg-warning-900/30 rounded-lg flex items-center justify-center">
                            <i class="fas fa-users text-warning-600 dark:text-warning-400 text-xl"></i>
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- Detailed Metrics -->
        <div class="grid grid-cols-1 xl:grid-cols-3 gap-8">
            <!-- Performance Chart -->
            <div class="xl:col-span-2">
                <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
                    <div class="flex items-center justify-between mb-6">
                        <h2 class="text-xl font-semibold text-gray-900 dark:text-white flex items-center">
                            <div class="w-8 h-8 bg-primary-100 dark:bg-primary-900/30 rounded-lg flex items-center justify-center mr-3">
                                <i class="fas fa-chart-area text-primary-600 dark:text-primary-400"></i>
                            </div>
                            Performance Metrics
                        </h2>
                        <div class="flex space-x-2">
                            <button class="px-4 py-2 text-sm font-medium rounded-lg transition-colors chart-toggle active" id="latency-btn">
                                Latency
                            </button>
                            <button class="px-4 py-2 text-sm font-medium rounded-lg transition-colors chart-toggle" id="requests-btn">
                                Requests
                            </button>
                        </div>
                    </div>
                    <div class="h-80">
                        <canvas id="performanceChart"></canvas>
                    </div>
                </div>
            </div>

            <!-- Cost Analysis -->
            <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
                <h2 class="text-xl font-semibold text-gray-900 dark:text-white mb-6 flex items-center">
                    <div class="w-8 h-8 bg-success-100 dark:bg-success-900/30 rounded-lg flex items-center justify-center mr-3">
                        <i class="fas fa-dollar-sign text-success-600 dark:text-success-400"></i>
                    </div>
                    Cost Analysis
                </h2>
                <div class="space-y-6">
                    <!-- Total Cost -->
                    <div class="text-center p-6 bg-gradient-to-br from-success-50 to-success-100 dark:from-success-900/20 dark:to-success-900/10 rounded-xl">
                        <div class="text-4xl font-bold text-success-600 dark:text-success-400 mb-2" id="total-cost">$0.00</div>
                        <div class="text-sm text-success-700 dark:text-success-300">Total Cost (USD)</div>
                    </div>
                    
                    <!-- Provider Costs -->
                    <div>
                        <h3 class="text-lg font-medium text-gray-900 dark:text-white mb-4">Cost by Provider</h3>
                        <div id="provider-costs" class="space-y-3">
                            <!-- Provider costs will be populated here -->
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- Provider Health & Error Analytics -->
        <div class="grid grid-cols-1 lg:grid-cols-2 gap-8 mt-8">
            <!-- Provider Health -->
            <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
                <h2 class="text-xl font-semibold text-gray-900 dark:text-white mb-6 flex items-center">
                    <div class="w-8 h-8 bg-danger-100 dark:bg-danger-900/30 rounded-lg flex items-center justify-center mr-3">
                        <i class="fas fa-heartbeat text-danger-600 dark:text-danger-400"></i>
                    </div>
                    Provider Health
                </h2>
                <div id="provider-health" class="space-y-4">
                    <!-- Provider health cards will be populated here -->
                </div>
            </div>

            <!-- Error Analytics -->
            <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
                <h2 class="text-xl font-semibold text-gray-900 dark:text-white mb-6 flex items-center">
                    <div class="w-8 h-8 bg-warning-100 dark:bg-warning-900/30 rounded-lg flex items-center justify-center mr-3">
                        <i class="fas fa-bug text-warning-600 dark:text-warning-400"></i>
                    </div>
                    Error Analytics
                </h2>
                <div class="space-y-6">
                    <div>
                        <h3 class="text-lg font-medium text-gray-900 dark:text-white mb-3">Error Types</h3>
                        <div id="error-types" class="space-y-3">
                            <!-- Error types will be populated here -->
                        </div>
                    </div>
                    <div>
                        <h3 class="text-lg font-medium text-gray-900 dark:text-white mb-3">Recent Errors</h3>
                        <div id="recent-errors" class="space-y-3 max-h-48 overflow-y-auto">
                            <!-- Recent errors will be populated here -->
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </main>

    <!-- Toast Container -->
    <div id="toast-container" class="fixed top-4 right-4 z-50 space-y-2"></div>

    <!-- External JavaScript -->
    <script src="/dashboard.js"></script>
</body>
</html>"#,
            theme_class, self.config.title
        )
    }
}

pub async fn render_dashboard(config: DashboardConfig) -> Result<Html<String>, GatewayError> {
    let renderer = DashboardRenderer::new(config);
    renderer.render()
}

// Default dashboard handler for backward compatibility
pub async fn dashboard() -> Result<Html<String>, GatewayError> {
    render_dashboard(DashboardConfig::default()).await
}
