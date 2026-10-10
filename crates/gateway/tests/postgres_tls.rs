//! TLS to PostgreSQL: `sslmode=require` and `verify-full` in `UF_DATABASE_URL`.
//!
//! `sslmode=require` against a server without TLS must fail (so TLS really is
//! attempted); that needs `UF_TEST_DATABASE_URL`. Against a server with TLS it
//! needs `UF_TEST_TLS_DATABASE_URL` (a URL with no query) and
//! `UF_TEST_TLS_CA` (the file with the server's certificate, which is
//! self-signed for `localhost`); CI has no such server and skips those.

use ultrafast_gateway::store::Store;

fn with(url: &str, query: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}{query}")
}

fn message(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tokio::test]
async fn require_fails_against_a_server_without_tls() {
    let Ok(base) = std::env::var("UF_TEST_DATABASE_URL") else {
        eprintln!("SKIPPED without UF_TEST_DATABASE_URL: no PostgreSQL to connect to");
        return;
    };
    let err = Store::connect_url(&with(&base, "sslmode=require"), 1)
        .await
        .err()
        .expect("a server without TLS refuses sslmode=require");
    let text = message(err).to_lowercase();
    assert!(
        text.contains("tls") || text.contains("ssl"),
        "unexpected: {text}"
    );
}

#[tokio::test]
async fn require_and_verify_full_connect_to_a_server_with_tls() {
    let (Ok(base), Ok(ca)) = (
        std::env::var("UF_TEST_TLS_DATABASE_URL"),
        std::env::var("UF_TEST_TLS_CA"),
    ) else {
        eprintln!("SKIPPED without UF_TEST_TLS_DATABASE_URL and UF_TEST_TLS_CA: no TLS server");
        return;
    };
    // Encrypted, certificate not checked.
    let store = Store::connect_url(&with(&base, "sslmode=require"), 2)
        .await
        .expect("sslmode=require connects");
    store.close().await;
    // Encrypted and checked against the given root: the name is localhost.
    let url = with(&base, &format!("sslmode=verify-full&sslrootcert={ca}"));
    let store = Store::connect_url(&url, 2)
        .await
        .expect("verify-full with the server's certificate connects");
    store.close().await;
    // The same without the root: a self-signed certificate is not trusted.
    let err = Store::connect_url(&with(&base, "sslmode=verify-full"), 1)
        .await
        .err()
        .expect("an unknown certificate authority is refused");
    let text = message(err).to_lowercase();
    assert!(
        text.contains("certificate") || text.contains("unknown") || text.contains("tls"),
        "unexpected: {text}"
    );
    // A URL with sslmode=disable still works against the same server.
    let store = Store::connect_url(&with(&base, "sslmode=disable"), 1).await;
    // (pg_hba of the stock image allows both; this only shows the option is read.)
    if let Ok(s) = store {
        s.close().await;
    }
}
