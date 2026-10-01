//! Data written by an earlier build must stay readable.
//!
//! `fixtures/v2-alpha1.db` was written by the gateway built with sqlx 0.8,
//! sha2 0.10, chacha20poly1305 0.10 and argon2 0.5: both migrations applied,
//! one provider, one virtual key, one user. Never regenerate it.

use std::path::{Path, PathBuf};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;
use ultrafast_gateway::identity::password::verify_password;
use ultrafast_gateway::identity::{Role, UserStatus};
use ultrafast_gateway::secrets::{hash_key, Cipher};
use ultrafast_gateway::store::Store;

const MASTER: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
const KEY: &str = "uf-sk-00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
const CREDENTIAL_HEX: &str = "ab9df8363ccae36d1c5561c3498fad8249f6c496f7333e0a19e74e8f4e245ac658ece260e7e13e1964a7a124881fb5c4b4c7aa776f7063573b41";
const PASSWORD: &str = "fixture password 2026";

/// The rows sqlx 0.8 wrote: version, description, success, checksum as hex.
const MIGRATIONS: [(i64, &str, bool, &str); 2] = [
    (1, "init", true, "dda2bdbc1700deca3eb92245a1c053ed9a9007c088be19e261614dbf29056049e87727339580d38914c3d080e72ed708"),
    (2, "identity", true, "5721e96ccbcb67ab2846ea34faed6d847c930218c2dde3ef1020abd7085ff708a850cab0d4ca3aa42397241cd1479783"),
];

fn copy_of_fixture(dir: &Path) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v2-alpha1.db");
    let to = dir.join("gateway.db");
    std::fs::copy(from, &to).unwrap();
    to
}

/// Every column of every row of the migrations table, read without `Store`.
async fn migration_rows(path: &Path) -> Vec<(i64, String, String, bool, String, i64)> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path))
        .await
        .unwrap();
    let rows = sqlx::query(
        "SELECT version, description, installed_on, success, checksum, execution_time
         FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .iter()
    .map(|r| {
        (
            r.get::<i64, _>("version"),
            r.get::<String, _>("description"),
            r.get::<String, _>("installed_on"),
            r.get::<bool, _>("success"),
            hex::encode(r.get::<Vec<u8>, _>("checksum")),
            r.get::<i64, _>("execution_time"),
        )
    })
    .collect();
    pool.close().await;
    rows
}

#[tokio::test]
async fn a_database_written_by_the_earlier_build_opens_and_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_of_fixture(dir.path());
    let before = migration_rows(&path).await;
    assert_eq!(before.len(), MIGRATIONS.len());
    for (row, (version, description, success, checksum)) in before.iter().zip(MIGRATIONS) {
        assert_eq!(row.0, version);
        assert_eq!(row.1, description);
        assert_eq!(row.3, success);
        assert_eq!(row.4, checksum);
    }

    let store = Store::open(&path).await.expect("the stored database opens");

    let providers = store.list_providers().await.unwrap();
    assert_eq!(providers.len(), 1);
    let provider = &providers[0];
    assert_eq!(provider.name, "fixture-openai");
    assert_eq!(provider.kind, "openai");
    assert_eq!(provider.base_url, "https://api.example.com/v1");
    let credential = provider.credential.as_deref().unwrap();
    assert_eq!(hex::encode(credential), CREDENTIAL_HEX);
    let cipher = Cipher::from_hex(MASTER).unwrap();
    assert_eq!(
        cipher.decrypt(credential).unwrap(),
        b"sk-fixture-provider-credential"
    );

    let key = store
        .active_key_by_hash(&hash_key(KEY))
        .await
        .unwrap()
        .expect("the stored key is found by the hash of its value");
    assert_eq!(key.name, "fixture-key");
    assert_eq!(key.display, "uf-sk-\u{2026}eeff");
    assert_eq!(store.list_keys().await.unwrap().len(), 1);

    let user = store
        .user_by_email("admin@example.com")
        .await
        .unwrap()
        .expect("the stored user is found");
    assert_eq!(user.name, "Fixture Admin");
    assert_eq!(user.role, Role::Admin);
    assert_eq!(user.status, UserStatus::Active);
    let phc = user.password_hash.as_deref().unwrap();
    assert!(phc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
    assert!(verify_password(PASSWORD, phc));
    assert!(!verify_password("fixture password 2025", phc));

    store.close().await;
    // Opening applied the migrations written since, and rewrote none of the
    // rows the earlier build left.
    let after = migration_rows(&path).await;
    assert_eq!(after[..before.len()], before[..]);
    let later: Vec<(i64, &str, bool)> = after[before.len()..]
        .iter()
        .map(|r| (r.0, r.1.as_str(), r.3))
        .collect();
    assert_eq!(
        later,
        [
            (3, "catalog", true),
            (4, "routes", true),
            (5, "route everyone", true)
        ]
    );

    // A second open of the same file behaves the same and migrates nothing.
    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.list_providers().await.unwrap().len(), 1);
    store.close().await;
    assert_eq!(migration_rows(&path).await, after);
}
