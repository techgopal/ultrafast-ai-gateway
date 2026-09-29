//! Shared state and the route table.

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use serde_json::json;
use tokio::sync::{watch, Mutex, Semaphore};
use tokio::task::JoinHandle;

use crate::api;
use crate::identity::limiter::LoginLimiter;
use crate::proxy;
use crate::secrets::Cipher;
use crate::snapshot::Snapshot;
use crate::store::Store;

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
        Ok(())
    }
}

/// Refreshes the snapshot every `refresh_interval`, so changes made by the
/// CLI reach a running gateway. A failure is logged and the next round runs
/// as usual. The task ends when `stop` becomes true or its sender is dropped.
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
        }
    })
}

pub fn router(state: Arc<AppState>) -> Router {
    // The chat handler enforces `max_body_bytes` itself, after authentication.
    Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/v1/chat/completions", post(proxy::chat_completions))
        .nest("/api", api::router())
        // The nested router does not see this path.
        .route("/api/", any(|| async { api::ApiError::not_found() }))
        .with_state(state)
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
