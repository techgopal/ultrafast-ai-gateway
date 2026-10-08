//! sqlx stores a checksum of every migration it has applied, so a changed
//! byte in a shipped file makes every existing database refuse to start.
//! Each SQLite migration is pinned here by the SHA-256 of its bytes. Adding
//! a migration adds a line; no line ever changes.

use std::path::Path;

use sha2::{Digest, Sha256};

const PINNED: [(&str, &str); 16] = [
    (
        "0001_init.sql",
        "f5adcd9d9a503c65a85cc59cc69a07379bfde9c8c3ea08933428b9f7f7716896",
    ),
    (
        "0002_identity.sql",
        "e6a3f15f82eb971419c732743a3b61a9d4dac209097f5883031f80211674e041",
    ),
    (
        "0003_catalog.sql",
        "d19b6ad23129a30c2a538470f5e319469877d91755a4479be15525e125bf13f0",
    ),
    (
        "0004_routes.sql",
        "c52185fb0a0a50a9833ab0b58d51d2ea70ac20a348703156648ead2a28e6a207",
    ),
    (
        "0005_route_everyone.sql",
        "c257dfcb4d2b101297cfe4a51b8ab881bac3a307e84ff26616ede0b47dec600b",
    ),
    (
        "0006_logs.sql",
        "611c427286088ea1b385c0f773097c71476b3156cdc98a98ce1a931f1dfcd90d",
    ),
    (
        "0007_limits.sql",
        "1eb39f10f5b2d5649ee1de9e9c4ef9ef7f252af14b1d7fc0354b3b81f4e3fbb2",
    ),
    (
        "0008_budgets.sql",
        "33eea6dbe3147b69a7fcb1aaef0dee473fb0d6862a7c03abe4b7297e301cb7e1",
    ),
    (
        "0009_route_cache.sql",
        "91e54d0eac21c42690f21ba2c2907d0425b4b89c7c06650640990392071c30ff",
    ),
    (
        "0010_logs_privacy_estimated.sql",
        "cbb2e41dcaa02a75a13d7cea9d223dfb56f8ed27fffaeea41e77072d3279eaeb",
    ),
    (
        "0011_tags.sql",
        "bcba48054c288bdb780bbdfbff28c530a9f9a915a268373bc56e8956ee3fe68f",
    ),
    (
        "0012_key_creator.sql",
        "370d8c32da8480cbc8c16765a400ddb645d6d189595c2ff9d2cffabdf7bfab43",
    ),
    (
        "0013_key_team_only.sql",
        "8baf4e23c07ae8b8da8feb05982b07291fa070b1940ed624cbe473b97878fb99",
    ),
    (
        "0014_alerts.sql",
        "9182acfc8b818ed7e50a7ce7892de3cfd81bc82f5c30a6c4dce4e6685e699977",
    ),
    (
        "0015_oidc.sql",
        "5f47b5009ea2ff2b7c202d4ce6e152dc301fc6ac21fef8c19324c6a927b58248",
    ),
    (
        "0016_alert_owner.sql",
        "dc30d31107ac474710f69d43bc27c12b6e66dcfcd61bd7c2a41738d9ad584cae",
    ),
];

/// The PostgreSQL migrations: the baseline folds SQLite 0001-0015 into one,
/// so the numbers differ from there on. Pinned the same way.
const PINNED_POSTGRES: [(&str, &str); 2] = [
    (
        "0001_baseline.sql",
        "6459fe201768aee81e11533403a897f94d0070f7bbe8164ee1ed1ad608bfbbcd",
    ),
    (
        "0002_alert_owner.sql",
        "19c24f1552ed7ca7791f4283b8c5243f1b66bf0a29fb499c59e87740ebfcc12d",
    ),
];

fn postgres_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations/postgres")
}

#[test]
fn every_postgres_migration_keeps_its_bytes() {
    for (name, expected) in PINNED_POSTGRES {
        let bytes = std::fs::read(postgres_dir().join(name))
            .unwrap_or_else(|e| panic!("migrations/postgres/{name}: {e}"));
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            expected,
            "{name} changed: shipped migrations never change"
        );
    }
}

#[test]
fn no_unpinned_postgres_migration_exists() {
    let mut names: Vec<String> = std::fs::read_dir(postgres_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let pinned: Vec<&str> = PINNED_POSTGRES.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names, pinned,
        "add the SHA-256 of a new migration to PINNED_POSTGRES"
    );
}

fn sqlite_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations/sqlite")
}

#[test]
fn every_sqlite_migration_keeps_its_bytes() {
    for (name, expected) in PINNED {
        let bytes = std::fs::read(sqlite_dir().join(name))
            .unwrap_or_else(|e| panic!("migrations/sqlite/{name}: {e}"));
        let got = hex::encode(Sha256::digest(&bytes));
        assert_eq!(
            got, expected,
            "{name} changed: shipped migrations never change"
        );
    }
}

#[test]
fn no_unpinned_sqlite_migration_exists() {
    let mut names: Vec<String> = std::fs::read_dir(sqlite_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let pinned: Vec<&str> = PINNED.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names, pinned,
        "add the SHA-256 of a new migration to PINNED"
    );
}

#[test]
fn old_migrations_are_not_left_beside_the_directory() {
    let top = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let stray: Vec<_> = std::fs::read_dir(top)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "sql"))
        .collect();
    assert!(
        stray.is_empty(),
        "migrations belong in a per-database directory: {stray:?}"
    );
}

/// The copies the beta.2 upgrade test builds its database from.
#[test]
fn beta2_fixture_migrations_are_the_shipped_ones() {
    let copies = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/migrations-beta2");
    let mut n = 0;
    for entry in std::fs::read_dir(&copies).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            std::fs::read(sqlite_dir().join(&name)).unwrap(),
            "{name}"
        );
        n += 1;
    }
    assert_eq!(n, 13);
}
