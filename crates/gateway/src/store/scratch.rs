//! A private schema in the PostgreSQL database that tests are pointed at
//! (`UF_TEST_DATABASE_URL`), so each test store starts empty and tests can
//! run side by side. Nothing here is used unless that variable is set.

use anyhow::Result;
use sqlx::any::{install_default_drivers, AnyPoolOptions};
use sqlx::Connection;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::{Dialect, Store};

/// The PostgreSQL URL tests run against, if one is set.
pub(super) fn test_database_url() -> Option<String> {
    std::env::var("UF_TEST_DATABASE_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
}

/// The schema a test store lives in. Dropping the last clone of the store
/// drops it, best effort: a failure leaves an unused schema behind.
pub(super) struct Schema {
    url: String,
    name: String,
}

impl Drop for Schema {
    fn drop(&mut self) {
        let (url, name) = (self.url.clone(), self.name.clone());
        let (done, finished) = std::sync::mpsc::channel();
        // A thread of its own: this may run inside a runtime that is going away.
        std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                rt.block_on(async {
                    if let Ok(mut c) = sqlx::postgres::PgConnection::connect(&url).await {
                        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
                            "DROP SCHEMA IF EXISTS {name} CASCADE"
                        )))
                        .execute(&mut c)
                        .await;
                    }
                });
            }
            let _ = done.send(());
        });
        // Waits for it, so a test binary that ends leaves nothing behind, but
        // never for long: a stuck drop only leaves an unused schema.
        let _ = finished.recv_timeout(std::time::Duration::from_secs(5));
    }
}

fn schema_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let random: u64 = rand::random();
    format!("t_{:08x}_{n}", random >> 32)
}

/// A store in a new schema of the database at `url`.
pub(super) async fn open(url: &str) -> Result<Store> {
    install_default_drivers();
    anyhow::ensure!(
        Dialect::of_url(url) == Some(Dialect::Postgres),
        "UF_TEST_DATABASE_URL must be a postgres:// URL"
    );
    let name = schema_name();
    {
        let mut c = sqlx::postgres::PgConnection::connect(url).await?;
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {name}")))
            .execute(&mut c)
            .await?;
    }
    let guard = Arc::new(Schema {
        url: url.to_string(),
        name: name.clone(),
    });
    let set = format!("SET search_path TO {name}");
    let pool = AnyPoolOptions::new()
        .max_connections(4)
        .idle_timeout(std::time::Duration::from_secs(5))
        .after_connect(move |conn, _| {
            let set = set.clone();
            Box::pin(async move {
                sqlx::query(sqlx::AssertSqlSafe(set)).execute(conn).await?;
                Ok(())
            })
        });
    let mut store = Store::connect(url, pool, None).await?;
    store.scratch = Some(guard);
    Ok(store)
}
