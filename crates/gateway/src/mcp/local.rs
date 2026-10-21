//! Local MCP servers: programs the gateway runs and talks to over stdio.
//!
//! They are off unless the operator sets `UF_MCP_LOCAL=on`, and refused when
//! the gateway runs as root. A program is started from the registry row
//! exactly as written (`command` and `args`, no shell), with an environment
//! of the names the admin listed plus `PATH`, `HOME` and `LANG`, in a
//! working directory of its own under the data directory, in its own process
//! group (so stopping it stops what it started). It is restarted with a
//! growing delay when it dies. What it writes to stderr is kept (the last
//! 64 KiB) and never blocks it. It is supervised, not sandboxed: a local
//! server is a trusted program.
//!
//! The wire is newline-delimited JSON-RPC, as the MCP stdio transport has
//! it. Many requests may be in flight at once; replies are matched by id.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{oneshot, watch, Mutex as AsyncMutex};
use tokio::task::JoinHandle;

use super::upstream::{reply_result, Discovered, Options, UpstreamError};

/// How much of a program's stderr is kept.
pub const STDERR_BYTES: usize = 64 * 1024;
/// Failures in a row (runs shorter than `stable_after`) after which a server
/// is said to be crash-looping.
pub const CRASH_LOOP_AFTER: u32 = 3;

/// What a local server is started with. The environment values are secrets.
#[derive(Clone, PartialEq, Eq)]
pub struct Spec {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl fmt::Debug for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Spec")
            .field("command", &self.command)
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// A server that should be running.
#[derive(Debug, Clone)]
pub struct Wanted {
    pub id: i64,
    pub name: String,
    pub spec: Arc<Spec>,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// The first wait before a restart; it doubles up to `restart_max`.
    pub restart_min: Duration,
    pub restart_max: Duration,
    /// A run at least this long counts as healthy and resets the wait.
    pub stable_after: Duration,
    /// The longest line read from a program.
    pub max_line: usize,
    /// How long the `initialize` exchange may take.
    pub handshake: Duration,
    /// How long a program gets to stop after SIGTERM.
    pub grace: Duration,
    /// Pretend to run as this user (a test of the refusal as root).
    pub euid: Option<u32>,
    /// `RLIMIT_AS` of a program, in MiB. Off by default: runtimes such as
    /// V8 reserve far more address space than they use.
    pub memory_mib: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            restart_min: Duration::from_secs(1),
            restart_max: Duration::from_secs(60),
            stable_after: Duration::from_secs(60),
            max_line: super::upstream::DEFAULT_MAX_DISCOVERY_BYTES * 2,
            handshake: Duration::from_secs(10),
            grace: Duration::from_secs(2),
            euid: None,
            memory_mib: std::env::var("UF_MCP_LOCAL_MEMORY_MIB")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|n| *n > 0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Running,
    /// Died; waiting to be started again.
    Backoff,
    /// Died again and again without ever running for long.
    CrashLooping,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health {
    pub phase: Phase,
    /// How many times it was started again.
    pub restarts: u32,
}

/// What the last 64 KiB of stderr looks like, kept without ever blocking
/// the writer.
struct Ring {
    buf: VecDeque<u8>,
    cap: usize,
}

impl Ring {
    fn push(&mut self, chunk: &[u8]) {
        let chunk = &chunk[chunk.len().saturating_sub(self.cap)..];
        let over = (self.buf.len() + chunk.len()).saturating_sub(self.cap);
        self.buf.drain(..over);
        self.buf.extend(chunk);
    }
}

type Reply = Result<(Value, usize), UpstreamError>;

/// One live connection to a running program.
struct Live {
    stdin: AsyncMutex<ChildStdin>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Reply>>>,
    next: AtomicU64,
}

impl Live {
    fn fail_all(&self, error: UpstreamError) {
        let drained: Vec<_> = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain()
            .collect();
        for (_, tx) in drained {
            let _ = tx.send(Err(error));
        }
    }

    async fn write(&self, message: &Value) -> Result<(), UpstreamError> {
        let mut line = message.to_string();
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        let written = async {
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await
        };
        match tokio::time::timeout(Duration::from_secs(5), written).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(UpstreamError::Connect),
            Err(_) => Err(UpstreamError::Timeout),
        }
    }

    async fn request(
        &self,
        method: &str,
        params: &Value,
        opts: Options,
    ) -> Result<Value, UpstreamError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, tx);
        let forget = || {
            self.pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id);
        };
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(e) = self.write(&message).await {
            forget();
            return Err(e);
        }
        match tokio::time::timeout(opts.timeout, rx).await {
            Ok(Ok(Ok((message, size)))) => {
                if size > opts.max_body_bytes {
                    return Err(UpstreamError::TooLarge);
                }
                reply_result(message)
            }
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_)) => Err(UpstreamError::Connect),
            Err(_) => {
                forget();
                Err(UpstreamError::Timeout)
            }
        }
    }
}

/// What the supervisor shares with a server's task.
struct Shared {
    health: Mutex<Health>,
    stderr: Mutex<Ring>,
    live: Mutex<Option<Arc<Live>>>,
}

impl Shared {
    fn set_phase(&self, phase: Phase) {
        self.health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .phase = phase;
    }
}

struct Running {
    spec: Arc<Spec>,
    shared: Arc<Shared>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

/// Runs the local servers of the registry.
pub struct Supervisor {
    cfg: Config,
    enabled: bool,
    root: PathBuf,
    servers: Mutex<HashMap<i64, Running>>,
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
fn effective_uid() -> u32 {
    1
}

impl Supervisor {
    /// A supervisor that starts nothing: local servers are off.
    pub fn disabled() -> Self {
        Self {
            cfg: Config::default(),
            enabled: false,
            root: PathBuf::new(),
            servers: Mutex::default(),
        }
    }

    /// A supervisor whose programs live under `root`. It starts nothing on
    /// a platform without process groups, or as root.
    pub fn new(root: PathBuf, cfg: Config) -> Self {
        let euid = cfg.euid.unwrap_or_else(effective_uid);
        let enabled = cfg!(unix) && euid != 0;
        if cfg!(unix) && euid == 0 {
            tracing::error!("local MCP servers are refused: the gateway runs as root");
        }
        Self {
            cfg,
            enabled,
            root,
            servers: Mutex::default(),
        }
    }

    /// Whether local servers may be added and run.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<i64, Running>> {
        self.servers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Starts what should run and is not running, restarts what changed and
    /// stops what is no longer wanted. Does not wait for any of it.
    pub fn reconcile(&self, wanted: &[Wanted]) {
        if !self.enabled {
            return;
        }
        let mut servers = self.lock();
        let gone: Vec<i64> = servers
            .keys()
            .copied()
            .filter(|id| !wanted.iter().any(|w| w.id == *id))
            .collect();
        for id in gone {
            if let Some(running) = servers.remove(&id) {
                let _ = running.stop.send(true);
            }
        }
        for w in wanted {
            if servers.get(&w.id).is_some_and(|r| r.spec == w.spec) {
                continue;
            }
            if let Some(old) = servers.remove(&w.id) {
                let _ = old.stop.send(true);
            }
            let shared = Arc::new(Shared {
                health: Mutex::new(Health {
                    phase: Phase::Starting,
                    restarts: 0,
                }),
                stderr: Mutex::new(Ring {
                    buf: VecDeque::new(),
                    cap: STDERR_BYTES,
                }),
                live: Mutex::new(None),
            });
            let (stop, stopped) = watch::channel(false);
            let task = tokio::spawn(supervise(
                shared.clone(),
                w.spec.clone(),
                self.root.join(&w.name),
                self.cfg,
                stopped,
            ));
            servers.insert(
                w.id,
                Running {
                    spec: w.spec.clone(),
                    shared,
                    stop,
                    task,
                },
            );
        }
    }

    fn shared(&self, id: i64) -> Option<Arc<Shared>> {
        self.lock().get(&id).map(|r| r.shared.clone())
    }

    /// How the server stands; `None` if it is not supervised.
    pub fn health(&self, id: i64) -> Option<Health> {
        let shared = self.shared(id)?;
        let health = *shared
            .health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Some(health)
    }

    /// The last 64 KiB the program wrote to stderr.
    pub fn stderr(&self, id: i64) -> Option<Vec<u8>> {
        let shared = self.shared(id)?;
        let ring = shared
            .stderr
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Some(ring.buf.iter().copied().collect())
    }

    /// Waits until the program is running, or has failed to start three
    /// times, or `limit` has passed. Whether it is running.
    pub async fn wait_running(&self, id: i64, limit: Duration) -> bool {
        let end = Instant::now() + limit;
        loop {
            match self.health(id).map(|h| h.phase) {
                Some(Phase::Running) => return true,
                None | Some(Phase::CrashLooping | Phase::Stopped) => return false,
                _ => {}
            }
            if Instant::now() >= end {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// One request to a running program.
    pub async fn call(
        &self,
        id: i64,
        method: &str,
        params: &Value,
        opts: Options,
    ) -> Result<Value, UpstreamError> {
        let live = self
            .shared(id)
            .and_then(|s| {
                s.live
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
            })
            .ok_or(UpstreamError::Unavailable)?;
        live.request(method, params, opts).await
    }

    /// Lists the tools of a running program, page by page.
    pub async fn discover(&self, id: i64, opts: Options) -> Result<Discovered, UpstreamError> {
        super::upstream::paginate(|cursor, _| {
            let params = match cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            async move { self.call(id, "tools/list", &params, opts).await }
        })
        .await
    }

    /// Stops every program and waits a little for them to go.
    pub async fn shutdown(&self) {
        let running: Vec<Running> = self.lock().drain().map(|(_, r)| r).collect();
        for r in &running {
            let _ = r.stop.send(true);
        }
        let tasks = running.into_iter().map(|r| r.task);
        let all = futures::future::join_all(tasks);
        let _ = tokio::time::timeout(Duration::from_secs(5), all).await;
    }
}

enum Ended {
    Stopped,
    Exited,
}

async fn supervise(
    shared: Arc<Shared>,
    spec: Arc<Spec>,
    cwd: PathBuf,
    cfg: Config,
    mut stop: watch::Receiver<bool>,
) {
    let mut failures = 0u32;
    let mut wait = cfg.restart_min;
    loop {
        if *stop.borrow() {
            break;
        }
        shared.set_phase(Phase::Starting);
        let started = Instant::now();
        match run_once(&shared, &spec, &cwd, &cfg, &mut stop).await {
            Ended::Stopped => break,
            Ended::Exited => {}
        }
        *shared
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        if started.elapsed() >= cfg.stable_after {
            failures = 0;
            wait = cfg.restart_min;
        } else {
            failures += 1;
        }
        {
            let mut health = shared
                .health
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            health.restarts += 1;
            health.phase = if failures >= CRASH_LOOP_AFTER {
                Phase::CrashLooping
            } else {
                Phase::Backoff
            };
        }
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            _ = stop.changed() => {}
        }
        wait = (wait * 2).min(cfg.restart_max);
    }
    *shared
        .live
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    shared.set_phase(Phase::Stopped);
}

/// The environment variables a program always gets.
fn base_env(cwd: &std::path::Path) -> Vec<(String, String)> {
    vec![
        (
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_string()),
        ),
        ("HOME".to_string(), cwd.display().to_string()),
        ("LANG".to_string(), "C.UTF-8".to_string()),
    ]
}

#[cfg(unix)]
fn command_of(spec: &Spec, cwd: &std::path::Path, cfg: &Config) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(&spec.command);
    cmd.args(&spec.args)
        .env_clear()
        .envs(base_env(cwd))
        .envs(&spec.env)
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        // Its own process group: stopping it reaches what it started.
        .process_group(0);
    let memory = cfg.memory_mib;
    // SAFETY: the closure runs between fork and exec and only calls
    // setrlimit, which is async-signal-safe. A failure is ignored: the
    // limits are best effort.
    unsafe {
        cmd.pre_exec(move || {
            let none = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &none);
            if let Some(mib) = memory {
                let bytes = mib.saturating_mul(1024 * 1024) as libc::rlim_t;
                let limit = libc::rlimit {
                    rlim_cur: bytes,
                    rlim_max: bytes,
                };
                libc::setrlimit(libc::RLIMIT_AS, &limit);
            }
            Ok(())
        });
    }
    cmd
}

#[cfg(not(unix))]
fn command_of(_: &Spec, _: &std::path::Path, _: &Config) -> tokio::process::Command {
    unreachable!("local servers are never enabled without Unix process groups")
}

/// Signals the process group of `pid`.
#[cfg(unix)]
fn signal_group(pid: u32, signal: i32) {
    let Ok(pid) = i32::try_from(pid) else { return };
    // SAFETY: kill takes plain integers; a group that is gone is an error we ignore.
    unsafe {
        libc::kill(-pid, signal);
    }
}

#[cfg(not(unix))]
fn signal_group(_: u32, _: i32) {}

/// Stops the program and everything it started.
async fn terminate(child: &mut Child, pid: Option<u32>, grace: Duration) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        signal_group(pid, libc::SIGTERM);
    }
    if tokio::time::timeout(grace, child.wait()).await.is_err() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    // Whatever is left of the group.
    #[cfg(unix)]
    if let Some(pid) = pid {
        signal_group(pid, libc::SIGKILL);
    }
    let _ = pid;
}

async fn run_once(
    shared: &Arc<Shared>,
    spec: &Spec,
    cwd: &std::path::Path,
    cfg: &Config,
    stop: &mut watch::Receiver<bool>,
) -> Ended {
    if std::fs::create_dir_all(cwd).is_err() {
        tracing::warn!("could not make the working directory of a local MCP server");
        return Ended::Exited;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(cwd, std::fs::Permissions::from_mode(0o700));
    }
    let mut child = match command_of(spec, cwd, cfg).spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(kind = ?e.kind(), "a local MCP server could not be started");
            return Ended::Exited;
        }
    };
    let pid = child.id();
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        terminate(&mut child, pid, cfg.grace).await;
        return Ended::Exited;
    };
    // stderr is read all the time, so the program never blocks on it.
    let shared_err = shared.clone();
    let drain = tokio::spawn(async move {
        let mut stderr = stderr;
        let mut chunk = vec![0u8; 8192];
        while let Ok(n) = stderr.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            shared_err
                .stderr
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(&chunk[..n]);
        }
    });
    let live = Arc::new(Live {
        stdin: AsyncMutex::new(stdin),
        pending: Mutex::default(),
        next: AtomicU64::new(0),
    });
    let mut reader = tokio::spawn(read_loop(live.clone(), stdout, cfg.max_line));

    let opts = Options {
        timeout: cfg.handshake,
        max_body_bytes: cfg.max_line,
    };
    let init = json!({"protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "ufaigw", "version": env!("CARGO_PKG_VERSION")}});
    let handshake = live.request("initialize", &init, opts);
    let shaken = tokio::select! {
        r = handshake => r.is_ok(),
        _ = stop.changed() => false,
    };
    if shaken {
        let _ = live
            .write(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        *shared
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(live.clone());
        shared.set_phase(Phase::Running);
    }
    let ended = if !shaken {
        if *stop.borrow() {
            Ended::Stopped
        } else {
            Ended::Exited
        }
    } else {
        tokio::select! {
            _ = child.wait() => Ended::Exited,
            // The program closed its output or sent too long a line.
            _ = &mut reader => Ended::Exited,
            _ = stop.changed() => {
                if *stop.borrow() { Ended::Stopped } else { Ended::Exited }
            }
        }
    };
    *shared
        .live
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    terminate(&mut child, pid, cfg.grace).await;
    live.fail_all(UpstreamError::Connect);
    reader.abort();
    // The pipe closes with the group; the reader ends by itself.
    let _ = tokio::time::timeout(Duration::from_secs(1), drain).await;
    ended
}

/// Reads the lines a program writes and hands replies to who waits for them.
async fn read_loop(live: Arc<Live>, stdout: tokio::process::ChildStdout, max_line: usize) {
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        line.clear();
        match read_line(&mut reader, &mut line, max_line).await {
            Line::Eof | Line::Failed => break,
            Line::TooLong => {
                live.fail_all(UpstreamError::TooLarge);
                break;
            }
            Line::Read => {}
        }
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let size = line.len();
        let has_method = message.get("method").is_some();
        match (message.get("id").cloned(), has_method) {
            (Some(id), false) => {
                let Some(id) = id.as_u64() else { continue };
                let waiting = live
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&id);
                if let Some(tx) = waiting {
                    let _ = tx.send(Ok((message, size)));
                }
            }
            (Some(id), true) => {
                // The program asks something of us: only `ping` is answered.
                let reply = if message["method"] == "ping" {
                    json!({"jsonrpc": "2.0", "id": id, "result": {}})
                } else {
                    json!({"jsonrpc": "2.0", "id": id,
                           "error": {"code": -32601, "message": "Method not found."}})
                };
                let _ = live.write(&reply).await;
            }
            _ => {}
        }
    }
    live.fail_all(UpstreamError::Connect);
}

enum Line {
    Read,
    Eof,
    TooLong,
    Failed,
}

/// One line without its newline, at most `max` bytes.
async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R, out: &mut Vec<u8>, max: usize) -> Line {
    loop {
        let available = match r.fill_buf().await {
            Ok(a) => a,
            Err(_) => return Line::Failed,
        };
        if available.is_empty() {
            return if out.is_empty() {
                Line::Eof
            } else {
                Line::Read
            };
        }
        match available.iter().position(|b| *b == b'\n') {
            Some(at) => {
                if out.len() + at > max {
                    return Line::TooLong;
                }
                out.extend_from_slice(&available[..at]);
                r.consume(at + 1);
                if out.last() == Some(&b'\r') {
                    out.pop();
                }
                return Line::Read;
            }
            None => {
                let n = available.len();
                if out.len() + n > max {
                    return Line::TooLong;
                }
                out.extend_from_slice(available);
                r.consume(n);
            }
        }
    }
}
