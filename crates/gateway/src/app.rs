//! Shared state and the route table.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::{ArcSwap, ArcSwapOption};
use axum::http::header::{CACHE_CONTROL, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::map_response;
use axum::response::Response;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use ipnet::IpNet;
use serde_json::json;
use tokio::sync::{watch, Mutex, Semaphore};
use tokio::task::JoinHandle;

use crate::api;
use crate::budgets::{Budgets, MemoryBudgets};
use crate::cache::{Flights, MemoryCache, ResponseCache};
use crate::errors::error_response;
use crate::identity::external::SignInProvider;
use crate::identity::limiter::LoginLimiter;
use crate::limits::{Limiter, MemoryLimiter};
use crate::metrics::{self, Metrics};
use crate::proxy;
use crate::routing::{HealthStore, InMemoryHealth};
use crate::secrets::{generate_setup_code, Cipher};
use crate::snapshot::Snapshot;
use crate::store::Store;
use crate::telemetry::{NoopSink, RequestSink};
use crate::web;

/// How many passwords may be hashed at the same time.
pub const MAX_CONCURRENT_HASHES: usize = 4;
/// How often a running gateway reads changes made outside it.
pub const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
/// The default of [`AppState::stream_keepalive`].
pub const DEFAULT_STREAM_KEEPALIVE: Duration = Duration::from_secs(10);
pub const DEFAULT_MAX_BODY_BYTES: usize = 10 * 1024 * 1024;
pub const DEFAULT_MAX_PROVIDER_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

pub struct AppState {
    pub store: Store,
    pub cipher: Cipher,
    pub http: reqwest::Client,
    pub max_body_bytes: usize,
    /// The largest non-streaming provider response that is read.
    pub max_provider_response_bytes: usize,
    /// Failed sign-in attempts, kept in memory.
    pub limiter: LoginLimiter,
    /// Whether the session cookie is marked `Secure`.
    pub cookie_secure: bool,
    /// Bounds how many passwords are hashed at once, so a flood of
    /// sign-ins cannot occupy every blocking thread.
    pub hashing: Arc<Semaphore>,
    /// The keys and providers `/v1` works from.
    pub snapshot: ArcSwap<Snapshot>,
    /// How long the background task waits between refreshes.
    pub refresh_interval: Duration,
    /// Networks whose peers may name the client in `CF-Connecting-IP` or
    /// `X-Forwarded-For`. Empty: those headers are never read.
    pub trusted_proxies: Vec<IpNet>,
    /// The address people reach the gateway at (`UF_PUBLIC_URL`). Needed
    /// to turn single sign-on on: the identity provider returns the
    /// browser to `<public_url>/api/auth/oidc/callback`.
    pub public_url: Option<reqwest::Url>,
    /// The provider of single sign-on, when it is on and set up. Rebuilt by
    /// [`AppState::reload_sign_in`] whenever its settings change.
    pub sign_in: ArcSwapOption<Arc<dyn SignInProvider>>,
    /// The settings `sign_in` was last built from, to see whether another
    /// process has changed them since.
    sign_in_seen: std::sync::Mutex<Option<crate::store::OidcSettings>>,
    /// Receives one record per authenticated `/v1` call.
    pub sink: Arc<dyn RequestSink>,
    /// The rate limits of `/v1`: requests, tokens and concurrency.
    pub rate: Arc<dyn Limiter>,
    /// The answers kept by routes with the cache on.
    pub cache: Arc<dyn ResponseCache>,
    /// Makes concurrent misses of one cache key wait for the first.
    pub flights: Flights,
    /// The spend counters of the budgets of `/v1`.
    pub budgets: Arc<dyn Budgets>,
    /// The in-flight limit of each external guardrail.
    pub hook_gates: Arc<crate::guardrails::external::HookGates>,
    /// How often a stream held for an external guardrail sends an SSE
    /// comment so a proxy in front does not cut it for being idle.
    pub stream_keepalive: Duration,
    /// The circuit breaker of every target that was called.
    pub health: Arc<dyn HealthStore>,
    /// The counters `/metrics` shows.
    pub metrics: Arc<Metrics>,
    /// Exports a trace of every call over OTLP/HTTP. `None`: off.
    pub otel: Option<crate::otel::Exporter>,
    /// Delivers alert events to their channels. `None`: no delivery.
    pub alerts: Option<crate::alerts::Deliverer>,
    /// Decides when an alert fires; fed by calls, budgets and breakers.
    /// `None`: no alert rule is evaluated.
    pub alert_engine: Option<crate::alerts::EngineHandle>,
    /// The bearer token of `/metrics`. None: the path does not exist.
    pub metrics_token: Option<String>,
    /// The one-time code that `POST /api/setup` needs, made when the
    /// gateway starts without users. `None` when a user existed then.
    pub setup_code: Option<String>,
    /// How many snapshots have been swapped in since the start.
    refreshes: AtomicU64,
    /// The fingerprint of the configuration the cache was filled under.
    cache_fingerprint: std::sync::Mutex<[u8; 32]>,
    /// Held while a snapshot is loaded and swapped in, so an older one
    /// can never replace a newer one.
    refreshing: Mutex<()>,
    /// Held by a budget flush, so two never overlap.
    pub flushing: Mutex<()>,
}

impl AppState {
    /// A state with the default limits, an empty limiter and secure cookies.
    /// Loads the first snapshot.
    pub async fn new(store: Store, cipher: Cipher) -> anyhow::Result<Self> {
        let snapshot = Snapshot::load(&store, &cipher).await?;
        let snapshot_fingerprint = snapshot.cache_fingerprint();
        let setup_code = (store.count_users().await? == 0).then(generate_setup_code);
        Ok(Self {
            setup_code,
            snapshot: ArcSwap::from_pointee(snapshot),
            refresh_interval: DEFAULT_REFRESH_INTERVAL,
            trusted_proxies: Vec::new(),
            public_url: None,
            sign_in: ArcSwapOption::empty(),
            sign_in_seen: std::sync::Mutex::new(None),
            sink: Arc::new(NoopSink),
            rate: Arc::new(MemoryLimiter::new()),
            cache: Arc::new(MemoryCache::new()),
            flights: Flights::new(),
            hook_gates: Arc::default(),
            stream_keepalive: DEFAULT_STREAM_KEEPALIVE,
            budgets: Arc::new(MemoryBudgets::new()),
            health: Arc::new(InMemoryHealth::new()),
            metrics: Arc::new(Metrics::new()),
            otel: None,
            alerts: None,
            alert_engine: None,
            metrics_token: None,
            refreshes: AtomicU64::new(0),
            cache_fingerprint: std::sync::Mutex::new(snapshot_fingerprint),
            refreshing: Mutex::new(()),
            flushing: Mutex::new(()),
            store,
            cipher,
            http: http_client(),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            max_provider_response_bytes: DEFAULT_MAX_PROVIDER_RESPONSE_BYTES,
            limiter: LoginLimiter::new(),
            cookie_secure: true,
            hashing: Arc::new(Semaphore::new(MAX_CONCURRENT_HASHES)),
        })
    }

    /// The address to register at the identity provider: the public URL
    /// followed by the callback path. `None` without a public URL.
    pub fn oidc_redirect_uri(&self) -> Option<String> {
        let base = self.public_url.as_ref()?.as_str().trim_end_matches('/');
        Some(format!("{base}/api/auth/oidc/callback"))
    }

    /// The client secret stored in `settings`, decrypted. `None` when none
    /// is stored, or when the stored value cannot be read (the master key
    /// changed).
    pub fn oidc_client_secret(&self, settings: &crate::store::OidcSettings) -> Option<String> {
        settings
            .client_secret_enc
            .as_deref()
            .and_then(|hex| hex::decode(hex).ok())
            .and_then(|bytes| self.cipher.decrypt(&bytes).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    /// Rebuilds the sign-in provider from the stored settings: none when
    /// single sign-on is off or not complete (no public URL, issuer, client
    /// id or readable secret). Call it after the settings change, and
    /// never while a `Tx` is open.
    pub async fn reload_sign_in(&self) -> anyhow::Result<()> {
        let settings = self.store.oidc_settings().await?;
        self.apply_sign_in(settings);
        Ok(())
    }

    /// [`AppState::reload_sign_in`] only when the stored settings are not
    /// the ones the provider was built from: what the refresher calls, so a
    /// change another process saved reaches this one within a refresh
    /// interval. One cheap read when nothing changed. Returns whether the
    /// provider was rebuilt.
    pub async fn reload_sign_in_if_changed(&self) -> anyhow::Result<bool> {
        let settings = self.store.oidc_settings().await?;
        let unchanged = self
            .sign_in_seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            == Some(&settings);
        if unchanged {
            return Ok(false);
        }
        self.apply_sign_in(settings);
        Ok(true)
    }

    fn apply_sign_in(&self, settings: crate::store::OidcSettings) {
        let secret = self.oidc_client_secret(&settings);
        if settings.client_secret_enc.is_some() && secret.is_none() {
            tracing::warn!(
                "the stored single sign-on client secret cannot be decrypted \
                 (the master key changed?): single sign-on stays off until a new secret is saved"
            );
        }
        let provider = match (
            settings.enabled,
            self.oidc_redirect_uri(),
            secret,
            settings.issuer.is_empty() || settings.client_id.is_empty(),
        ) {
            (true, Some(redirect_uri), Some(secret), false) => {
                crate::identity::external::build_oidc(
                    &settings,
                    &secret,
                    &redirect_uri,
                    &self.http,
                    &self.cipher,
                )
            }
            _ => None,
        };
        self.sign_in.store(provider.map(Arc::new));
        *self
            .sign_in_seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(settings);
    }

    /// Rebuilds the snapshot from the database and swaps it in. Do not call
    /// it while a `Tx` is open.
    pub async fn refresh(&self) -> anyhow::Result<()> {
        let _guard = self.refreshing.lock().await;
        let snapshot =
            Snapshot::load_after(&self.store, &self.cipher, Some(&self.snapshot.load())).await?;
        // What left the catalog is no longer worth a breaker.
        self.health
            .retain(&|provider, model| snapshot.model(provider, model).is_some());
        // A deleted budget (or one whose team or user is gone) loses its counter.
        let budget_ids: Vec<i64> = snapshot.all_budgets().iter().map(|b| b.id).collect();
        self.budgets.retain(&budget_ids);
        let fingerprint = snapshot.cache_fingerprint();
        self.snapshot.store(Arc::new(snapshot));
        // Answers kept under the old configuration are not given under a new
        // one: a team, user or key id may be another one now, a route or a
        // provider may have changed. A refresh that finds nothing changed
        // keeps them.
        {
            let mut seen = self
                .cache_fingerprint
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *seen != fingerprint {
                *seen = fingerprint;
                self.cache.clear();
            }
        }
        self.refreshes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// How many snapshots have been swapped in since the start. A test
    /// watches it to see that a change was published.
    pub fn refresh_count(&self) -> u64 {
        self.refreshes.load(Ordering::Relaxed)
    }
}

/// Refreshes the snapshot every `refresh_interval`, so changes made by the
/// CLI reach a running gateway, and deletes the sessions that have expired,
/// so the table does not grow for the life of the process. A failure of
/// either is logged; the other still runs, and so does the next round. The
/// task ends when `stop` becomes true or its sender is dropped.
pub fn spawn_refresher(state: Arc<AppState>, mut stop: watch::Receiver<bool>) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(state.refresh_interval) => {}
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        return;
                    }
                    continue;
                }
            }
            if let Err(e) = state.refresh().await {
                tracing::error!(error = %e, "snapshot refresh failed");
            }
            // Single sign-on settings saved on another process.
            match state.reload_sign_in_if_changed().await {
                Ok(true) => tracing::info!("single sign-on settings changed: provider rebuilt"),
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e, "could not check the single sign-on settings"),
            }
            // The sign-in limiter forgets what left its window here, not
            // on every attempt.
            state.limiter.prune(std::time::Instant::now());
            match state.store.delete_expired_sessions().await {
                Ok(expired) => tracing::debug!(expired, "removed expired sessions"),
                Err(e) => tracing::warn!(error = %e, "could not remove expired sessions"),
            }
        }
    })
}

pub fn router(state: Arc<AppState>) -> Router {
    // The chat handler enforces `max_body_bytes` itself, after authentication.
    let mut app = Router::new().route("/health", get(|| async { Json(json!({ "status": "ok" })) }));
    // Without a token the path is not routed at all: it is answered like any
    // other path the console does not know.
    if state.metrics_token.is_some() {
        app = app.route("/metrics", get(metrics::serve));
    }
    app.route("/v1/chat/completions", post(proxy::chat_completions))
        .route("/v1/messages", post(proxy::messages))
        .route("/v1/responses", post(proxy::responses))
        .route("/v1/embeddings", post(proxy::embeddings))
        .route("/v1/models", get(proxy::list_models))
        // Every other path under `/v1` is answered here, so the console's
        // pages never stand in for a model API that does not exist.
        .route("/v1", any(v1_not_found))
        .route("/v1/", any(v1_not_found))
        .route("/v1/{*rest}", any(v1_not_found))
        .nest("/api", api::router().layer(map_response(api_headers)))
        // The nested router does not see this path.
        .route(
            "/api/",
            any(|| async { api::ApiError::not_found() }).layer(map_response(api_headers)),
        )
        // The console: `/`, its files, and every path not claimed above.
        .merge(web::router())
        .with_state(state)
}

async fn v1_not_found() -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "invalid_request_error",
        "Unknown path.",
    )
}

/// What every answer of `/api` carries: it is never stored by a browser or
/// a proxy, and never read as anything but its declared type.
async fn api_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(300);

/// The client for provider calls. It has timeouts so a hung provider cannot
/// hold a request forever, and never follows redirects, so a provider cannot
/// send a request carrying a credential to another host.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TOTAL_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("the HTTP client configuration is valid")
}

/// Resolves when the process is asked to stop: ctrl-c, or SIGTERM on Unix.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => tokio::select! {
                _ = ctrl_c => {}
                _ = term.recv() => {}
            },
            Err(e) => {
                tracing::warn!(error = %e, "could not listen for SIGTERM");
                ctrl_c.await;
            }
        }
    }
    #[cfg(not(unix))]
    ctrl_c.await;
}
