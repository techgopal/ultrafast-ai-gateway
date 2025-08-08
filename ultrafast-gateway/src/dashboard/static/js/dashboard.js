// Ultrafast Gateway Dashboard JavaScript
// Modern dashboard with real-time updates and proper error handling

class ModernDashboard {
    constructor() {
        this.config = {
            refreshInterval: 30000, // 30 seconds
            theme: 'auto',
            apiEndpoint: '/metrics',
            wsEndpoint: '/ws/dashboard',
            retryAttempts: 3,
            retryDelay: 1000
        };
        this.charts = {};
        this.dataHistory = {
            latency: [],
            requests: []
        };
        this.filters = {
            timeRange: '24h',
            provider: 'all',
            model: 'all',
            showSuccess: true,
            showErrors: true
        };
        this.isLoading = false;
        this.errorCount = 0;
        this.wsConnection = null;
        this.init();
    }

    init() {
        this.setupThemeToggle();
        this.setupCharts();
        this.setupChartToggles();
        this.setupFilters();
        this.setupErrorHandling();
        this.setupWebSocket();
        this.updateMetrics();
        this.startAutoRefresh();
        this.showToast('Dashboard loaded successfully', 'success');
    }

    setupThemeToggle() {
        const toggle = document.getElementById('theme-toggle');
        if (toggle) {
            toggle.addEventListener('click', () => {
                document.documentElement.classList.toggle('dark');
                this.updateThemeIcon();
                this.updateChartColors();
                this.showToast('Theme updated', 'success');
            });
        }
    }

    updateThemeIcon() {
        const toggle = document.getElementById('theme-toggle');
        if (toggle) {
            const isDark = document.documentElement.classList.contains('dark');
            toggle.innerHTML = isDark ? '<i class="fas fa-sun text-gray-300"></i>' : '<i class="fas fa-moon text-gray-600"></i>';
        }
    }

    updateChartColors() {
        const isDark = document.documentElement.classList.contains('dark');
        const textColor = isDark ? '#9ca3af' : '#6b7280';
        const gridColor = isDark ? 'rgba(156, 163, 175, 0.1)' : 'rgba(156, 163, 175, 0.1)';
        
        if (this.charts.performance) {
            this.charts.performance.options.scales.x.grid.color = gridColor;
            this.charts.performance.options.scales.y.grid.color = gridColor;
            this.charts.performance.options.scales.x.ticks.color = textColor;
            this.charts.performance.options.scales.y.ticks.color = textColor;
            this.charts.performance.options.plugins.legend.labels.color = textColor;
            this.charts.performance.update();
        }
    }

    setupCharts() {
        const ctx = document.getElementById('performanceChart');
        if (!ctx) return;

        // Check if Chart.js is loaded
        if (typeof Chart === 'undefined') {
            console.error('Chart.js not loaded. Charts will not be available.');
            this.showToast('Chart.js failed to load. Charts will not be available.', 'error');
            return;
        }

        // Configure Chart.js defaults
        Chart.defaults.font.family = 'Inter, system-ui, sans-serif';
        Chart.defaults.color = document.documentElement.classList.contains('dark') ? '#9ca3af' : '#6b7280';
        
        this.charts.performance = new Chart(ctx, {
            type: 'line',
            data: {
                labels: [],
                datasets: [{
                    label: 'Latency (ms)',
                    data: [],
                    borderColor: '#3b82f6',
                    backgroundColor: 'rgba(59, 130, 246, 0.1)',
                    tension: 0.4,
                    fill: true,
                    pointRadius: 4,
                    pointHoverRadius: 6,
                    borderWidth: 2
                }, {
                    label: 'Requests/min',
                    data: [],
                    borderColor: '#22c55e',
                    backgroundColor: 'rgba(34, 197, 94, 0.1)',
                    tension: 0.4,
                    fill: true,
                    pointRadius: 4,
                    pointHoverRadius: 6,
                    borderWidth: 2
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                interaction: {
                    intersect: false,
                    mode: 'index'
                },
                plugins: {
                    legend: {
                        position: 'top',
                        labels: {
                            usePointStyle: true,
                            padding: 20,
                            font: {
                                size: 12,
                                weight: '500'
                            }
                        }
                    },
                    tooltip: {
                        backgroundColor: 'rgba(0, 0, 0, 0.8)',
                        titleColor: '#ffffff',
                        bodyColor: '#ffffff',
                        borderColor: '#3b82f6',
                        borderWidth: 1,
                        cornerRadius: 8,
                        displayColors: true
                    }
                },
                scales: {
                    x: {
                        grid: {
                            color: 'rgba(156, 163, 175, 0.1)',
                            drawBorder: false
                        },
                        ticks: {
                            maxTicksLimit: 8,
                            font: {
                                size: 11
                            }
                        }
                    },
                    y: {
                        beginAtZero: true,
                        grid: {
                            color: 'rgba(156, 163, 175, 0.1)',
                            drawBorder: false
                        },
                        ticks: {
                            font: {
                                size: 11
                            }
                        }
                    }
                }
            }
        });
    }

    setupChartToggles() {
        const latencyBtn = document.getElementById('latency-btn');
        const requestsBtn = document.getElementById('requests-btn');
        
        if (latencyBtn && requestsBtn) {
            latencyBtn.addEventListener('click', () => {
                this.toggleChartDataset('latency');
                this.updateButtonStates(latencyBtn, requestsBtn);
            });
            
            requestsBtn.addEventListener('click', () => {
                this.toggleChartDataset('requests');
                this.updateButtonStates(requestsBtn, latencyBtn);
            });
        }
    }

    updateButtonStates(activeBtn, inactiveBtn) {
        if (activeBtn && inactiveBtn) {
            activeBtn.classList.add('bg-primary-500', 'text-white');
            activeBtn.classList.remove('bg-gray-100', 'text-gray-700', 'dark:bg-gray-700', 'dark:text-gray-300');
            inactiveBtn.classList.remove('bg-primary-500', 'text-white');
            inactiveBtn.classList.add('bg-gray-100', 'text-gray-700', 'dark:bg-gray-700', 'dark:text-gray-300');
        }
    }

    toggleChartDataset(type) {
        const chart = this.charts.performance;
        if (!chart) return;

        if (type === 'latency') {
            chart.data.datasets[0].hidden = false;
            chart.data.datasets[1].hidden = true;
        } else {
            chart.data.datasets[0].hidden = true;
            chart.data.datasets[1].hidden = false;
        }
        chart.update();
    }
    
    setupFilters() {
        // Time range filter
        const timeRangeFilter = document.getElementById('time-range-filter');
        if (timeRangeFilter) {
            timeRangeFilter.addEventListener('change', (e) => {
                this.filters.timeRange = e.target.value;
                this.applyFilters();
            });
        }
        
        // Provider filter
        const providerFilter = document.getElementById('provider-filter');
        if (providerFilter) {
            providerFilter.addEventListener('change', (e) => {
                this.filters.provider = e.target.value;
                this.applyFilters();
            });
        }
        
        // Model filter
        const modelFilter = document.getElementById('model-filter');
        if (modelFilter) {
            modelFilter.addEventListener('change', (e) => {
                this.filters.model = e.target.value;
                this.applyFilters();
            });
        }
        
        // Status filter buttons
        const successFilter = document.getElementById('filter-success');
        const errorFilter = document.getElementById('filter-errors');
        
        if (successFilter) {
            successFilter.addEventListener('click', () => {
                this.filters.showSuccess = !this.filters.showSuccess;
                this.updateFilterButtonState(successFilter, this.filters.showSuccess);
                this.applyFilters();
            });
        }
        
        if (errorFilter) {
            errorFilter.addEventListener('click', () => {
                this.filters.showErrors = !this.filters.showErrors;
                this.updateFilterButtonState(errorFilter, this.filters.showErrors);
                this.applyFilters();
            });
        }
        
        // Reset filters button
        const resetFilters = document.getElementById('reset-filters');
        if (resetFilters) {
            resetFilters.addEventListener('click', () => {
                this.resetFilters();
            });
        }
    }
    
    updateFilterButtonState(button, isActive) {
        if (!button) return;

        if (isActive) {
            button.classList.add('bg-success-100', 'dark:bg-success-900/30', 'text-success-700', 'dark:text-success-300');
            button.classList.remove('bg-gray-100', 'dark:bg-gray-700', 'text-gray-600', 'dark:text-gray-300');
            button.setAttribute('data-active', 'true');
        } else {
            button.classList.remove('bg-success-100', 'dark:bg-success-900/30', 'text-success-700', 'dark:text-success-300');
            button.classList.add('bg-gray-100', 'dark:bg-gray-700', 'text-gray-600', 'dark:text-gray-300');
            button.setAttribute('data-active', 'false');
        }
    }
    
    applyFilters() {
        this.showToast(`Filters applied: ${this.filters.timeRange} range`, 'info');
        this.updateMetrics();
    }
    
    resetFilters() {
        this.filters = {
            timeRange: '24h',
            provider: 'all',
            model: 'all',
            showSuccess: true,
            showErrors: true
        };
        
        // Reset UI elements
        const timeRangeFilter = document.getElementById('time-range-filter');
        const providerFilter = document.getElementById('provider-filter');
        const modelFilter = document.getElementById('model-filter');
        const successFilter = document.getElementById('filter-success');
        const errorFilter = document.getElementById('filter-errors');

        if (timeRangeFilter) timeRangeFilter.value = '24h';
        if (providerFilter) providerFilter.value = 'all';
        if (modelFilter) modelFilter.value = 'all';
        
        this.updateFilterButtonState(successFilter, true);
        this.updateFilterButtonState(errorFilter, true);
        
        this.showToast('Filters reset', 'success');
        this.updateMetrics();
    }
    
    buildFilteredApiEndpoint() {
        const params = new URLSearchParams();
        params.append('time_range', this.filters.timeRange);
        
        if (this.filters.provider !== 'all') {
            params.append('provider', this.filters.provider);
        }
        
        if (this.filters.model !== 'all') {
            params.append('model', this.filters.model);
        }
        
        return `${this.config.apiEndpoint}?${params.toString()}`;
    }

    setupErrorHandling() {
        window.addEventListener('error', (e) => {
            console.error('Dashboard error:', e.error);
            this.showToast('An error occurred', 'error');
        });

        window.addEventListener('unhandledrejection', (e) => {
            console.error('Unhandled promise rejection:', e.reason);
            this.showToast('Network error occurred', 'error');
        });
    }

    setupWebSocket() {
        try {
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const wsUrl = `${protocol}//${window.location.host}${this.config.wsEndpoint}`;
            
            this.wsConnection = new WebSocket(wsUrl);
            
            this.wsConnection.onopen = () => {
                console.log('WebSocket connected');
                this.showToast('Real-time updates connected', 'success');
            };
            
            this.wsConnection.onmessage = (event) => {
                try {
                    const data = JSON.parse(event.data);
                    this.handleWebSocketMessage(data);
                } catch (error) {
                    console.error('Failed to parse WebSocket message:', error);
                }
            };
            
            this.wsConnection.onerror = (error) => {
                console.error('WebSocket error:', error);
                this.showToast('Real-time connection error', 'error');
            };
            
            this.wsConnection.onclose = () => {
                console.log('WebSocket disconnected');
                this.showToast('Real-time connection lost', 'warning');
                // Attempt to reconnect after 5 seconds
                setTimeout(() => this.setupWebSocket(), 5000);
            };
        } catch (error) {
            console.error('Failed to setup WebSocket:', error);
        }
    }

    handleWebSocketMessage(data) {
        switch (data.type) {
            case 'metrics_update':
                this.updateMetricsFromWebSocket(data.data);
                break;
            case 'provider_status':
                this.updateProviderStatus(data.data);
                break;
            case 'alert':
                this.showAlert(data.data);
                break;
            default:
                console.log('Unknown WebSocket message type:', data.type);
        }
    }

    updateMetricsFromWebSocket(metrics) {
        this.updateQuickStats(metrics);
        this.updatePerformanceChart(metrics);
        this.updateLastUpdate();
    }

    async updateMetrics() {
        if (this.isLoading) return;
        
        this.isLoading = true;
        this.showLoadingStates();
        
        try {
            const endpoint = this.buildFilteredApiEndpoint();
            const response = await this.fetchWithRetry(endpoint);
            
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }
            
            const data = await response.json();
            
            this.updateQuickStats(data);
            this.updatePerformanceChart(data);
            this.updateCostSummary(data);
            this.updateProviderHealth(data);
            this.updateErrorAnalytics(data);
            this.updateLastUpdate();
            
            this.errorCount = 0;
            this.hideLoadingStates();
            
        } catch (error) {
            console.error('Failed to fetch metrics:', error);
            this.errorCount++;
            
            if (this.errorCount >= this.config.retryAttempts) {
                this.showErrorState();
                this.showToast('Failed to load metrics after multiple attempts', 'error');
            } else {
                this.showToast('Retrying...', 'warning');
                setTimeout(() => this.updateMetrics(), this.config.retryDelay);
            }
        } finally {
            this.isLoading = false;
        }
    }

    async fetchWithRetry(url, attempts = this.config.retryAttempts) {
        for (let i = 0; i < attempts; i++) {
            try {
                const response = await fetch(url, {
                    method: 'GET',
                    headers: {
                        'Content-Type': 'application/json',
                        'Accept': 'application/json'
                    }
                });
                
                if (!response.ok) {
                    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
                }
                
                return response;
            } catch (error) {
                if (i === attempts - 1) throw error;
                await new Promise(resolve => setTimeout(resolve, this.config.retryDelay));
            }
        }
    }

    showLoadingStates() {
        const elements = ['requests-per-minute', 'avg-latency', 'error-rate', 'active-connections'];
        elements.forEach(id => {
            const element = document.getElementById(id);
            if (element) {
                element.innerHTML = '<div class="loading-spinner"></div>';
            }
        });
    }

    hideLoadingStates() {
        // Loading states will be replaced with actual data
    }

    showErrorState() {
        const elements = ['requests-per-minute', 'avg-latency', 'error-rate', 'active-connections'];
        elements.forEach(id => {
            const element = document.getElementById(id);
            if (element) {
                element.innerHTML = '<span class="text-red-500">Error</span>';
            }
        });
    }

    updateQuickStats(data) {
        // Only show real data, not fake data
        const hasRealData = data.total_requests > 0;
        
        const updates = {
            'requests-per-minute': hasRealData ? (data.requests_per_minute || 0).toFixed(1) : '0.0',
            'avg-latency': hasRealData ? (data.average_latency_ms || 0).toFixed(1) : '0.0',
            'error-rate': hasRealData ? ((data.error_rate || 0) * 100).toFixed(2) + '%' : '0.00%',
            'active-connections': hasRealData ? (data.active_connections || 0) : 0
        };
        
        Object.entries(updates).forEach(([id, value]) => {
            const element = document.getElementById(id);
            if (element) {
                element.textContent = value;
                element.classList.add('animate-fade-in');
            }
        });
    }

    updatePerformanceChart(data) {
        const chart = this.charts.performance;
        if (!chart) return;

        // Only show real data, not fake data
        const hasRealData = data.total_requests > 0;
        
        if (!hasRealData) {
            // Clear history when there's no real data
            this.dataHistory.latency = [];
            this.dataHistory.requests = [];
            chart.data.labels = [];
            chart.data.datasets[0].data = [];
            chart.data.datasets[1].data = [];
            chart.update('none');
            return;
        }

        const now = new Date().toLocaleTimeString();
        
        // Add to history only if there's real data
        this.dataHistory.latency.push(data.average_latency_ms || 0);
        this.dataHistory.requests.push(data.requests_per_minute || 0);
        
        // Keep only last 30 data points
        if (this.dataHistory.latency.length > 30) {
            this.dataHistory.latency.shift();
            this.dataHistory.requests.shift();
        }
        
        chart.data.labels = Array.from({length: this.dataHistory.latency.length}, (_, i) => 
            new Date(Date.now() - (this.dataHistory.latency.length - 1 - i) * 30000).toLocaleTimeString()
        );
        chart.data.datasets[0].data = this.dataHistory.latency;
        chart.data.datasets[1].data = this.dataHistory.requests;
        
        chart.update('none');
    }

    updateCostSummary(data) {
        const totalCost = data.total_cost_usd || 0;
        const totalCostElement = document.getElementById('total-cost');
        if (totalCostElement) {
            totalCostElement.textContent = `$${totalCost.toFixed(4)}`;
        }
        
        const providerCosts = document.getElementById('provider-costs');
        if (providerCosts) {
            providerCosts.innerHTML = '';
            
            if (data.provider_stats && Object.keys(data.provider_stats).length > 0) {
                Object.entries(data.provider_stats).forEach(([provider, stats]) => {
                    const cost = stats.total_cost_usd || 0;
                    // Show all providers, even those with zero cost
                    const div = document.createElement('div');
                    div.className = 'flex justify-between items-center p-4 bg-gray-50 dark:bg-gray-700 rounded-lg border border-gray-200 dark:border-gray-600';
                    div.innerHTML = `
                        <div class="flex items-center space-x-3">
                            <div class="w-8 h-8 bg-primary-100 dark:bg-primary-900/30 rounded-lg flex items-center justify-center">
                                <i class="fas fa-server text-primary-600 dark:text-primary-400 text-sm"></i>
                            </div>
                            <span class="text-sm font-medium text-gray-900 dark:text-white">${provider}</span>
                        </div>
                        <span class="text-sm font-bold text-success-600 dark:text-success-400">$${cost.toFixed(4)}</span>
                    `;
                    providerCosts.appendChild(div);
                });
            } else {
                // Show placeholder when no provider data
                const placeholderDiv = document.createElement('div');
                placeholderDiv.className = 'text-center p-6 text-gray-500 dark:text-gray-400';
                placeholderDiv.innerHTML = `
                    <div class="w-12 h-12 bg-gray-100 dark:bg-gray-700 rounded-lg flex items-center justify-center mx-auto mb-3">
                        <i class="fas fa-chart-line text-gray-400 text-xl"></i>
                    </div>
                    <div class="text-sm">No cost data available</div>
                `;
                providerCosts.appendChild(placeholderDiv);
            }
        }
    }

    updateProviderHealth(data) {
        const container = document.getElementById('provider-health');
        if (!container) return;

        container.innerHTML = '';
        
        // Check if there are real requests and providers
        const hasRealData = data.total_requests > 0;
        const hasProviders = data.provider_stats && Object.keys(data.provider_stats).length > 0;
        
        if (hasProviders && hasRealData) {
            Object.entries(data.provider_stats).forEach(([provider, stats]) => {
                const errorRate = stats.error_rate || 0;
                const latency = stats.average_latency_ms || 0;
                const uptime = stats.uptime_percentage || 0;
                
                const slaMetrics = this.calculateSLA(errorRate, latency, uptime);
                const statusInfo = this.getProviderStatusInfo(slaMetrics);
                
                const div = document.createElement('div');
                div.className = `provider-card border-l-4 border-${statusInfo.color}-500 bg-white dark:bg-gray-800 rounded-lg p-6 shadow-sm border border-gray-200 dark:border-gray-700 mb-4`;
                div.innerHTML = `
                    <div class="space-y-4">
                        <div class="flex items-center justify-between">
                            <div class="flex items-center space-x-3">
                                <div class="flex-shrink-0">
                                    <div class="w-10 h-10 bg-${statusInfo.color}-100 dark:bg-${statusInfo.color}-900/30 rounded-lg flex items-center justify-center">
                                        <i class="fas fa-${statusInfo.icon} text-${statusInfo.color}-600 dark:text-${statusInfo.color}-400 text-lg"></i>
                                    </div>
                                </div>
                                <div>
                                    <div class="font-semibold text-lg text-gray-900 dark:text-white">
                                        ${provider.charAt(0).toUpperCase() + provider.slice(1)}
                                    </div>
                                    <div class="text-sm text-${statusInfo.color}-600 dark:text-${statusInfo.color}-400">
                                        ${statusInfo.status} • SLA Score: ${slaMetrics.score.toFixed(1)}%
                                    </div>
                                </div>
                            </div>
                            <div class="flex items-center space-x-2">
                                <div class="w-3 h-3 bg-${statusInfo.color}-500 rounded-full animate-pulse"></div>
                                <span class="text-xs font-medium text-gray-500 dark:text-gray-400">
                                    Last: ${stats.last_request ? new Date(stats.last_request).toLocaleTimeString() : 'Never'}
                                </span>
                            </div>
                        </div>
                        
                        <div class="grid grid-cols-2 lg:grid-cols-4 gap-3">
                            <div class="text-center p-3 bg-gray-50 dark:bg-gray-700 rounded-lg border border-gray-200 dark:border-gray-600">
                                <div class="text-lg font-bold text-gray-900 dark:text-white">
                                    ${stats.requests || 0}
                                </div>
                                <div class="text-xs text-gray-600 dark:text-gray-400">Requests</div>
                            </div>
                            <div class="text-center p-3 bg-gray-50 dark:bg-gray-700 rounded-lg border border-gray-200 dark:border-gray-600">
                                <div class="text-lg font-bold text-gray-900 dark:text-white">
                                    ${latency.toFixed(0)}ms
                                </div>
                                <div class="text-xs text-gray-600 dark:text-gray-400">Avg Latency</div>
                            </div>
                            <div class="text-center p-3 bg-gray-50 dark:bg-gray-700 rounded-lg border border-gray-200 dark:border-gray-600">
                                <div class="text-lg font-bold text-${errorRate > 0.05 ? 'red' : 'green'}-600">
                                    ${(errorRate * 100).toFixed(2)}%
                                </div>
                                <div class="text-xs text-gray-600 dark:text-gray-400">Error Rate</div>
                            </div>
                            <div class="text-center p-3 bg-gray-50 dark:bg-gray-700 rounded-lg border border-gray-200 dark:border-gray-600">
                                <div class="text-lg font-bold text-${uptime > 95 ? 'green' : uptime > 90 ? 'yellow' : 'red'}-600">
                                    ${uptime.toFixed(1)}%
                                </div>
                                <div class="text-xs text-gray-600 dark:text-gray-400">Uptime</div>
                            </div>
                        </div>
                    </div>
                `;
                container.appendChild(div);
            });
        } else if (!hasProviders) {
            // Show placeholder when no providers configured
            const noProvidersDiv = document.createElement('div');
            noProvidersDiv.className = 'text-center p-8 text-gray-500 dark:text-gray-400';
            noProvidersDiv.innerHTML = `
                <div class="w-16 h-16 bg-gray-100 dark:bg-gray-700 rounded-full flex items-center justify-center mx-auto mb-4">
                    <i class="fas fa-plug-circle-plus text-gray-400 text-2xl"></i>
                </div>
                <div class="text-sm font-medium">No providers configured</div>
                <div class="text-xs mt-2">Configure providers in your gateway configuration</div>
                <div class="text-xs mt-1 text-gray-400">Check your config.toml file</div>
            `;
            container.appendChild(noProvidersDiv);
        } else {
            // Show placeholder when providers configured but no requests made
            const noRequestsDiv = document.createElement('div');
            noRequestsDiv.className = 'text-center p-8 text-gray-500 dark:text-gray-400';
            noRequestsDiv.innerHTML = `
                <div class="w-16 h-16 bg-gray-100 dark:bg-gray-700 rounded-full flex items-center justify-center mx-auto mb-4">
                    <i class="fas fa-clock text-gray-400 text-2xl"></i>
                </div>
                <div class="text-sm font-medium">Providers configured</div>
                <div class="text-xs mt-2">No requests made yet</div>
                <div class="text-xs mt-1 text-gray-400">Make a request to see provider health</div>
            `;
            container.appendChild(noRequestsDiv);
        }
    }
    
    calculateSLA(errorRate, latency, uptime) {
        const reliabilityScore = Math.max(0, 100 - (errorRate * 100 * 10));
        const performanceScore = Math.max(0, 100 - Math.max(0, (latency - 1000) / 50));
        const uptimeScore = uptime;
        
        const score = (reliabilityScore * 0.4) + (performanceScore * 0.3) + (uptimeScore * 0.3);
        
        return {
            score,
            reliability: reliabilityScore,
            performance: performanceScore,
            uptime: uptimeScore
        };
    }
    
    getProviderStatusInfo(slaMetrics) {
        const score = slaMetrics.score;
        
        if (score >= 95) {
            return { 
                status: 'Excellent', 
                color: 'success', 
                icon: 'shield-check'
            };
        } else if (score >= 90) {
            return { 
                status: 'Good', 
                color: 'primary', 
                icon: 'shield'
            };
        } else if (score >= 75) {
            return { 
                status: 'Warning', 
                color: 'warning', 
                icon: 'shield-exclamation'
            };
        } else {
            return { 
                status: 'Critical', 
                color: 'danger', 
                icon: 'shield-xmark'
            };
        }
    }

    updateErrorAnalytics(data) {
        const errorTypes = document.getElementById('error-types');
        const recentErrors = document.getElementById('recent-errors');
        
        // Only show errors if there are real requests
        const hasRealData = data.total_requests > 0;
        
        if (errorTypes) {
            errorTypes.innerHTML = '';
            
            if (hasRealData && data.error_stats && data.error_stats.error_types && Object.keys(data.error_stats.error_types).length > 0) {
                const errorEntries = Object.entries(data.error_stats.error_types)
                    .sort((a, b) => b[1] - a[1]);
                
                errorEntries.forEach(([errorType, count]) => {
                    const div = document.createElement('div');
                    div.className = 'flex items-center justify-between p-4 border rounded-lg bg-red-50 border-red-200 dark:bg-red-900/20 dark:border-red-800';
                    div.innerHTML = `
                        <div class="flex items-center space-x-3">
                            <div class="flex-shrink-0">
                                <div class="w-8 h-8 bg-red-100 dark:bg-red-900/30 rounded-lg flex items-center justify-center">
                                    <i class="fas fa-exclamation-circle text-red-600 dark:text-red-400 text-sm"></i>
                                </div>
                            </div>
                            <div>
                                <div class="text-sm font-medium text-gray-900 dark:text-white">
                                    ${this.formatErrorType(errorType)}
                                </div>
                                <div class="text-xs text-gray-600 dark:text-gray-400">
                                    Severity: High
                                </div>
                            </div>
                        </div>
                        <div class="flex items-center space-x-2">
                            <span class="text-lg font-bold text-red-600 dark:text-red-400">
                                ${count}
                            </span>
                            <div class="text-xs text-gray-500">
                                ${((count / data.total_requests) * 100).toFixed(1)}%
                            </div>
                        </div>
                    `;
                    errorTypes.appendChild(div);
                });
            } else {
                // Show no errors message
                const noErrorsDiv = document.createElement('div');
                noErrorsDiv.className = 'text-center p-6 text-gray-500 dark:text-gray-400';
                noErrorsDiv.innerHTML = `
                    <div class="w-12 h-12 bg-green-100 dark:bg-green-900/30 rounded-lg flex items-center justify-center mx-auto mb-3">
                        <i class="fas fa-check-circle text-green-600 dark:text-green-400 text-xl"></i>
                    </div>
                    <div class="text-sm">${hasRealData ? 'No errors detected' : 'No requests made yet'}</div>
                `;
                errorTypes.appendChild(noErrorsDiv);
            }
        }
        
        if (recentErrors) {
            recentErrors.innerHTML = '';
            
            const noErrorsDiv = document.createElement('div');
            noErrorsDiv.className = 'text-center p-6 text-gray-500 dark:text-gray-400';
            noErrorsDiv.innerHTML = `
                <div class="w-12 h-12 bg-green-100 dark:bg-green-900/30 rounded-lg flex items-center justify-center mx-auto mb-3">
                    <i class="fas fa-check-circle text-green-600 dark:text-green-400 text-xl"></i>
                </div>
                <div class="text-sm">${hasRealData ? 'No recent errors' : 'No requests made yet'}</div>
            `;
            recentErrors.appendChild(noErrorsDiv);
        }
    }
    
    formatErrorType(errorType) {
        return errorType
            .split('_')
            .map(word => word.charAt(0).toUpperCase() + word.slice(1))
            .join(' ');
    }

    updateLastUpdate() {
        const now = new Date();
        const timeString = now.toLocaleTimeString();
        const lastUpdateElement = document.getElementById('last-update');
        if (lastUpdateElement) {
            lastUpdateElement.textContent = `Updated ${timeString}`;
        }
    }

    showToast(message, type = 'info') {
        const container = document.getElementById('toast-container');
        if (!container) return;

        const toast = document.createElement('div');
        toast.className = `toast ${type}`;
        toast.innerHTML = `
            <div class="flex items-center space-x-2">
                <i class="fas fa-${type === 'success' ? 'check-circle' : type === 'error' ? 'exclamation-circle' : 'info-circle'}"></i>
                <span>${message}</span>
            </div>
        `;
        
        container.appendChild(toast);
        
        setTimeout(() => {
            toast.remove();
        }, 3000);
    }

    startAutoRefresh() {
        setInterval(() => {
            this.updateMetrics();
        }, this.config.refreshInterval);
    }
}

// Initialize dashboard when page loads
document.addEventListener('DOMContentLoaded', () => {
    new ModernDashboard();
}); 