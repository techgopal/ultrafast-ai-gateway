//! The MCP catalog: fetched, verified, cached, with a built-in fallback and an
//! off switch that makes no request at all.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use time::OffsetDateTime;
use ultrafast_gateway::mcp::catalog::{self, Catalog, Config, Source};
use ultrafast_gateway::store::Store;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn doc(ids: &[&str]) -> String {
    let entries: Vec<_> = ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id, "name": id, "description": "d", "homepage": "https://x.example",
                "transport": "http", "url": format!("https://{id}.example/mcp"),
                "env": [], "auth": "none", "tags": []
            })
        })
        .collect();
    serde_json::json!({"schema": 1, "updated": "2999-01-01", "entries": entries}).to_string()
}

fn sign(body: &str) -> String {
    STANDARD.encode(key().sign(body.as_bytes()).to_bytes())
}

async fn serve(server: &MockServer, body: &str, sig: &str, cache_control: Option<&str>) {
    let mut ok = ResponseTemplate::new(200).set_body_string(body.to_string());
    if let Some(c) = cache_control {
        ok = ok.insert_header("cache-control", c);
    }
    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/cat/v1.json"))
        .respond_with(ok)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/cat/v1.json.sig"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sig.to_string()))
        .mount(server)
        .await;
}

async fn service(server: &MockServer, off: bool) -> Catalog {
    let store = Store::open_in_memory().await.unwrap();
    Catalog::new(
        store,
        reqwest::Client::new(),
        Config {
            off,
            url: format!("{}/cat/v1.json", server.uri()),
            refresh: Duration::from_secs(86_400),
        },
        key().verifying_key(),
    )
}

fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()
}

fn ids(view: &catalog::View) -> Vec<&str> {
    view.entries.iter().map(|e| e.id.as_str()).collect()
}

#[tokio::test]
async fn a_valid_catalog_is_remote_and_then_cached_across_a_restart() {
    let server = MockServer::start().await;
    let body = doc(&["one", "two"]);
    serve(&server, &body, &sign(&body), None).await;
    let c = service(&server, false).await;
    // Before any fetch the copy in the binary is shown.
    assert_eq!(c.view().await.source, Source::Builtin);
    c.refresh(now()).await;
    let view = c.view().await;
    assert_eq!(view.source, Source::Remote);
    assert_eq!(ids(&view), ["one", "two"]);
    assert!(view.error.is_none());
    assert!(view.fetched_at.unwrap().starts_with("2027-"));
    // A new process on the same database starts from the cache.
    let again = Catalog::new(
        c_store(&c),
        reqwest::Client::new(),
        Config::default(),
        key().verifying_key(),
    );
    let view = again.view().await;
    assert_eq!(view.source, Source::Cache);
    assert_eq!(ids(&view), ["one", "two"]);
}

fn c_store(c: &Catalog) -> Store {
    c.store_for_tests()
}

#[tokio::test]
async fn a_tampered_catalog_is_rejected_and_the_cached_copy_stays() {
    let server = MockServer::start().await;
    let good = doc(&["one"]);
    serve(&server, &good, &sign(&good), None).await;
    let c = service(&server, false).await;
    c.refresh(now()).await;
    // One byte changed after signing.
    let tampered = doc(&["onf"]);
    serve(&server, &tampered, &sign(&good), None).await;
    c.refresh(now() + time::Duration::days(2)).await;
    let view = c.view().await;
    assert_eq!(view.source, Source::Cache);
    assert_eq!(ids(&view), ["one"]);
    let error = view.error.unwrap();
    assert!(
        error.starts_with("catalog signature invalid, showing the copy from 2027-"),
        "{error}"
    );
}

#[tokio::test]
async fn a_tampered_catalog_with_no_cache_falls_back_to_the_builtin_copy() {
    let server = MockServer::start().await;
    let body = doc(&["evil"]);
    serve(&server, &body, &sign("something else"), None).await;
    let c = service(&server, false).await;
    c.refresh(now()).await;
    let view = c.view().await;
    assert_eq!(view.source, Source::Builtin);
    assert!(!ids(&view).contains(&"evil"));
    assert!(ids(&view).contains(&"github"));
    assert_eq!(
        view.error.as_deref(),
        Some("catalog signature invalid, showing the built-in copy")
    );
}

#[tokio::test]
async fn a_catalog_that_cannot_be_fetched_keeps_the_cache() {
    let server = MockServer::start().await;
    let good = doc(&["one"]);
    serve(&server, &good, &sign(&good), None).await;
    let c = service(&server, false).await;
    c.refresh(now()).await;
    server.reset().await; // every path now answers 404
    c.refresh(now() + time::Duration::days(2)).await;
    let view = c.view().await;
    assert_eq!(ids(&view), ["one"]);
    assert!(view
        .error
        .unwrap()
        .starts_with("could not fetch the catalog, showing the copy from"));
}

#[tokio::test]
async fn a_catalog_over_the_size_cap_is_refused() {
    let server = MockServer::start().await;
    let big = doc(&["one"]) + &" ".repeat(catalog::MAX_BYTES);
    serve(&server, &big, &sign(&big), None).await;
    let c = service(&server, false).await;
    c.refresh(now()).await;
    assert_eq!(c.view().await.source, Source::Builtin);
    assert!(c.view().await.error.is_some());
}

#[tokio::test]
async fn off_makes_no_request_at_all() {
    let server = MockServer::start().await;
    let body = doc(&["one"]);
    serve(&server, &body, &sign(&body), None).await;
    let c = service(&server, true).await;
    c.refresh(now()).await;
    c.refresh_if_due(now()).await;
    c.refresh_if_due(now() + time::Duration::days(30)).await;
    let view = c.view().await;
    assert_eq!(view.source, Source::Builtin);
    assert!(view.error.is_none());
    assert!(c.is_off());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn max_age_decides_when_the_catalog_is_fetched_again() {
    let server = MockServer::start().await;
    let body = doc(&["one"]);
    serve(&server, &body, &sign(&body), Some("public, max-age=3600")).await;
    let c = service(&server, false).await;
    async fn count(s: &MockServer) -> usize {
        s.received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == "/cat/v1.json")
            .count()
    }
    c.refresh_if_due(now()).await;
    assert_eq!(count(&server).await, 1);
    c.refresh_if_due(now() + time::Duration::seconds(1800))
        .await;
    assert_eq!(count(&server).await, 1);
    c.refresh_if_due(now() + time::Duration::seconds(3601))
        .await;
    assert_eq!(count(&server).await, 2);
}

#[tokio::test]
async fn a_second_process_does_not_fetch_what_the_first_just_got() {
    let server = MockServer::start().await;
    let body = doc(&["one"]);
    serve(&server, &body, &sign(&body), Some("max-age=3600")).await;
    let first = service(&server, false).await;
    first.refresh_if_due(now()).await;
    let second = Catalog::new(
        first.store_for_tests(),
        reqwest::Client::new(),
        Config {
            off: false,
            url: format!("{}/cat/v1.json", server.uri()),
            refresh: Duration::from_secs(86_400),
        },
        key().verifying_key(),
    );
    second
        .refresh_if_due(now() + time::Duration::seconds(10))
        .await;
    let n = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/cat/v1.json")
        .count();
    assert_eq!(n, 1);
}
