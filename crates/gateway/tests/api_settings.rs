//! `GET` and `PATCH /api/settings`.

mod common;

use axum::http::StatusCode;
use common::{error_code, org};
use serde_json::{json, Value};

#[tokio::test]
async fn an_admin_reads_and_changes_the_retention() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = org.call(Some(&maya), "GET", "/api/settings", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "log_retention_days": 30 }));

    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            "/api/settings",
            Some(json!({ "log_retention_days": 90 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "log_retention_days": 90 }));
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
