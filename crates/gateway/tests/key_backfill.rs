//! Keys made before the gateway kept who made them get their maker from the
//! audit log, and a key a non-admin made for another user in a team becomes
//! a team key, when the database is opened by this build.

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;
use ultrafast_gateway::store::Store;

/// The migrations up to and including `last`, in a directory of their own.
fn migrations_up_to(last: u32, dir: &Path) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    for entry in std::fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let number: u32 = name[..4].parse().unwrap();
        if number <= last {
            std::fs::copy(&path, dir.join(&name)).unwrap();
        }
    }
}

#[tokio::test]
async fn old_keys_get_their_maker_from_the_audit_log() {
    let dir = tempfile::tempdir().unwrap();
    let migrations = dir.path().join("migrations");
    std::fs::create_dir(&migrations).unwrap();
    migrations_up_to(12, &migrations);
    let db = dir.path().join("gateway.db");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(migrations.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    for sql in [
        "INSERT INTO users (id, email, name, role, status) VALUES
            (1, 'maya@example.com', 'Maya', 'admin', 'active'),
            (2, 'arjun@example.com', 'Arjun', 'member', 'active'),
            (3, 'lena@example.com', 'Lena', 'member', 'active')",
        "INSERT INTO teams (id, name) VALUES (10, 'Platform')",
        "INSERT INTO team_members (team_id, user_id, role) VALUES (10, 2, 'lead'), (10, 3, 'member')",
        // 1: the lead's key for Lena in the team. 2: the admin's key for Lena.
        // 3: Lena's own. 4: from the CLI. 5: the lead's for Lena without a team.
        // 6: already has its maker (made by the build before): left as it is.
        // 7: the lead's key for Lena made by the build before, maker kept.
        "INSERT INTO virtual_keys (id, name, key_hash, display, user_id, team_id, created_by) VALUES
            (1, 'lead-for-lena', 'h1', 'd', 3, 10, NULL),
            (2, 'admin-for-lena', 'h2', 'd', 3, 10, NULL),
            (3, 'lena-own', 'h3', 'd', 3, 10, NULL),
            (4, 'cli', 'h4', 'd', NULL, NULL, NULL),
            (5, 'lead-no-team', 'h5', 'd', 3, NULL, NULL),
            (6, 'kept', 'h6', 'd', 3, 10, 3),
            (7, 'lead-for-lena-kept', 'h7', 'd', 3, 10, 2)",
        "INSERT INTO audit_log (actor_user_id, actor_email, action, target_type, target_id, summary) VALUES
            (2, 'arjun@example.com', 'key.create', 'key', 1, 's'),
            (1, 'maya@example.com', 'key.create', 'key', 2, 's'),
            (3, 'lena@example.com', 'key.create', 'key', 3, 's'),
            (2, 'arjun@example.com', 'key.revoke', 'key', 4, 's'),
            (2, 'arjun@example.com', 'key.create', 'key', 5, 's'),
            (2, 'arjun@example.com', 'key.create', 'key', 6, 's'),
            (1, 'maya@example.com', 'key.create', 'key', 1, 'a later entry for the same id')",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    pool.close().await;

    let store = Store::open(&db).await.unwrap();
    store.close().await;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&db))
        .await
        .unwrap();
    let rows: Vec<(i64, Option<i64>, bool)> =
        sqlx::query("SELECT id, created_by, team_only FROM virtual_keys ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap()
            .iter()
            .map(|r| (r.get("id"), r.get("created_by"), r.get("team_only")))
            .collect();
    assert_eq!(
        rows,
        [
            (1, Some(2), true),
            (2, Some(1), false),
            (3, Some(3), false),
            (4, None, false),
            (5, Some(2), false),
            (6, Some(3), false),
            (7, Some(2), true),
        ]
    );
    pool.close().await;
}
