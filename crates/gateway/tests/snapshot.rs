//! `/v1` is served from the in-memory snapshot, never from the database.

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use common::{api_on, harness, org, post_chat, Org, Signed};
use serde_json::{json, Value};
use ultrafast_gateway::app::{router, spawn_refresher, AppState};
use ultrafast_gateway::secrets::{generate_key, hash_key, Cipher};
use ultrafast_gateway::snapshot::{SnapProvider, Snapshot};
use ultrafast_gateway::store::{after, now, Store};
use ultrafast_translate::provider::ProviderKind;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CHAT: &str = r#"{"model":"p/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;

fn chat_for(provider: &str) -> String {
    json!({
        "model": format!("{provider}/m"),
        "messages": [{ "role": "user", "content": "hi" }]
    })
    .to_string()
}

fn openai_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "gpt-4o",
        "choices": [{
            "message": { "role": "assistant", "content": "hello" },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
    }))
}

/// A provider that answers every call.
async fn upstream() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(openai_ok())
        .mount(&server)
        .await;
    server
}

/// The organization, its admin signed in, and a provider "p" made through
/// the API.
struct World {
    org: Org,
    maya: Signed,
    upstream: MockServer,
    provider_id: i64,
}

async fn add_provider(org: &Org, maya: &Signed, name: &str, url: &str, key: &str) -> i64 {
    let body = json!({ "name": name, "kind": "openai", "base_url": url, "api_key": key });
    let (status, body) = org
        .call(Some(maya), "POST", "/api/providers", Some(body))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_i64().unwrap()
}

async fn world() -> World {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let upstream = upstream().await;
    let provider_id = add_provider(&org, &maya, "p", &upstream.uri(), "provider-secret").await;
    World {
        org,
        maya,
        upstream,
        provider_id,
    }
}

impl World {
    /// Creates a key through the API. Returns its id and the key itself.
    async fn key_for(&self, owner_id: i64) -> (i64, String) {
        let body = json!({ "name": "k", "owner_id": owner_id });
        let (status, body) = self
            .org
            .call(Some(&self.maya), "POST", "/api/keys", Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        (
            body["key"]["id"].as_i64().unwrap(),
            body["secret"].as_str().unwrap().to_string(),
        )
    }

    async fn chat(&self, key: &str, body: &str) -> StatusCode {
        post_chat(&self.org.api.app, Some(key), body).await.0
    }

    async fn admin(&self, method: &str, path: &str, body: Option<Value>) -> StatusCode {
        self.org.call(Some(&self.maya), method, path, body).await.0
    }
}

#[tokio::test]
async fn v1_does_not_touch_the_database() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(openai_ok())
        .expect(1)
        .mount(&h.upstream)
        .await;
    h.store.pool().close().await;
    let (status, body) = post_chat(&h.app, Some(&h.key), CHAT).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn a_new_key_works_at_once() {
    let w = world().await;
    let (_, secret) = w.key_for(w.org.maya).await;
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);
}

#[tokio::test]
async fn a_revoked_key_stops_at_once() {
    let w = world().await;
    let (id, secret) = w.key_for(w.org.maya).await;
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);
    let status = w.admin("DELETE", &format!("/api/keys/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_expiring_key_stops_without_refresh() {
    let w = world().await;
    let key = generate_key();
    w.org
        .api
        .store
        .insert_key("short", &key.hash, &key.display, Some(&after(2)))
        .await
        .unwrap();
    w.org.api.state.refresh().await.unwrap();
    assert_eq!(w.chat(&key.full, CHAT).await, StatusCode::OK);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(w.chat(&key.full, CHAT).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_key_is_dead_at_the_second_it_expires() {
    let w = world().await;
    let key = generate_key();
    w.org
        .api
        .store
        .insert_key("k", &key.hash, &key.display, Some("2030-01-01 00:00:00"))
        .await
        .unwrap();
    w.org.api.state.refresh().await.unwrap();
    let snapshot = w.org.api.state.snapshot.load_full();
    assert!(snapshot.key(&key.hash, "2029-12-31 23:59:59").is_some());
    assert!(snapshot.key(&key.hash, "2030-01-01 00:00:00").is_none());
    assert!(snapshot.key(&key.hash, "2030-01-01 00:00:01").is_none());
    assert!(snapshot
        .key("no such hash", "2029-01-01 00:00:00")
        .is_none());
}

#[tokio::test]
async fn a_disabled_owner_stops_their_keys() {
    let w = world().await;
    let (_, secret) = w.key_for(w.org.lena).await;
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);

    let lena = format!("/api/users/{}", w.org.lena);
    let status = w
        .admin("PATCH", &lena, Some(json!({ "status": "disabled" })))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::UNAUTHORIZED);

    let status = w
        .admin("PATCH", &lena, Some(json!({ "status": "active" })))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);
}

#[tokio::test]
async fn the_key_of_an_invited_user_works_once_they_accept() {
    let w = world().await;
    let invite = json!({ "email": "nora@example.com", "name": "Nora", "role": "member" });
    let (status, body) = w
        .org
        .call(Some(&w.maya), "POST", "/api/users", Some(invite))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let nora = body["user"]["id"].as_i64().unwrap();
    let link = body["invite_link"].as_str().unwrap();
    let token = &link[link.find("uf-inv-").unwrap()..];

    // A key of theirs, as the CLI or an earlier version could have left it.
    let key = generate_key();
    let store = &w.org.api.store;
    let mut tx = store.begin().await.unwrap();
    tx.insert_key("k", &key.hash, &key.display, None, Some(nora), None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    w.org.api.state.refresh().await.unwrap();
    assert_eq!(w.chat(&key.full, CHAT).await, StatusCode::UNAUTHORIZED);

    let accept = json!({ "token": token, "password": "a long enough password" });
    let (status, body) = w
        .org
        .call(None, "POST", "/api/auth/accept-invite", Some(accept))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(w.chat(&key.full, CHAT).await, StatusCode::OK);
}

#[tokio::test]
async fn deleting_the_owner_keeps_the_key_working() {
    let w = world().await;
    let (id, secret) = w.key_for(w.org.lena).await;
    let status = w
        .admin("DELETE", &format!("/api/users/{}", w.org.lena), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);

    let snapshot = w.org.api.state.snapshot.load_full();
    let key = snapshot.key(&hash_key(&secret), &now()).unwrap();
    assert_eq!(key.id, id);
    assert_eq!(key.user_id, None);
}

#[tokio::test]
async fn a_new_provider_works_at_once() {
    let w = world().await;
    let (_, secret) = w.key_for(w.org.maya).await;
    assert_eq!(
        w.chat(&secret, &chat_for("fresh")).await,
        StatusCode::NOT_FOUND
    );
    add_provider(&w.org, &w.maya, "fresh", &w.upstream.uri(), "sk-1").await;
    assert_eq!(w.chat(&secret, &chat_for("fresh")).await, StatusCode::OK);
}

#[tokio::test]
async fn a_changed_credential_is_used_at_once() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer old-secret"))
        .respond_with(openai_ok())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer new-secret"))
        .respond_with(openai_ok())
        .expect(1)
        .mount(&server)
        .await;
    let provider_id = add_provider(&org, &maya, "p", &server.uri(), "old-secret").await;
    let w = World {
        org,
        maya,
        upstream: server,
        provider_id,
    };
    let (_, secret) = w.key_for(w.org.maya).await;
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);

    let path = format!("/api/providers/{}", w.provider_id);
    let status = w
        .admin("PATCH", &path, Some(json!({ "api_key": "new-secret" })))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);
    // The expectations are checked when the server is dropped.
}

#[tokio::test]
async fn a_deleted_provider_is_404() {
    let w = world().await;
    let (_, secret) = w.key_for(w.org.maya).await;
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);
    let path = format!("/api/providers/{}", w.provider_id);
    assert_eq!(w.admin("DELETE", &path, None).await, StatusCode::NO_CONTENT);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_undecryptable_provider_is_skipped() {
    let w = world().await;
    let (_, secret) = w.key_for(w.org.maya).await;
    let api = &w.org.api;
    let url = w.upstream.uri();
    api.store
        .insert_provider("garbage", "openai", &url, Some(&[7u8; 40]))
        .await
        .unwrap();
    let not_text = api.state.cipher.encrypt(&[0xff, 0xfe, 0xfd]);
    api.store
        .insert_provider("nottext", "openai", &url, Some(&not_text))
        .await
        .unwrap();
    let fine = api.state.cipher.encrypt(b"sk-1");
    api.store
        .insert_provider("oddkind", "telepathy", &url, Some(&fine))
        .await
        .unwrap();
    api.store
        .insert_provider("open", "openai", &url, None)
        .await
        .unwrap();

    api.state.refresh().await.unwrap();

    for skipped in ["garbage", "nottext", "oddkind"] {
        let status = w.chat(&secret, &chat_for(skipped)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{skipped}");
    }
    assert_eq!(w.chat(&secret, &chat_for("open")).await, StatusCode::OK);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);
}

#[tokio::test]
async fn snapshot_debug_hides_credentials() {
    let provider = SnapProvider {
        id: 1,
        name: "openai".into(),
        kind: ProviderKind::parse("openai").unwrap(),
        base_url: "https://api.openai.com/v1".into(),
        api_key: Some("sk-very-secret".into()),
    };
    let shown = format!("{provider:?}");
    assert!(!shown.contains("sk-very-secret"), "{shown}");
    assert!(!shown.contains("very"), "{shown}");
    assert!(shown.contains("openai"));
    assert!(shown.contains("<redacted>"));

    let without = SnapProvider {
        api_key: None,
        ..provider
    };
    assert!(format!("{without:?}").contains("<none>"));
}

#[tokio::test]
async fn cli_changes_appear_after_refresh() {
    let w = world().await;
    let key = generate_key();
    w.org
        .api
        .store
        .insert_key("cli", &key.hash, &key.display, None)
        .await
        .unwrap();
    assert_eq!(w.chat(&key.full, CHAT).await, StatusCode::UNAUTHORIZED);
    w.org.api.state.refresh().await.unwrap();
    assert_eq!(w.chat(&key.full, CHAT).await, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_refreshes_end_equal_to_the_database() {
    // A database on disk, so writes and reads really run side by side.
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("test.db")).await.unwrap();
    let api = api_on(store, false).await;

    let mut tasks = Vec::new();
    for i in 0..24 {
        let state = api.state.clone();
        tasks.push(tokio::spawn(async move {
            let kept = generate_key();
            let dropped = generate_key();
            let store = &state.store;
            store
                .insert_key("kept", &kept.hash, &kept.display, None)
                .await
                .unwrap();
            let id = store
                .insert_key("dropped", &dropped.hash, &dropped.display, None)
                .await
                .unwrap();
            state.refresh().await.unwrap();
            store.revoke_key(id).await.unwrap();
            store
                .insert_provider(&format!("p{i}"), "openai", "http://127.0.0.1:9", None)
                .await
                .unwrap();
            state.refresh().await.unwrap();
            (kept.hash, dropped.hash)
        }));
    }
    let mut hashes = Vec::new();
    for task in tasks {
        hashes.push(task.await.unwrap());
    }

    let held = api.state.snapshot.load_full();
    let fresh = Snapshot::load(&api.store, &api.state.cipher).await.unwrap();
    let now = now();
    assert_eq!(held.key_count(), 24);
    assert_eq!(held.key_count(), fresh.key_count());
    assert_eq!(held.provider_count(), 24);
    assert_eq!(held.provider_count(), fresh.provider_count());
    for (i, (kept, dropped)) in hashes.iter().enumerate() {
        assert!(held.key(kept, &now).is_some());
        assert!(held.key(dropped, &now).is_none());
        assert!(held.provider(&format!("p{i}")).is_some());
    }
}

#[tokio::test]
async fn the_background_task_picks_up_direct_changes_and_stops() {
    let server = upstream().await;
    let store = Store::open_in_memory().await.unwrap();
    let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
    let mut state = AppState::new(store.clone(), cipher).await.unwrap();
    state.refresh_interval = Duration::from_millis(50);
    let state = Arc::new(state);
    let app = router(state.clone());
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let task = spawn_refresher(state.clone(), stopped);

    // Written as the CLI would, with no API call.
    let key = generate_key();
    store
        .insert_key("cli", &key.hash, &key.display, None)
        .await
        .unwrap();
    store
        .insert_provider("p", "openai", &server.uri(), None)
        .await
        .unwrap();

    let mut status = StatusCode::UNAUTHORIZED;
    for _ in 0..100 {
        status = post_chat(&app, Some(&key.full), CHAT).await.0;
        if status == StatusCode::OK {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, StatusCode::OK);

    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("the task stops when told to")
        .unwrap();
}

#[tokio::test]
async fn the_background_task_survives_a_failed_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.db");
    let store = Store::open(&file).await.unwrap();
    let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
    let mut state = AppState::new(store.clone(), cipher).await.unwrap();
    state.refresh_interval = Duration::from_millis(50);
    let state = Arc::new(state);
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let task = spawn_refresher(state.clone(), stopped);

    // While the table is away every refresh fails.
    sqlx::query("ALTER TABLE providers RENAME TO providers_away")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(state.refresh().await.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!task.is_finished());
    sqlx::query("ALTER TABLE providers_away RENAME TO providers")
        .execute(store.pool())
        .await
        .unwrap();

    let key = generate_key();
    store
        .insert_key("k", &key.hash, &key.display, None)
        .await
        .unwrap();
    let mut seen = false;
    for _ in 0..100 {
        seen = state.snapshot.load().key(&key.hash, &now()).is_some();
        if seen {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(seen, "the task refreshed again after the failures");

    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("the task stops when told to")
        .unwrap();
}
