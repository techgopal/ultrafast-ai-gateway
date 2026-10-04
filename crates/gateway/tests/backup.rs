//! `GET /api/backup` and `ultrafast backup`: a consistent copy of the
//! database, taken while the gateway writes.

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{error_code, seed_user, ORG_PASSWORD};
use tower::ServiceExt;
use ultrafast_gateway::app::{router, AppState};
use ultrafast_gateway::config::db_path;
use ultrafast_gateway::identity::password::warm_up;
use ultrafast_gateway::identity::Role;
use ultrafast_gateway::secrets::Cipher;
use ultrafast_gateway::store::{AuditEntry, Store};

/// A gateway over a database file in a directory of its own.
struct Disk {
    dir: tempfile::TempDir,
    app: axum::Router,
    store: Store,
    state: Arc<AppState>,
}

async fn disk() -> Disk {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&db_path(dir.path())).await.unwrap();
    warm_up().unwrap();
    let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
    let mut state = AppState::new(store.clone(), cipher).await.unwrap();
    state.cookie_secure = false;
    let state = Arc::new(state);
    seed_user(&store, "maya@example.com", Role::Admin, ORG_PASSWORD).await;
    seed_user(&store, "lena@example.com", Role::Member, ORG_PASSWORD).await;
    Disk {
        app: router(state.clone()),
        dir,
        store,
        state,
    }
}

async fn download(
    app: &axum::Router,
    signed: Option<&common::Signed>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut req = Request::builder().method("GET").uri("/api/backup");
    if let Some(s) = signed {
        req = req.header("cookie", &s.cookie);
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = resp.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    (parts.status, parts.headers, bytes.to_vec())
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("backup") || n.ends_with(".tmp"))
        .collect()
}

/// A copy that was written to a directory, opened as the gateway opens a
/// database (which runs its migrations), and a plain connection to ask it
/// what the gateway has no call for.
struct Copy {
    _dir: tempfile::TempDir,
    store: Store,
    sql: sqlx::SqlitePool,
}

async fn open_copy(bytes: &[u8]) -> Copy {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(db_path(dir.path()), bytes).unwrap();
    // Opening runs the migrations: a copy that does not pass them fails here.
    let store = Store::open(&db_path(dir.path())).await.unwrap();
    let sql = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(db_path(dir.path())),
    )
    .await
    .unwrap();
    Copy {
        _dir: dir,
        store,
        sql,
    }
}

#[tokio::test]
async fn an_admin_downloads_a_sqlite_file_with_the_tables_and_nothing_is_left_behind() {
    let d = disk().await;
    let maya = common::sign_in(&d.app, "maya@example.com", ORG_PASSWORD).await;
    let (status, headers, bytes) = download(&d.app, Some(&maya)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/vnd.sqlite3");
    let disposition = headers["content-disposition"].to_str().unwrap();
    assert!(
        disposition.starts_with("attachment; filename=\"ultrafast-"),
        "{disposition}"
    );
    assert!(disposition.ends_with(".db\""), "{disposition}");
    // `ultrafast-YYYYMMDD-HHMMSS.db`
    let name = disposition
        .trim_start_matches("attachment; filename=\"")
        .trim_end_matches('"');
    assert_eq!(name.len(), "ultrafast-20260101-000000.db".len(), "{name}");
    assert_eq!(
        headers["content-length"].to_str().unwrap(),
        bytes.len().to_string()
    );
    assert!(bytes.starts_with(b"SQLite format 3\0"));

    // A file of the same database: its tables, and the users that were in it.
    let copy = open_copy(&bytes).await;
    let users = copy.store.list_users().await.unwrap();
    assert_eq!(users.len(), 2);
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .fetch_all(&copy.sql)
            .await
            .unwrap();
    for table in [
        "audit_log",
        "models",
        "providers",
        "request_logs",
        "routes",
        "settings",
        "teams",
        "users",
        "virtual_keys",
    ] {
        assert!(
            tables.contains(&table.to_string()),
            "{table} is not in {tables:?}"
        );
    }
    assert!(
        leftovers(d.dir.path()).is_empty(),
        "{:?}",
        leftovers(d.dir.path())
    );
    // It is audited, in the database it was taken of.
    assert_eq!(
        d.store
            .list_audit(5, None)
            .await
            .unwrap()
            .iter()
            .find(|r| r.action == "backup.download")
            .map(|r| r.summary.clone()),
        Some("Downloaded a backup of the database".to_string())
    );
}

#[tokio::test]
async fn only_an_admin_downloads_a_backup() {
    let d = disk().await;
    let lena = common::sign_in(&d.app, "lena@example.com", ORG_PASSWORD).await;
    let (status, _, bytes) = download(&d.app, Some(&lena)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(error_code(&body), "forbidden");
    let (status, _, _) = download(&d.app, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(leftovers(d.dir.path()).is_empty());
    assert!(d
        .store
        .list_audit(50, None)
        .await
        .unwrap()
        .iter()
        .all(|r| r.action != "backup.download"));
}

/// Writers that put one row into each of two tables in one transaction,
/// as fast as they can. A copy that is consistent has as many of one as of
/// the other.
fn writers(
    store: &Store,
    n: usize,
) -> (
    Vec<tokio::task::JoinHandle<()>>,
    tokio::sync::watch::Sender<bool>,
) {
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let handles = (0..n)
        .map(|w| {
            let store = store.clone();
            let mut stopped = stopped.clone();
            tokio::spawn(async move {
                let mut i = 0u64;
                while !*stopped.borrow_and_update() {
                    let mut tx = store.begin().await.unwrap();
                    let summary = format!("w{w}-{i}");
                    tx.audit(AuditEntry {
                        actor_user_id: None,
                        actor_email: "writer",
                        action: "test.pair",
                        target_type: "test",
                        target_id: None,
                        summary: &summary,
                    })
                    .await
                    .unwrap();
                    tx.set_log_retention_days(i64::try_from(1 + i % 3000).unwrap())
                        .await
                        .unwrap();
                    tx.audit(AuditEntry {
                        actor_user_id: None,
                        actor_email: "writer",
                        action: "test.pair-end",
                        target_type: "test",
                        target_id: None,
                        summary: &summary,
                    })
                    .await
                    .unwrap();
                    tx.commit().await.unwrap();
                    i += 1;
                    tokio::task::yield_now().await;
                }
            })
        })
        .collect();
    (handles, stop)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_backup_taken_while_the_gateway_writes_is_consistent_migrated_and_serves() {
    let d = disk().await;
    let maya = common::sign_in(&d.app, "maya@example.com", ORG_PASSWORD).await;
    let (handles, stop) = writers(&d.store, 4);
    // Let the writers get going, then back up several times in the middle of them.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let mut copies = Vec::new();
    for _ in 0..4 {
        let (status, _, bytes) = download(&d.app, Some(&maya)).await;
        assert_eq!(status, StatusCode::OK);
        copies.push(bytes);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let _ = stop.send(true);
    for h in handles {
        h.await.unwrap();
    }
    let mut seen_pairs = 0;
    for bytes in copies {
        let copy = open_copy(&bytes).await;
        let check: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&copy.sql)
            .await
            .unwrap();
        assert_eq!(check, "ok");
        // Every transaction of the writers is in the copy whole, or not at all.
        let starts: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = 'test.pair'")
                .fetch_one(&copy.sql)
                .await
                .unwrap();
        let ends: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = 'test.pair-end'")
                .fetch_one(&copy.sql)
                .await
                .unwrap();
        assert_eq!(starts, ends, "a transaction was cut in two");
        seen_pairs += starts;

        // It serves: a gateway over the copy signs a user in and answers.
        let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        let mut state = AppState::new(copy.store.clone(), cipher).await.unwrap();
        state.cookie_secure = false;
        let app = router(Arc::new(state));
        let signed = common::sign_in(&app, "maya@example.com", ORG_PASSWORD).await;
        let (status, _, body) = common::call(&app, "GET", "/api/users", Some(&signed), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        copy.store.close().await;
    }
    assert!(
        seen_pairs > 0,
        "the writers wrote nothing before the backups"
    );
    assert!(
        leftovers(d.dir.path()).is_empty(),
        "{:?}",
        leftovers(d.dir.path())
    );
    let _ = &d.state;
}

fn program(data_dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ultrafast"))
        .env_clear()
        .arg("--data-dir")
        .arg(data_dir)
        .args(args)
        .output()
        .unwrap()
}

#[tokio::test]
async fn the_command_writes_a_backup_that_has_no_master_key() {
    let d = disk().await;
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("copy.db");
    let ran = program(d.dir.path(), &["backup", target.to_str().unwrap()]);
    let said = String::from_utf8_lossy(&ran.stdout).to_string();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(said.contains("useless without the master key"), "{said}");
    let copy = open_copy(&std::fs::read(&target).unwrap()).await;
    assert_eq!(copy.store.list_users().await.unwrap().len(), 2);
    // No master key was read or made, and none went into the copy's directory.
    assert!(!d.dir.path().join("master.key").exists());
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        1,
        "only the copy is there: {:?}",
        leftovers(out.path())
    );

    // It never overwrites, and it needs a database to copy.
    let again = program(d.dir.path(), &["backup", target.to_str().unwrap()]);
    assert!(!again.status.success());
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("already exists"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    let nothing = tempfile::tempdir().unwrap();
    let none = program(
        nothing.path(),
        &["backup", out.path().join("x.db").to_str().unwrap()],
    );
    assert!(!none.status.success());
    assert!(String::from_utf8_lossy(&none.stderr).contains("no database"));
}
