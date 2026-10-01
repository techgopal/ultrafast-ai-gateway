//! `/v1` is served from the in-memory snapshot, never from the database.

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use common::{
    allow_model, api_on, call, error_code, harness, org, post_chat, seed_user, sign_in, Api, Org,
    Signed,
};
use serde_json::{json, Value};
use ultrafast_gateway::api::refresh_snapshot;
use ultrafast_gateway::app::{router, spawn_refresher, AppState};
use ultrafast_gateway::identity::Role;
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
    allow_model(&org.api.store, "p", "gpt-4o").await;
    org.api.state.refresh().await.unwrap();
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
    h.store.close().await;
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
    allow_model(&w.org.api.store, "fresh", "m").await;
    w.org.api.state.refresh().await.unwrap();
    assert_eq!(w.chat(&secret, &chat_for("fresh")).await, StatusCode::OK);
}

#[tokio::test]
async fn model_switches_and_grants_work_at_once() {
    let w = world().await;
    let (_, secret) = w.key_for(w.org.lena).await;
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);
    let model_id = w.org.api.store.list_models().await.unwrap()[0].id;

    let model = format!("/api/models/{model_id}");
    let off = json!({ "enabled": false });
    assert_eq!(w.admin("PATCH", &model, Some(off)).await, StatusCode::OK);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::FORBIDDEN);
    let on = json!({ "enabled": true });
    assert_eq!(w.admin("PATCH", &model, Some(on)).await, StatusCode::OK);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);

    // Granted to Research only: Lena is in Platform.
    let grants = format!("{model}/grants");
    let research = json!({
        "everyone": false, "team_ids": [w.org.research], "user_ids": []
    });
    let status = w.admin("PUT", &grants, Some(research)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::FORBIDDEN);
    let hers = json!({ "everyone": false, "team_ids": [], "user_ids": [w.org.lena] });
    assert_eq!(w.admin("PUT", &grants, Some(hers)).await, StatusCode::OK);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);
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
    allow_model(&org.api.store, "p", "gpt-4o").await;
    org.api.state.refresh().await.unwrap();
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
    for name in ["garbage", "nottext", "oddkind", "open"] {
        allow_model(&api.store, name, "m").await;
    }

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
    allow_model(&store, "p", "gpt-4o").await;

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

/// Runs SQL on the database file through a connection of its own.
async fn raw_sql(file: &std::path::Path, sql: &str) {
    use sqlx::Connection;
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(file);
    let mut conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();
}

// While the providers table is away every refresh fails.
const BREAK: &str = "ALTER TABLE providers RENAME TO providers_away";
const MEND: &str = "ALTER TABLE providers_away RENAME TO providers";

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

    raw_sql(&file, BREAK).await;
    assert!(state.refresh().await.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!task.is_finished());
    raw_sql(&file, MEND).await;

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

/// The `id_hash` of every session row, read through a connection of its own.
async fn session_hashes(file: &std::path::Path) -> Vec<String> {
    use sqlx::{Connection, Row};
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(file);
    let mut conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    let rows = sqlx::query("SELECT id_hash FROM sessions ORDER BY id_hash")
        .fetch_all(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();
    rows.iter().map(|r| r.get("id_hash")).collect()
}

#[tokio::test]
async fn the_background_task_deletes_expired_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.db");
    let store = Store::open(&file).await.unwrap();
    let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
    let user = seed_user(
        &store,
        "maya@example.com",
        Role::Admin,
        "correct horse battery",
    )
    .await;
    let live = store.create_session(user).await.unwrap();
    let insert = format!(
        "INSERT INTO sessions (user_id, id_hash, csrf_token, expires_at)
         VALUES ({user}, 'expired-row', 'c', '{}')",
        after(-1)
    );
    raw_sql(&file, &insert).await;
    assert_eq!(session_hashes(&file).await.len(), 2);

    let mut state = AppState::new(store.clone(), cipher).await.unwrap();
    state.refresh_interval = Duration::from_millis(50);
    let state = Arc::new(state);
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let task = spawn_refresher(state.clone(), stopped);

    let mut left = Vec::new();
    for _ in 0..100 {
        left = session_hashes(&file).await;
        if left.len() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(left, [hash_key(&live.id)]);
    assert!(store.live_session(&live.id).await.unwrap().is_some());

    // While the sessions table is away the cleanup fails; the task goes on
    // and still refreshes the snapshot.
    raw_sql(&file, "ALTER TABLE sessions RENAME TO sessions_away").await;
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
    assert!(seen, "the snapshot is refreshed while the cleanup fails");
    assert!(!task.is_finished());
    raw_sql(&file, "ALTER TABLE sessions_away RENAME TO sessions").await;

    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("the task stops when told to")
        .unwrap();
}

async fn revoked_in_database(w: &World, id: i64) -> bool {
    let key = w.org.api.store.key_by_id(id).await.unwrap().unwrap();
    key.revoked_at.is_some()
}

#[tokio::test]
async fn deleting_a_disabled_user_revokes_their_keys() {
    let w = world().await;
    let (first, secret) = w.key_for(w.org.lena).await;
    let (second, other) = w.key_for(w.org.lena).await;
    let (tomas_key, tomas_secret) = w.key_for(w.org.tomas).await;
    let lena = format!("/api/users/{}", w.org.lena);
    let status = w
        .admin("PATCH", &lena, Some(json!({ "status": "disabled" })))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::UNAUTHORIZED);
    assert!(!revoked_in_database(&w, first).await);

    assert_eq!(w.admin("DELETE", &lena, None).await, StatusCode::NO_CONTENT);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::UNAUTHORIZED);
    assert_eq!(w.chat(&other, CHAT).await, StatusCode::UNAUTHORIZED);
    assert!(revoked_in_database(&w, first).await);
    assert!(revoked_in_database(&w, second).await);
    assert_eq!(
        w.org.last_summary("user.delete").await,
        "Deleted user lena@example.com, revoked 2 keys"
    );

    // The keys of others are left alone.
    assert!(!revoked_in_database(&w, tomas_key).await);
    assert_eq!(w.chat(&tomas_secret, CHAT).await, StatusCode::OK);
}

#[tokio::test]
async fn deleting_an_invited_user_revokes_their_keys() {
    let w = world().await;
    let invite = json!({ "email": "nora@example.com", "name": "Nora", "role": "member" });
    let (status, body) = w
        .org
        .call(Some(&w.maya), "POST", "/api/users", Some(invite))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let nora = body["user"]["id"].as_i64().unwrap();
    let key = generate_key();
    let mut tx = w.org.api.store.begin().await.unwrap();
    let id = tx
        .insert_key("k", &key.hash, &key.display, None, Some(nora), None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    w.org.api.state.refresh().await.unwrap();
    assert_eq!(w.chat(&key.full, CHAT).await, StatusCode::UNAUTHORIZED);

    let status = w.admin("DELETE", &format!("/api/users/{nora}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(w.chat(&key.full, CHAT).await, StatusCode::UNAUTHORIZED);
    assert!(revoked_in_database(&w, id).await);
    assert_eq!(
        w.org.last_summary("user.delete").await,
        "Deleted user nora@example.com, revoked 1 key"
    );
}

#[tokio::test]
async fn deleting_an_active_user_revokes_nothing() {
    let w = world().await;
    let (id, secret) = w.key_for(w.org.lena).await;
    let (other, other_secret) = w.key_for(w.org.lena).await;
    // A key that is already revoked is not one that was left working.
    let (gone, _) = w.key_for(w.org.lena).await;
    let status = w.admin("DELETE", &format!("/api/keys/{gone}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let status = w
        .admin("DELETE", &format!("/api/users/{}", w.org.lena), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);
    assert_eq!(w.chat(&other_secret, CHAT).await, StatusCode::OK);
    assert!(!revoked_in_database(&w, id).await);
    assert!(!revoked_in_database(&w, other).await);
    assert_eq!(
        w.org.last_summary("user.delete").await,
        "Deleted user lena@example.com, left 2 keys working without an owner"
    );
}

#[tokio::test]
async fn deleting_an_active_user_without_keys_mentions_none() {
    let w = world().await;
    let status = w
        .admin("DELETE", &format!("/api/users/{}", w.org.tomas), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        w.org.last_summary("user.delete").await,
        "Deleted user tomas@example.com"
    );
}

#[tokio::test]
async fn revoking_a_revoked_key_still_refreshes() {
    let w = world().await;
    let (id, secret) = w.key_for(w.org.maya).await;
    // As after a revoke whose refresh failed: committed, not in the snapshot.
    assert!(w.org.api.store.revoke_key(id).await.unwrap());
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);

    let status = w.admin("DELETE", &format!("/api/keys/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn deleting_a_deleted_provider_still_refreshes() {
    let w = world().await;
    let (_, secret) = w.key_for(w.org.maya).await;
    let mut tx = w.org.api.store.begin().await.unwrap();
    assert!(tx.delete_provider(w.provider_id).await.unwrap());
    tx.commit().await.unwrap();
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);

    let path = format!("/api/providers/{}", w.provider_id);
    assert_eq!(w.admin("DELETE", &path, None).await, StatusCode::NOT_FOUND);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_provider_update_that_changes_nothing_still_refreshes() {
    let w = world().await;
    let (_, secret) = w.key_for(w.org.maya).await;
    let server = upstream().await;
    // As after an update whose refresh failed.
    let mut tx = w.org.api.store.begin().await.unwrap();
    tx.update_provider(w.provider_id, Some("http://127.0.0.1:9"), None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    drop(server);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::OK);

    let path = format!("/api/providers/{}", w.provider_id);
    let same = json!({ "base_url": "http://127.0.0.1:9" });
    assert_eq!(w.admin("PATCH", &path, Some(same)).await, StatusCode::OK);
    assert_eq!(w.chat(&secret, CHAT).await, StatusCode::BAD_GATEWAY);
}

async fn in_snapshot(api: &Api, hash: &str) -> bool {
    for _ in 0..100 {
        if api.state.snapshot.load().key(hash, &now()).is_some() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test]
async fn a_refresh_finishes_when_its_caller_goes_away() {
    let w = world().await;
    let api = &w.org.api;
    let key = generate_key();
    // The open transaction holds the only connection, so the refresh waits.
    let mut tx = api.store.begin().await.unwrap();
    tx.insert_key("k", &key.hash, &key.display, None, None, None)
        .await
        .unwrap();

    // Polled and dropped, as the handler of a caller who disconnects.
    let waited =
        tokio::time::timeout(Duration::from_millis(50), refresh_snapshot(&api.state)).await;
    assert!(waited.is_err(), "the refresh was not finished when dropped");
    tx.commit().await.unwrap();
    assert!(in_snapshot(api, &key.hash).await);
}

#[tokio::test]
async fn a_failed_refresh_is_500_and_the_change_stays() {
    const EMAIL: &str = "maya@example.com";
    const PASSWORD: &str = "correct horse battery";
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.db");
    let api = api_on(Store::open(&file).await.unwrap(), false).await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let maya = sign_in(&api.app, EMAIL, PASSWORD).await;
    let server = upstream().await;
    let provider = json!({ "name": "p", "kind": "openai", "base_url": server.uri() });
    let (status, _, _) = call(
        &api.app,
        "POST",
        "/api/providers",
        Some(&maya),
        Some(provider),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    allow_model(&api.store, "p", "gpt-4o").await;
    let new_key = json!({ "name": "k" });
    let (status, _, body) = call(&api.app, "POST", "/api/keys", Some(&maya), Some(new_key)).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["key"]["id"].as_i64().unwrap();
    let secret = body["secret"].as_str().unwrap().to_string();
    assert_eq!(
        post_chat(&api.app, Some(&secret), CHAT).await.0,
        StatusCode::OK
    );

    raw_sql(&file, BREAK).await;
    let path = format!("/api/keys/{id}");
    let (status, _, body) = call(&api.app, "DELETE", &path, Some(&maya), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(error_code(&body), "internal_error");
    assert_eq!(
        body,
        json!({ "error": { "code": "internal_error", "message": "Something went wrong." } })
    );
    // Committed, with its audit entry, though `/v1` does not know yet.
    let key = api.store.key_by_id(id).await.unwrap().unwrap();
    assert!(key.revoked_at.is_some());
    let audit = api.store.list_audit(10, None).await.unwrap();
    assert_eq!(audit[0].action, "key.revoke");
    assert_eq!(
        post_chat(&api.app, Some(&secret), CHAT).await.0,
        StatusCode::OK
    );

    raw_sql(&file, MEND).await;
    // The next write that refreshes carries the change along.
    let other = json!({ "name": "other" });
    let (status, _, _) = call(&api.app, "POST", "/api/keys", Some(&maya), Some(other)).await;
    assert_eq!(status, StatusCode::CREATED);
    let status = post_chat(&api.app, Some(&secret), CHAT).await.0;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
