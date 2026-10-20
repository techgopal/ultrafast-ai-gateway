//! Local MCP servers: programs the gateway supervises and talks to over stdio.
//! The program is the test-support binary `mcp-stdio-mock`.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use ufaigw_gateway::mcp::local::{Config, Health, Phase, Spec, Supervisor, Wanted};
use ufaigw_gateway::mcp::upstream::{Options, UpstreamError};

const MOCK: &str = env!("CARGO_BIN_EXE_mcp-stdio-mock");

fn fast() -> Config {
    Config {
        restart_min: Duration::from_millis(50),
        restart_max: Duration::from_millis(400),
        grace: Duration::from_millis(500),
        handshake: Duration::from_secs(5),
        ..Config::default()
    }
}

fn wanted(id: i64, name: &str, args: &[&str], env: &[(&str, &str)]) -> Wanted {
    Wanted {
        id,
        name: name.to_string(),
        spec: std::sync::Arc::new(Spec {
            command: MOCK.to_string(),
            args: args.iter().map(|a| (*a).to_string()).collect(),
            env: env
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        }),
    }
}

fn opts() -> Options {
    Options {
        timeout: Duration::from_secs(5),
        max_body_bytes: 1024 * 1024,
    }
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn phase(s: &Supervisor, id: i64) -> Option<Phase> {
    s.health(id).map(|h| h.phase)
}

async fn running(s: &Supervisor, id: i64) {
    until("a running server", || phase(s, id) == Some(Phase::Running)).await;
}

async fn tool(s: &Supervisor, id: i64, name: &str, args: Value) -> Result<String, UpstreamError> {
    let params = json!({"name": name, "arguments": args});
    let result = s.call(id, "tools/call", &params, opts()).await?;
    Ok(result["content"][0]["text"].as_str().unwrap().to_string())
}

/// Whether a process exists and is not a zombie.
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            s.rsplit(')')
                .next()
                .map(|r| r.trim_start().starts_with(['Z', 'X']))
        })
        .is_some_and(|dead| !dead)
}

fn supervisor(cfg: Config) -> (Supervisor, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (Supervisor::new(dir.path().join("mcp"), cfg), dir)
}

#[tokio::test]
async fn a_server_starts_answers_and_is_stopped_with_what_it_started() {
    let (s, _dir) = supervisor(fast());
    assert!(s.enabled());
    s.reconcile(&[wanted(1, "one", &["--child"], &[])]);
    running(&s, 1).await;
    let found = s.discover(1, opts()).await.unwrap();
    assert_eq!(found.tools.len(), 6);
    assert_eq!(found.tools[0].name, "echo");
    let pids = tool(&s, 1, "pids", json!({})).await.unwrap();
    let (me, child): (u32, u32) = {
        let mut it = pids.split(' ').map(|p| p.parse().unwrap());
        (it.next().unwrap(), it.next().unwrap())
    };
    assert!(alive(me) && alive(child));
    // Taken off the list: both the program and its child go.
    s.reconcile(&[]);
    until("the program and its child to be gone", || {
        !alive(me) && !alive(child)
    })
    .await;
    assert_eq!(s.health(1), None);
    assert_eq!(
        tool(&s, 1, "echo", json!({})).await,
        Err(UpstreamError::Unavailable)
    );
}

#[tokio::test]
async fn shutdown_stops_every_program() {
    let (s, _dir) = supervisor(fast());
    s.reconcile(&[wanted(1, "one", &["--child"], &[])]);
    running(&s, 1).await;
    let pids = tool(&s, 1, "pids", json!({})).await.unwrap();
    let me: u32 = pids.split(' ').next().unwrap().parse().unwrap();
    s.shutdown().await;
    until("the program to be gone", || !alive(me)).await;
}

#[tokio::test]
async fn concurrent_calls_get_their_own_answers() {
    let (s, _dir) = supervisor(fast());
    s.reconcile(&[wanted(1, "one", &[], &[])]);
    running(&s, 1).await;
    let s = std::sync::Arc::new(s);
    let calls: Vec<_> = (0..40)
        .map(|n| {
            let s = s.clone();
            tokio::spawn(async move { (n, tool(&s, 1, "echo", json!({"n": n})).await.unwrap()) })
        })
        .collect();
    for call in calls {
        let (n, text) = call.await.unwrap();
        assert_eq!(text, format!("{{\"n\":{n}}}"));
    }
}

#[tokio::test]
async fn a_program_that_dies_at_once_is_restarted_with_growing_waits() {
    let (s, _dir) = supervisor(fast());
    s.reconcile(&[wanted(1, "loop", &["--crash"], &[])]);
    until("a crash loop", || phase(&s, 1) == Some(Phase::CrashLooping)).await;
    // Waits of 50, 100, 200, 400, 400 ms: in a second and a half there are
    // a handful of starts, not hundreds.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let Health { restarts, phase } = s.health(1).unwrap();
    assert!((4..=12).contains(&restarts), "{restarts} restarts");
    assert!(
        matches!(phase, Phase::CrashLooping | Phase::Starting),
        "{phase:?}"
    );
    // A call says so, without waiting for the program.
    let started = Instant::now();
    assert_eq!(
        tool(&s, 1, "echo", json!({})).await,
        Err(UpstreamError::Unavailable)
    );
    assert!(started.elapsed() < Duration::from_millis(100));
    // What it said before dying is kept.
    let err = String::from_utf8(s.stderr(1).unwrap()).unwrap();
    assert!(err.contains("told to crash"), "{err}");
    // Edited into a working program, it comes up on its own.
    s.reconcile(&[wanted(1, "loop", &[], &[])]);
    running(&s, 1).await;
}

#[tokio::test]
async fn the_first_waits_double_up_to_the_cap() {
    let mut cfg = fast();
    cfg.restart_min = Duration::from_millis(200);
    cfg.restart_max = Duration::from_millis(400);
    let (s, _dir) = supervisor(cfg);
    s.reconcile(&[wanted(1, "loop", &["--crash"], &[])]);
    tokio::time::sleep(Duration::from_millis(1300)).await;
    // Starts at about 0, 200, 600, 1000 ms: three or four restarts.
    let restarts = s.health(1).unwrap().restarts;
    assert!((3..=5).contains(&restarts), "{restarts}");
}

#[tokio::test]
async fn stderr_is_capped_and_never_blocks_the_program() {
    let (s, _dir) = supervisor(fast());
    s.reconcile(&[wanted(1, "noisy", &["--stderr", "20000000"], &[])]);
    // The program writes 20 MB before it answers `initialize`; it is
    // running, so it was never held up by a full pipe.
    running(&s, 1).await;
    until("the end of the noise", || {
        s.stderr(1).is_some_and(|e| e.ends_with(b"END\n"))
    })
    .await;
    let err = s.stderr(1).unwrap();
    assert_eq!(err.len(), 64 * 1024);
    assert_eq!(
        tool(&s, 1, "echo", json!({"a": 1})).await.unwrap(),
        "{\"a\":1}"
    );
}

#[tokio::test]
async fn only_the_listed_names_reach_the_program() {
    std::env::set_var("PARENT_ONLY_SECRET", "leak");
    std::env::set_var("UF_MASTER_KEY", "leak");
    let (s, _dir) = supervisor(fast());
    s.reconcile(&[wanted(
        1,
        "env",
        &[],
        &[("LISTED", "value-1"), ("OTHER", "2")],
    )]);
    running(&s, 1).await;
    let names = tool(&s, 1, "env", json!({})).await.unwrap();
    assert_eq!(names, "HOME,LANG,LISTED,OTHER,PATH");
    assert_eq!(
        tool(&s, 1, "env", json!({"name": "LISTED"})).await.unwrap(),
        "value-1"
    );
    assert_eq!(
        tool(&s, 1, "env", json!({"name": "PARENT_ONLY_SECRET"}))
            .await
            .unwrap(),
        ""
    );
}

#[tokio::test]
async fn the_working_directory_is_the_servers_own_under_the_data_dir() {
    let (s, dir) = supervisor(fast());
    s.reconcile(&[wanted(1, "cwd-one", &[], &[])]);
    running(&s, 1).await;
    let cwd = tool(&s, 1, "cwd", json!({})).await.unwrap();
    let expected = dir.path().join("mcp").join("cwd-one");
    assert_eq!(
        std::fs::canonicalize(&cwd).unwrap(),
        std::fs::canonicalize(&expected).unwrap()
    );
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&expected).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0, "{mode:o}");
}

#[tokio::test]
async fn as_root_nothing_is_started() {
    let cfg = Config {
        euid: Some(0),
        ..fast()
    };
    let (s, _dir) = supervisor(cfg);
    assert!(!s.enabled());
    s.reconcile(&[wanted(1, "one", &[], &[])]);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(s.health(1), None);
    assert_eq!(
        tool(&s, 1, "echo", json!({})).await,
        Err(UpstreamError::Unavailable)
    );
}

#[tokio::test]
async fn a_disabled_supervisor_starts_nothing() {
    let s = Supervisor::disabled();
    assert!(!s.enabled());
    s.reconcile(&[wanted(1, "one", &[], &[])]);
    assert_eq!(s.health(1), None);
}

#[tokio::test]
async fn a_changed_spec_restarts_the_program_and_an_unchanged_one_does_not() {
    let (s, _dir) = supervisor(fast());
    s.reconcile(&[wanted(1, "one", &[], &[("A", "1")])]);
    running(&s, 1).await;
    let before = tool(&s, 1, "pids", json!({})).await.unwrap();
    s.reconcile(&[wanted(1, "one", &[], &[("A", "1")])]);
    assert_eq!(tool(&s, 1, "pids", json!({})).await.unwrap(), before);
    s.reconcile(&[wanted(1, "one", &[], &[("A", "2")])]);
    until("the new program", || {
        s.health(1).is_some_and(|h| h.phase == Phase::Running)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    running(&s, 1).await;
    assert_ne!(tool(&s, 1, "pids", json!({})).await.unwrap(), before);
    assert_eq!(tool(&s, 1, "env", json!({"name": "A"})).await.unwrap(), "2");
}

#[tokio::test]
async fn a_line_over_the_cap_ends_the_connection_and_the_program_is_started_again() {
    let mut cfg = fast();
    cfg.max_line = 100_000;
    let (s, _dir) = supervisor(cfg);
    s.reconcile(&[wanted(1, "big", &[], &[])]);
    running(&s, 1).await;
    let big = opts();
    let params = json!({"name": "big", "arguments": {"n": 500_000}});
    let error = s.call(1, "tools/call", &params, big).await.unwrap_err();
    assert_eq!(error, UpstreamError::TooLarge);
    until("a restart", || s.health(1).is_some_and(|h| h.restarts == 1)).await;
    running(&s, 1).await;
    assert_eq!(tool(&s, 1, "echo", json!({})).await.unwrap(), "{}");
}

#[tokio::test]
async fn a_result_over_the_callers_cap_is_too_large_and_the_program_stays() {
    let (s, _dir) = supervisor(fast());
    s.reconcile(&[wanted(1, "one", &[], &[])]);
    running(&s, 1).await;
    let params = json!({"name": "big", "arguments": {"n": 50_000}});
    let small = Options {
        timeout: Duration::from_secs(5),
        max_body_bytes: 1_000,
    };
    assert_eq!(
        s.call(1, "tools/call", &params, small).await.unwrap_err(),
        UpstreamError::TooLarge
    );
    assert_eq!(s.health(1).unwrap().restarts, 0);
}

#[tokio::test]
async fn a_program_that_dies_mid_call_fails_the_call_and_comes_back() {
    let (s, _dir) = supervisor(fast());
    s.reconcile(&[wanted(1, "one", &[], &[])]);
    running(&s, 1).await;
    assert_eq!(
        tool(&s, 1, "die", json!({})).await,
        Err(UpstreamError::Connect)
    );
    until("a restart", || s.health(1).is_some_and(|h| h.restarts == 1)).await;
    running(&s, 1).await;
    assert_eq!(tool(&s, 1, "echo", json!({})).await.unwrap(), "{}");
}
