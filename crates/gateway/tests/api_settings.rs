//! `GET` and `PATCH /api/settings`.

mod common;

use axum::http::StatusCode;
use common::{call, error_code, org};
use serde_json::{json, Value};

#[tokio::test]
async fn an_admin_reads_and_changes_the_retention() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = org.call(Some(&maya), "GET", "/api/settings", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["log_retention_days"], 30);

    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            "/api/settings",
            Some(json!({ "log_retention_days": 90 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["log_retention_days"], 90);
    assert_eq!(org.api.store.log_retention_days().await.unwrap(), 90);
    let (_, body) = org.call(Some(&maya), "GET", "/api/settings", None).await;
    assert_eq!(body["log_retention_days"], 90);
    assert_eq!(
        org.last_summary("settings.update").await,
        "Set log retention to 90 days"
    );

    // The bounds are 1 and 3650 days.
    for days in [1, 3650] {
        let (status, _) = org
            .call(
                Some(&maya),
                "PATCH",
                "/api/settings",
                Some(json!({ "log_retention_days": days })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{days}");
    }
    for days in [0, -1, 3651, 100_000] {
        let (status, body) = org
            .call(
                Some(&maya),
                "PATCH",
                "/api/settings",
                Some(json!({ "log_retention_days": days })),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{days}: {body}");
        assert_eq!(error_code(&body), "validation_failed");
        assert!(body["error"]["fields"]["log_retention_days"].is_string());
    }
    assert_eq!(org.api.store.log_retention_days().await.unwrap(), 3650);

    // Not a number, missing, unknown field.
    for bad in [
        json!({ "log_retention_days": "30" }),
        json!({ "log_retention_days": null }),
        json!({}),
        json!({ "log_retention_days": 30, "other": 1 }),
    ] {
        let (status, _) = org
            .call(Some(&maya), "PATCH", "/api/settings", Some(bad.clone()))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(org.api.store.log_retention_days().await.unwrap(), 3650);
}

#[tokio::test]
async fn only_an_admin_sees_or_changes_settings() {
    let org = org().await;
    for who in ["arjun", "lena"] {
        let me = org.sign_in(who).await;
        let (status, body) = org.call(Some(&me), "GET", "/api/settings", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{who}: {body}");
        assert_eq!(error_code(&body), "forbidden");
        let (status, body) = org
            .call(
                Some(&me),
                "PATCH",
                "/api/settings",
                Some(json!({ "log_retention_days": 1 })),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{who}: {body}");
    }
    assert_eq!(org.api.store.log_retention_days().await.unwrap(), 30);
    let (status, body) = org.call(None, "GET", "/api/settings", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "unauthenticated");
    let _: Value = body;
}

/// What the settings say the gateway runs on.
fn database_of(org: &common::Org) -> &'static str {
    match org.api.store.dialect() {
        ultrafast_gateway::store::Dialect::Sqlite => "sqlite",
        ultrafast_gateway::store::Dialect::Postgres => "postgres",
    }
}

fn login_limits() -> Value {
    json!({ "window_minutes": 15, "max_per_email": 5, "max_per_address": 20 })
}

#[tokio::test]
async fn the_sign_in_settings_are_shown_and_the_limits_are_read_only() {
    let org = common::org_behind(&["10.0.0.0/8", "fd00::/8"]).await;
    let maya = org.sign_in("maya").await;
    let (status, body) = org.call(Some(&maya), "GET", "/api/settings", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({
            "log_retention_days": 30,
            "session_hours": 12,
            "trusted_proxies": ["10.0.0.0/8", "fd00::/8"],
            "login_limits": login_limits(),
            "database": database_of(&org),
        })
    );
    // Nothing of the read-only parts can be sent.
    for bad in [
        json!({ "trusted_proxies": [] }),
        json!({ "login_limits": login_limits() }),
        json!({ "session_hours": 12, "trusted_proxies": ["10.0.0.0/8"] }),
    ] {
        let (status, _) = org
            .call(Some(&maya), "PATCH", "/api/settings", Some(bad.clone()))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    // No proxy trusted: an empty list.
    let org = org_without_proxies().await;
    let maya = org.sign_in("maya").await;
    let (_, body) = org.call(Some(&maya), "GET", "/api/settings", None).await;
    assert_eq!(body["trusted_proxies"], json!([]));
}

async fn org_without_proxies() -> common::Org {
    common::org().await
}

#[tokio::test]
async fn session_hours_are_set_one_to_720_and_each_field_alone() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            "/api/settings",
            Some(json!({ "session_hours": 24 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["session_hours"], 24);
    // Retention is left as it was.
    assert_eq!(body["log_retention_days"], 30);
    assert_eq!(
        org.last_summary("settings.update").await,
        "Set session lifetime to 24 hours"
    );
    // Both at once: both are set, each with its audit entry.
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            "/api/settings",
            Some(json!({ "session_hours": 1, "log_retention_days": 7 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (
            body["session_hours"].clone(),
            body["log_retention_days"].clone()
        ),
        (json!(1), json!(7))
    );
    for hours in [1, 720] {
        let (status, _) = org
            .call(
                Some(&maya),
                "PATCH",
                "/api/settings",
                Some(json!({ "session_hours": hours })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{hours}");
    }
    for hours in [0, -1, 721, 100_000] {
        let (status, body) = org
            .call(
                Some(&maya),
                "PATCH",
                "/api/settings",
                Some(json!({ "session_hours": hours })),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{hours}: {body}");
        assert_eq!(
            body["error"]["fields"]["session_hours"],
            "must be from 1 to 720"
        );
    }
    // A value out of range changes nothing, not even the valid field beside it.
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            "/api/settings",
            Some(json!({ "session_hours": 721, "log_retention_days": 99 })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(org.api.store.log_retention_days().await.unwrap(), 7);
    assert_eq!(org.api.store.session_hours().await.unwrap(), 720);
    for bad in [
        json!({ "session_hours": "12" }),
        json!({ "session_hours": null }),
        json!({}),
    ] {
        let (status, _) = org
            .call(Some(&maya), "PATCH", "/api/settings", Some(bad.clone()))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn the_lifetime_applies_to_new_sessions_only() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let before = org
        .api
        .store
        .live_session(maya.cookie.trim_start_matches("uf_session="))
        .await
        .unwrap()
        .unwrap();
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            "/api/settings",
            Some(json!({ "session_hours": 2 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // A new sign-in lives two hours, in the row and in the cookie.
    let (status, headers, _) = call(
        &org.api.app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": common::email_of("lena"), "password": common::ORG_PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers["set-cookie"].to_str().unwrap();
    assert!(cookie.contains("Max-Age=7200"), "{cookie}");
    let value = common::cookie_pair(&headers);
    let session = org
        .api
        .store
        .live_session(value.trim_start_matches("uf_session="))
        .await
        .unwrap()
        .unwrap();
    assert!(session.expires_at >= ultrafast_gateway::store::after(2 * 3600 - 5));
    assert!(session.expires_at <= ultrafast_gateway::store::after(2 * 3600 + 5));
    // The session of before is as it was: 12 hours.
    assert!(before.expires_at >= ultrafast_gateway::store::after(12 * 3600 - 60));
    let again = org
        .api
        .store
        .live_session(maya.cookie.trim_start_matches("uf_session="))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again.expires_at, before.expires_at);
}

#[tokio::test]
async fn the_default_lifetime_is_twelve_hours_in_the_cookie() {
    let org = org().await;
    let (status, headers, _) = call(
        &org.api.app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": common::email_of("lena"), "password": common::ORG_PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers["set-cookie"]
        .to_str()
        .unwrap()
        .contains("Max-Age=43200"));
}
