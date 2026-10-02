//! `GET /api/usage`.

mod common;

use axum::http::StatusCode;
use common::{email_of, error_code, org, Org};
use serde_json::{json, Value};
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::store::NewLog;

#[allow(clippy::too_many_arguments)]
fn log(
    at: &str,
    user: Option<i64>,
    team: Option<i64>,
    requested: &str,
    status: i64,
    model: Option<&str>,
    tokens: Option<(i64, i64)>,
    cost: i64,
    priced: bool,
) -> NewLog {
    NewLog {
        at: at.into(),
        key_id: None,
        user_id: user,
        team_id: team,
        requested: requested.into(),
        endpoint: "chat".into(),
        stream: false,
        status,
        provider: model.map(|_| "main".to_string()),
        model: model.map(str::to_string),
        input_tokens: tokens.map(|t| t.0),
        output_tokens: tokens.map(|t| t.1),
        cost_micros: cost,
        priced,
        cached: false,
        duration_ms: 12,
        attempts: "[]".into(),
    }
}

struct Seeded {
    org: Org,
    lena_key: i64,
}

/// January 2026 plus one row in February:
/// 01-01 r1 lena/Platform key, r2 tomas/Research;
/// 01-02 r3 priya (429, no model, no usage), r4 tomas/Platform (usage, unpriced);
/// 01-03 r5 lena (500, no usage), r6 arjun/Research, r7 nobody with a gone key;
/// 02-10 r8 tomas/Research, model "other", 500.
async fn seeded() -> Seeded {
    let org = org().await;
    let key = generate_key();
    let mut tx = org.api.store.begin().await.unwrap();
    let lena_key = tx
        .insert_key(
            "lena-key",
            &key.hash,
            &key.display,
            None,
            Some(org.lena),
            Some(org.platform),
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let g = Some("gpt-4o");
    let mut r1 = log(
        "2026-01-01 10:00:00",
        Some(org.lena),
        Some(org.platform),
        "r1",
        200,
        g,
        Some((10, 5)),
        70,
        true,
    );
    r1.key_id = Some(lena_key);
    let r2 = log(
        "2026-01-01 23:59:59",
        Some(org.tomas),
        Some(org.research),
        "r2",
        200,
        g,
        Some((20, 10)),
        100,
        true,
    );
    let r3 = log(
        "2026-01-02 00:00:00",
        Some(org.priya),
        None,
        "r3",
        429,
        None,
        None,
        0,
        false,
    );
    let r4 = log(
        "2026-01-02 10:00:00",
        Some(org.tomas),
        Some(org.platform),
        "r4",
        200,
        g,
        Some((7, 3)),
        0,
        false,
    );
    let r5 = log(
        "2026-01-03 10:00:00",
        Some(org.lena),
        None,
        "r5",
        500,
        None,
        None,
        0,
        false,
    );
    let r6 = log(
        "2026-01-03 11:00:00",
        Some(org.arjun),
        Some(org.research),
        "r6",
        200,
        g,
        Some((1, 1)),
        5,
        true,
    );
    let mut r7 = log(
        "2026-01-03 12:00:00",
        None,
        None,
        "r7",
        200,
        g,
        Some((2, 2)),
        9,
        true,
    );
    r7.key_id = Some(9999);
    let r8 = log(
        "2026-02-10 10:00:00",
        Some(org.tomas),
        Some(org.research),
        "r8",
        500,
        Some("other"),
        Some((4, 4)),
        40,
        true,
    );
    org.api
        .store
        .insert_logs(&[r1, r2, r3, r4, r5, r6, r7, r8])
        .await
        .unwrap();
    Seeded { org, lena_key }
}

async fn usage(org: &Org, who: &str, query: &str) -> Value {
    let signed = org.sign_in(who).await;
    let (status, body) = org
        .call(Some(&signed), "GET", &format!("/api/usage{query}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{who} {query}: {body}");
    body
}

fn row(group: &str, label: &str, v: [i64; 6]) -> Value {
    json!({
        "group": group, "label": label,
        "requests": v[0], "errors": v[1], "input_tokens": v[2], "output_tokens": v[3],
        "cost_micros": v[4], "unpriced_requests": v[5],
    })
}

const JAN: &str = "from=2026-01-01&to=2026-01-31";

#[tokio::test]
async fn totals_are_exact_by_day() {
    let s = seeded().await;
    let body = usage(&s.org, "maya", &format!("?{JAN}&group=day")).await;
    assert_eq!(body["from"], "2026-01-01");
    assert_eq!(body["to"], "2026-01-31");
    assert_eq!(
        body["rows"],
        json!([
            row("2026-01-01", "2026-01-01", [2, 0, 30, 15, 170, 0]),
            row("2026-01-02", "2026-01-02", [2, 1, 7, 3, 0, 1]),
            row("2026-01-03", "2026-01-03", [3, 1, 3, 3, 14, 0]),
        ])
    );
    assert_eq!(body["total"], row("total", "Total", [7, 2, 40, 21, 184, 1]));
}

#[tokio::test]
async fn the_range_is_inclusive_and_default_group_is_day() {
    let s = seeded().await;
    let body = usage(&s.org, "maya", "?from=2026-01-03&to=2026-02-10").await;
    let days: Vec<&str> = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["group"].as_str().unwrap())
        .collect();
    assert_eq!(days, ["2026-01-03", "2026-02-10"]);
    assert_eq!(body["total"]["requests"], 4);
    // One day: both of its edges count.
    let one = usage(&s.org, "maya", "?from=2026-01-01&to=2026-01-01").await;
    assert_eq!(one["total"]["requests"], 2);
}

#[tokio::test]
async fn by_model() {
    let s = seeded().await;
    let body = usage(&s.org, "maya", &format!("?{JAN}&group=model")).await;
    // Answered rows by provider/model, the rest by the name asked for.
    assert_eq!(
        body["rows"],
        json!([
            row("main/gpt-4o", "main/gpt-4o", [5, 0, 40, 21, 184, 1]),
            row("r3", "r3", [1, 1, 0, 0, 0, 0]),
            row("r5", "r5", [1, 1, 0, 0, 0, 0]),
        ])
    );
}

#[tokio::test]
async fn by_key_user_and_team() {
    let s = seeded().await;
    let o = &s.org;
    let keys = usage(o, "maya", &format!("?{JAN}&group=key")).await;
    let rows = keys["rows"].as_array().unwrap();
    assert_eq!(rows[0], row("", "(none)", [5, 2, 28, 14, 105, 1]));
    assert_eq!(rows.len(), 3);
    let lena_key = s.lena_key.to_string();
    assert!(rows.contains(&row(&lena_key, "lena-key", [1, 0, 10, 5, 70, 0])));
    assert!(rows.contains(&row("9999", "(deleted)", [1, 0, 2, 2, 9, 0])));

    let users = usage(o, "maya", &format!("?{JAN}&group=user")).await;
    let rows = users["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    let by = |id: i64, name: &str, v| row(&id.to_string(), &email_of(name), v);
    assert!(rows.contains(&by(o.tomas, "tomas", [2, 0, 27, 13, 100, 1])));
    assert!(rows.contains(&by(o.lena, "lena", [2, 1, 10, 5, 70, 0])));
    assert!(rows.contains(&by(o.priya, "priya", [1, 1, 0, 0, 0, 0])));
    assert!(rows.contains(&row("", "(none)", [1, 0, 2, 2, 9, 0])));

    let teams = usage(o, "maya", &format!("?{JAN}&group=team")).await;
    let rows = teams["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.contains(&row(
        &o.platform.to_string(),
        "Platform",
        [2, 0, 17, 8, 70, 1]
    )));
    assert!(rows.contains(&row(
        &o.research.to_string(),
        "Research",
        [2, 0, 21, 11, 105, 0]
    )));
    assert!(rows.contains(&row("", "(none)", [3, 2, 2, 2, 9, 0])));
}

#[tokio::test]
async fn a_deleted_user_or_team_is_labelled() {
    let s = seeded().await;
    let o = &s.org;
    let gone = NewLogHelper::row("2026-01-05 10:00:00", Some(4040), Some(5050));
    o.api.store.insert_logs(&[gone]).await.unwrap();
    for (group, id) in [("user", "4040"), ("team", "5050")] {
        let body = usage(o, "maya", &format!("?{JAN}&group={group}")).await;
        let rows = body["rows"].as_array().unwrap();
        assert!(
            rows.iter()
                .any(|r| r["group"] == id && r["label"] == "(deleted)"),
            "{group}: {body}"
        );
    }
}

struct NewLogHelper;
impl NewLogHelper {
    fn row(at: &str, user: Option<i64>, team: Option<i64>) -> NewLog {
        log(
            at,
            user,
            team,
            "gone",
            200,
            Some("gpt-4o"),
            Some((1, 1)),
            1,
            true,
        )
    }
}

#[tokio::test]
async fn scope_follows_the_logs_scope() {
    let s = seeded().await;
    let o = &s.org;
    // lena (member): only her own user id.
    let body = usage(o, "lena", &format!("?{JAN}&group=user")).await;
    assert_eq!(
        body["rows"],
        json!([row(
            &o.lena.to_string(),
            &email_of("lena"),
            [2, 1, 10, 5, 70, 0]
        )])
    );
    // Her rows are in Platform and in no team: those are her teams, no other.
    let body = usage(o, "lena", &format!("?{JAN}&group=team")).await;
    let groups: Vec<&str> = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["group"].as_str().unwrap())
        .collect();
    assert_eq!(groups.len(), 2);
    assert!(groups.contains(&"") && groups.contains(&o.platform.to_string().as_str()));
    // arjun leads Platform: r1, r4 (team), r5 (member lena), r6 (own).
    let body = usage(o, "arjun", &format!("?{JAN}&group=day")).await;
    assert_eq!(body["total"]["requests"], 4);
    // priya sees only her row; tomas the rows of his id.
    assert_eq!(
        usage(o, "priya", &format!("?{JAN}")).await["total"]["requests"],
        1
    );
    // Rows without an owner are for admins only.
    let body = usage(o, "arjun", &format!("?{JAN}&group=key")).await;
    assert!(body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["group"] != "9999"));
}

#[tokio::test]
async fn totals_equal_the_sum_of_the_logs_list() {
    let s = seeded().await;
    let o = &s.org;
    for who in ["maya", "arjun", "lena", "tomas", "priya"] {
        let signed = o.sign_in(who).await;
        let (status, list) = o
            .call(
                Some(&signed),
                "GET",
                "/api/logs?from=2026-01-01&to=2026-01-31&limit=200",
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let logs = list["logs"].as_array().unwrap();
        let sum = |f: &str| -> i64 { logs.iter().map(|l| l[f].as_i64().unwrap_or(0)).sum() };
        let total = &usage(o, who, &format!("?{JAN}&group=model")).await["total"];
        assert_eq!(total["requests"], logs.len() as i64, "{who}");
        assert_eq!(total["input_tokens"], sum("input_tokens"), "{who}");
        assert_eq!(total["output_tokens"], sum("output_tokens"), "{who}");
        assert_eq!(total["cost_micros"], sum("cost_micros"), "{who}");
        let errors = logs.iter().filter(|l| l["status"].as_i64().unwrap() >= 400);
        assert_eq!(total["errors"], errors.count() as i64, "{who}");
    }
}

#[tokio::test]
async fn range_is_validated() {
    let s = seeded().await;
    let o = &s.org;
    let maya = o.sign_in("maya").await;
    for (query, field) in [
        ("?from=2026-01-05&to=2026-01-04", "to"),
        ("?from=2026-01-01&to=2027-01-02", "to"),
        ("?from=nonsense", "from"),
        ("?to=2026-13-01", "to"),
        ("?from=2026-01-01T00:00:00Z", "from"),
        ("?group=week", "group"),
        ("?group=", "group"),
    ] {
        let (status, body) = o
            .call(Some(&maya), "GET", &format!("/api/usage{query}"), None)
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}: {body}");
        assert_eq!(error_code(&body), "validation_failed", "{query}");
        assert!(
            body["error"]["fields"][field].is_string(),
            "{query}: {body}"
        );
    }
    // 366 days is the most.
    let body = usage(o, "maya", "?from=2026-01-01&to=2027-01-01").await;
    assert_eq!(body["total"]["requests"], 8);
}

#[tokio::test]
async fn the_default_range_is_the_last_30_days() {
    let s = seeded().await;
    let body = usage(&s.org, "maya", "").await;
    let day = |k: &str| {
        time::Date::parse(
            body[k].as_str().unwrap(),
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .unwrap()
    };
    assert_eq!((day("to") - day("from")).whole_days(), 29);
    // Only `from`: it runs to today; only `to`: 30 days up to it.
    let body = usage(&s.org, "maya", "?to=2026-02-10").await;
    assert_eq!(body["from"], "2026-01-12");
    assert_eq!(body["total"]["requests"], 1);
}

#[tokio::test]
async fn needs_a_session() {
    let s = seeded().await;
    let (status, body) = s.org.call(None, "GET", "/api/usage", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "unauthenticated");
}

#[tokio::test]
async fn an_error_names_the_field_the_caller_sent() {
    let s = seeded().await;
    let o = &s.org;
    let maya = o.sign_in("maya").await;
    for (query, field) in [
        // Only `from`, too far back: the error is on `from`.
        ("?from=2000-01-01", "from"),
        // Only `from`, in the far future: after today.
        ("?from=2999-01-01", "from"),
        // Both sent: `to` is the one out of range.
        ("?from=2026-01-05&to=2026-01-04", "to"),
        // The last day there is cannot be counted whole.
        ("?from=9999-12-01&to=9999-12-31", "to"),
    ] {
        let (status, body) = o
            .call(Some(&maya), "GET", &format!("/api/usage{query}"), None)
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}: {body}");
        let fields = body["error"]["fields"].as_object().unwrap();
        assert_eq!(fields.keys().collect::<Vec<_>>(), [field], "{query}");
    }
}
