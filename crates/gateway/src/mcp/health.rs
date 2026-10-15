//! The health of MCP servers: every `UF_MCP_HEALTH_INTERVAL` seconds (60) one
//! `tools/list` per enabled server, with a 5 second limit.
//!
//! A probe is the gateway asking, not an agent: it skips the key pipeline
//! entirely, so it takes no rate-limit permit, costs no budget and never
//! writes a row to the request logs. What it finds goes to the console
//! (`GET /api/mcp/servers`), to the gauge `uf_mcp_server_up{server}` and to
//! the alert rules of kind `mcp_down`. The state is per process.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::upstream::{self, Options, Target, UpstreamError};
use crate::app::AppState;
use crate::snapshot::SnapMcpServer;

pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(60);
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// An answer slower than this is `degraded`.
pub const SLOW: Duration = Duration::from_secs(2);
/// This many failed probes in a row make a server `down`; one makes it `degraded`.
pub const DOWN_AFTER: u32 = 2;
/// The most servers probed at once.
const PARALLEL: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    /// Answers slowly, or failed once.
    Degraded,
    Down,
    /// A local server whose program dies again and again.
    CrashLooping,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Degraded => "degraded",
            Self::Down => "down",
            Self::CrashLooping => "crashlooping",
        }
    }

    /// Whether calls can be expected to work.
    pub fn up(self) -> bool {
        matches!(self, Self::Ok | Self::Degraded)
    }
}

#[derive(Debug, Clone)]
pub struct Probe {
    pub status: Status,
    /// How long the answer took; none when there was none.
    pub latency_ms: Option<u64>,
    /// RFC 3339.
    pub checked_at: String,
    failures: u32,
}

/// The last probe of each server, by id.
#[derive(Default)]
pub struct Board {
    map: Mutex<HashMap<i64, Probe>>,
}

impl Board {
    pub fn get(&self, id: i64) -> Option<Probe> {
        self.lock().get(&id).cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<i64, Probe>> {
        self.map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn put(&self, id: i64, probe: Probe) {
        self.lock().insert(id, probe);
    }

    /// Forgets the servers that are not in `ids` any more.
    fn retain(&self, ids: &[i64]) {
        self.lock().retain(|id, _| ids.contains(id));
    }
}

/// `UF_MCP_HEALTH_INTERVAL` in seconds (at least 5); an unusable value reads as the default.
pub fn interval_from_env() -> Duration {
    std::env::var("UF_MCP_HEALTH_INTERVAL")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n >= 5)
        .map_or(DEFAULT_INTERVAL, Duration::from_secs)
}

/// What one probe found, before it is judged.
struct Found {
    ok: bool,
    latency: Duration,
}

async fn ask(state: &AppState, server: &SnapMcpServer) -> Found {
    let opts = Options {
        timeout: PROBE_TIMEOUT,
        max_body_bytes: upstream::max_discovery_bytes(),
    };
    let started = Instant::now();
    let answered: Result<_, UpstreamError> = if server.local.is_some() {
        state
            .local
            .call(server.id, "tools/list", &json!({}), opts)
            .await
    } else {
        let target = Target {
            url: &server.url,
            auth_kind: &server.auth_kind,
            auth_header: server.auth_header.as_deref(),
            secret: server.secret.as_deref(),
        };
        upstream::call_method(&state.http, &target, "tools/list", &json!({}), None, opts).await
    };
    Found {
        ok: answered.is_ok(),
        latency: started.elapsed(),
    }
}

/// One pass over every enabled server. Never fails: what goes wrong is the
/// finding.
pub async fn probe_once(state: &AppState) {
    let snapshot = state.snapshot.load_full();
    let servers: Vec<&SnapMcpServer> = snapshot
        .mcp_servers()
        .filter(|s| s.enabled && s.usable)
        // A local server while local servers are off has nothing to probe.
        .filter(|s| s.local.is_none() || state.local.enabled())
        .collect();
    let ids: Vec<i64> = snapshot.mcp_servers().map(|s| s.id).collect();
    state.mcp_health.retain(&ids);
    let at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default();

    let gate = Arc::new(tokio::sync::Semaphore::new(PARALLEL));
    let work = servers.iter().map(|server| {
        let gate = gate.clone();
        async move {
            let _permit = gate.acquire_owned().await.ok();
            // A program that is crash-looping is not asked.
            let crashing = server.local.is_some()
                && state
                    .local
                    .health(server.id)
                    .is_some_and(|h| h.phase == super::local::Phase::CrashLooping);
            let found = if crashing {
                None
            } else {
                Some(ask(state, server).await)
            };
            (*server, found)
        }
    });
    let results = futures::future::join_all(work).await;

    let mut gauge = std::collections::BTreeMap::new();
    for (server, found) in results {
        let before = state.mcp_health.get(server.id);
        let failures = before.as_ref().map_or(0, |p| p.failures);
        let probe = match found {
            None => Probe {
                status: Status::CrashLooping,
                latency_ms: None,
                checked_at: at.clone(),
                failures: failures.saturating_add(1),
            },
            Some(f) if f.ok => Probe {
                status: if f.latency > SLOW {
                    Status::Degraded
                } else {
                    Status::Ok
                },
                latency_ms: Some(u64::try_from(f.latency.as_millis()).unwrap_or(u64::MAX)),
                checked_at: at.clone(),
                failures: 0,
            },
            Some(_) => {
                let failures = failures.saturating_add(1);
                Probe {
                    status: if failures >= DOWN_AFTER {
                        Status::Down
                    } else {
                        Status::Degraded
                    },
                    latency_ms: None,
                    checked_at: at.clone(),
                    failures,
                }
            }
        };
        gauge.insert(server.name.clone(), probe.status.up());
        if let Some(engine) = &state.alert_engine {
            engine.mcp_status(&server.name, !probe.status.up());
        }
        state.mcp_health.put(server.id, probe);
    }
    state.metrics.set_mcp_up(gauge);
}

/// The probing task: waits an interval, probes, again; ends when `stop` is
/// set or its sender dropped.
pub fn spawn(
    state: Arc<AppState>,
    interval: Duration,
    mut stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // The first look comes soon after start, not a whole interval later.
        let mut wait = Duration::from_secs(3).min(interval);
        loop {
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        return;
                    }
                    continue;
                }
            }
            wait = interval;
            probe_once(&state).await;
        }
    })
}
