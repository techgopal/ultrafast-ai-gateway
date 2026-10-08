//! Running on PostgreSQL: `UF_DATABASE_URL` for the command line, the master
//! key that then must be given, and no backup download. The refusals need no
//! database; what runs against one needs `UF_TEST_DATABASE_URL`.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use axum::http::StatusCode;
use common::{error_code, org};
use serde_json::Value;
use sqlx::{Connection, Row};
use ultrafast_gateway::secrets::Cipher;
use ultrafast_gateway::store::Dialect;

/// The program with a clean environment plus `env`, run in a data directory
/// of its own.
fn run(data_dir: &Path, env: &[(&str, &str)], args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ultrafast"))
        .env_clear()
        .envs(env.iter().copied())
        .arg("--data-dir")
        .arg(data_dir)
        .args(args)
        .output()
        .expect("the program runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

const NOWHERE: &str = "postgres://nobody:hunter2-secret@127.0.0.1:1/none";

#[test]
fn backup_on_postgres_says_to_use_pg_dump_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let target = dir.path().join("copy.db");
    let out = run(
        &data,
        &[("UF_DATABASE_URL", NOWHERE)],
        &["backup", target.to_str().unwrap()],
    );
    assert!(!out.status.success());
    let err = text(&out.stderr);
    assert!(
        err.contains("Use pg_dump to back up a Postgres database."),
        "{err}"
    );
    assert!(!err.contains("hunter2"), "the password is never shown");
    assert!(!target.exists() && !data.exists());
}

#[test]
fn postgres_needs_the_master_key_in_the_environment_and_makes_no_data_directory() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    for (env, wanted) in [
        (
            vec![("UF_DATABASE_URL", NOWHERE)],
            "UF_MASTER_KEY is required",
        ),
        (
            vec![("UF_DATABASE_URL", NOWHERE), ("UF_MASTER_KEY", "")],
            "UF_MASTER_KEY is required",
        ),
        (
            vec![("UF_DATABASE_URL", NOWHERE), ("UF_MASTER_KEY", "not-hex")],
            "UF_MASTER_KEY is not valid",
        ),
    ] {
        let out = run(&data, &env, &["key", "create", "--name", "k"]);
        assert!(!out.status.success());
        let err = text(&out.stderr);
        assert!(err.contains(wanted), "{err}");
        assert!(
            !err.contains("hunter2") && !err.contains("not-hex"),
            "{err}"
        );
        assert!(!data.exists(), "nothing is created");
    }
}

#[test]
fn a_database_url_must_be_postgres_and_the_pool_size_sane() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let out = run(
        &data,
        &[("UF_DATABASE_URL", "mysql://u:secret-pw@h/db")],
        &[
            "config",
            "export",
            dir.path().join("x.json").to_str().unwrap(),
        ],
    );
    let err = text(&out.stderr);
    assert!(!out.status.success());
    assert!(err.contains("must start with postgres://"), "{err}");
    assert!(!err.contains("secret-pw"), "{err}");
    let out = run(
        &data,
        &[
            ("UF_DATABASE_URL", NOWHERE),
            ("UF_DATABASE_MAX_CONNECTIONS", "0"),
        ],
        &["key", "create", "--name", "k"],
    );
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("UF_DATABASE_MAX_CONNECTIONS must be between"));
    // An unreachable database is a plain failure that does not show the URL.
    let started = std::time::Instant::now();
    let out = run(
        &data,
        &[
            ("UF_DATABASE_URL", NOWHERE),
            ("UF_MASTER_KEY", &Cipher::generate_master_hex()),
        ],
        &["key", "create", "--name", "k"],
    );
    let err = text(&out.stderr);
    assert!(!out.status.success());
    assert!(err.contains("could not connect"), "{err}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "start-up gives up after about 10 s: {:?}",
        started.elapsed()
    );
    assert!(err.contains("within 10 s"), "{err}");
    assert!(!err.contains("hunter2"), "{err}");
    assert!(!data.exists());
}

#[tokio::test]
async fn the_download_is_refused_on_postgres_with_the_pg_dump_text() {
    let org = org().await;
    if org.api.store.dialect() != Dialect::Postgres {
        eprintln!("SKIPPED on SQLite: the download works there");
        return;
    }
    let maya = org.sign_in("maya").await;
    let (status, body) = org.call(Some(&maya), "GET", "/api/backup", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "backup_unsupported");
    assert_eq!(
        body["error"]["message"],
        "Use pg_dump to back up a Postgres database."
    );
    // The refusal is for admins; others are told what they are told elsewhere.
    let lena = org.sign_in("lena").await;
    let (status, _) = org.call(Some(&lena), "GET", "/api/backup", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, settings) = org.call(Some(&maya), "GET", "/api/settings", None).await;
    assert_eq!(settings["database"], "postgres");
}

#[tokio::test]
async fn every_command_that_opens_the_store_uses_the_database_url() {
    let Ok(base) = std::env::var("UF_TEST_DATABASE_URL") else {
        eprintln!("SKIPPED without UF_TEST_DATABASE_URL: no PostgreSQL to connect to");
        return;
    };
    // A schema of its own, chosen by the URL as an operator can.
    let mut admin = sqlx::PgConnection::connect(&base).await.unwrap();
    let schema = format!("t_cli_{}", std::process::id());
    for sql in [
        format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
        format!("CREATE SCHEMA {schema}"),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut admin)
            .await
            .unwrap();
    }
    let sep = if base.contains('?') { '&' } else { '?' };
    let url = format!("{base}{sep}options=-c%20search_path%3D{schema}");
    let master = Cipher::generate_master_hex();
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let env = [
        ("UF_DATABASE_URL", url.as_str()),
        ("UF_MASTER_KEY", master.as_str()),
        ("UF_DATABASE_MAX_CONNECTIONS", "2"),
    ];
    let ok = |args: &[&str]| {
        let out = run(&data, &env, args);
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
        text(&out.stdout)
    };
    ok(&[
        "provider",
        "add",
        "--name",
        "main",
        "--kind",
        "openai",
        "--base-url",
        "https://api.example/v1",
        "--api-key",
        "sk-provider-secret",
    ]);
    ok(&[
        "model",
        "add",
        "--provider",
        "main",
        "--model",
        "gpt-4o",
        "--enable",
        "--everyone",
    ]);
    let made = ok(&["key", "create", "--name", "ci"]);
    assert!(made.contains("Created key 'ci'"));
    let file = dir.path().join("config.json");
    ok(&["config", "export", file.to_str().unwrap()]);
    let exported: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(exported["providers"][0]["name"], "main");
    ok(&["config", "import", file.to_str().unwrap(), "--dry-run"]);
    // All of it went to PostgreSQL: no data directory, no database file.
    assert!(!data.exists());
    let there: i64 = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM {schema}.providers WHERE name = 'main'"
    )))
    .fetch_one(&mut admin)
    .await
    .unwrap()
    .get(0);
    assert_eq!(there, 1);
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&mut admin)
        .await
        .unwrap();
}
