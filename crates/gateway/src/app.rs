//! Shared state and the route table.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
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
use crate::errors::error_response;
use crate::identity::limiter::LoginLimiter;
use crate::proxy;
use crate::secrets::Cipher;
use crate::snapshot::Snapshot;
use crate::store::Store;
use crate::web;

/// How many passwords may be hashed at the same time.
pub const MAX_CONCURRENT_HASHES: usize = 4;
/// How often a running gateway reads changes made outside it.
pub const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
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
    /// How many snapshots have been swapped in since the start.
    refreshes: AtomicU64,
    /// Held while a snapshot is loaded and swapped in, so an older one
    /// can never replace a newer one.
    refreshing: Mutex<()>,
}

impl AppState {
    /// A state with the default limits, an empty limiter and secure cookies.
    /// Loads the first snapshot.
    pub async fn new(store: Store, cipher: Cipher) -> anyhow::Result<Self> {
        let snapshot = Snapshot::load(&store, &cipher).await?;
        Ok(Self {
            snapshot: ArcSwap::from_pointee(snapshot),
            refresh_interval: DEFAULT_REFRESH_INTERVAL,
            trusted_proxies: Vec::new(),
            refreshes: AtomicU64::new(0),
            refreshing: Mutex::new(()),
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

    /// Rebuilds the snapshot from the database and swaps it in. Do not call
    /// it while a `Tx` is open.
    pub async fn refresh(&self) -> anyhow::Result<()> {
        let _guard = self.refreshing.lock().await;
        let snapshot = Snapshot::load(&self.store, &self.cipher).await?;
        self.snapshot.store(Arc::new(snapshot));
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
            match state.store.delete_expired_sessions().await {
                Ok(expired) => tracing::debug!(expired, "removed expired sessions"),
                Err(e) => tracing::warn!(error = %e, "could not remove expired sessions"),
            }
        }
    })
}

pub fn router(state: Arc<AppState>) -> Router {
    // The chat handler enforces `max_body_bytes` itself, after authentication.
    Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/v1/chat/completions", post(proxy::chat_completions))
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
