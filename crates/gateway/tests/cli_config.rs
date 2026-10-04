//! `ultrafast config export` and `ultrafast config import`, run as the
//! program: they need no master key and touch no port.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use ultrafast_gateway::config::db_path;
use ultrafast_gateway::store::{Grants, RouteSettings, Store, TargetsInput};

fn run(data_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ultrafast"))
        .env_clear()
        .arg("--data-dir")
        .arg(data_dir)
        .args(args)
        .output()
        .expect("the program runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

async fn seeded(dir: &Path) {
    let store = Store::open(&db_path(dir)).await.unwrap();
    let provider = store
        .insert_provider(
            "main",
            "openai",
            "https://api.example/v1",
            Some(b"ciphertext-of-a-credential"),
        )
        .await
        .unwrap();
    let mut tx = store.begin().await.unwrap();
    let team = tx.insert_team("Platform").await.unwrap();
    let model = tx.insert_model(provider, "gpt-4o").await.unwrap();
    tx.set_model_enabled(model, true).await.unwrap();
    tx.replace_grants(
        model,
        &Grants {
            everyone: false,
            team_ids: vec![team],
            user_ids: vec![],
        },
    )
    .await
    .unwrap();
    let route = tx
        .insert_route(
            "chat",
            &RouteSettings {
                retries: 1,
                first_token_timeout_ms: 10_000,
                total_timeout_ms: 60_000,
                breaker_failures: 3,
                breaker_window_s: 20,
                breaker_open_s: 10,
            },
            true,
        )
        .await
        .unwrap();
    tx.replace_targets(
        route,
        &TargetsInput {
            primaries: vec![(model, 1)],
            fallbacks: vec![],
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn a_configuration_goes_from_one_data_directory_to_another() {
    let from = tempfile::tempdir().unwrap();
    let to = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    seeded(from.path()).await;
    let exported = files.path().join("config.json");
    let exported_arg = exported.to_str().unwrap();

    let out = run(from.path(), &["config", "export", exported_arg]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("holds no credentials"));
    let file: Value = serde_json::from_slice(&std::fs::read(&exported).unwrap()).unwrap();
    assert_eq!(file["format"], "ultrafast-config");
    assert!(!file.to_string().contains("ciphertext"));
    // No master key was read or made for it.
    assert!(!from.path().join("master.key").exists());

    // The file is never overwritten.
    let again = run(from.path(), &["config", "export", exported_arg]);
    assert!(!again.status.success());
    assert!(
        text(&again.stderr).contains("must not exist"),
        "{}",
        text(&again.stderr)
    );

    // A dry run says what it would do and writes nothing.
    let dry = run(to.path(), &["config", "import", exported_arg, "--dry-run"]);
    assert!(dry.status.success(), "{}", text(&dry.stderr));
    let said = text(&dry.stdout);
    assert!(said.contains("Dry run: nothing was written"), "{said}");
    assert!(said.contains("created: provider main"), "{said}");
    assert!(said.contains("warning: providers[0]"), "{said}");
    assert!(
        said.contains("4 created, 0 updated, 1 unchanged."),
        "{said}"
    );
    let empty = Store::open(&db_path(to.path())).await.unwrap();
    assert!(empty.config_state().await.unwrap().providers.is_empty());
    empty.close().await;

    // The real import, and the second one that finds nothing to do.
    let done = run(to.path(), &["config", "import", exported_arg]);
    assert!(done.status.success(), "{}", text(&done.stderr));
    assert!(text(&done.stdout).contains("4 created, 0 updated, 1 unchanged."));
    let second = run(to.path(), &["config", "import", exported_arg]);
    assert!(
        text(&second.stdout).contains("0 created, 0 updated, 5 unchanged."),
        "{}",
        text(&second.stdout)
    );
    assert!(!to.path().join("master.key").exists());

    // What was imported exports to the same file.
    let back = files.path().join("back.json");
    let out = run(to.path(), &["config", "export", back.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        std::fs::read(&back).unwrap(),
        std::fs::read(&exported).unwrap()
    );

    // And it was audited as the command line.
    let store = Store::open(&db_path(to.path())).await.unwrap();
    let audit = store.list_audit(50, None).await.unwrap();
    assert!(audit
        .iter()
        .any(|r| r.action == "config.import" && r.actor_email == "cli"));
    store.close().await;
}

#[tokio::test]
async fn a_file_with_errors_is_refused_and_exits_with_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let file = files.path().join("bad.json");
    std::fs::write(
        &file,
        r#"{"format":"ultrafast-config","version":1,"teams":[{"name":""}],
            "budgets":[{"scope":"team","name":"Nobody","amount_micros":1,"period":"daily","action":"block"}]}"#,
    )
    .unwrap();
    let out = run(dir.path(), &["config", "import", file.to_str().unwrap()]);
    assert!(!out.status.success());
    let said = text(&out.stdout);
    assert!(
        said.contains("error: teams[0].name: name must be 1 to 60 characters"),
        "{said}"
    );
    assert!(
        said.contains("error: budgets[0].name: team 'Nobody' does not exist"),
        "{said}"
    );
    // Not a file of ours at all.
    std::fs::write(&file, "nope").unwrap();
    let out = run(dir.path(), &["config", "import", file.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("not a configuration file"),
        "{}",
        text(&out.stderr)
    );
    // Export needs a database to export.
    let nothing = tempfile::tempdir().unwrap();
    let out = run(
        nothing.path(),
        &[
            "config",
            "export",
            files.path().join("x.json").to_str().unwrap(),
        ],
    );
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("no database"),
        "{}",
        text(&out.stderr)
    );
}
