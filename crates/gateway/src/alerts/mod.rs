//! Alerts: stored events delivered to channels as signed webhooks.
//!
//! A channel is a URL the gateway posts to. Every post is signed (see
//! [`sign`]) so a receiver can tell it came from the gateway. Delivery runs
//! on a background task behind a bounded queue and never touches a `/v1`
//! call.

mod deliver;
pub mod engine;
pub mod errors_window;
pub mod rules;
pub mod sign;

pub use deliver::{
    send_once, Attempt, Deliverer, DeliveryConfig, CHANNEL_BACKLOG, CHANNEL_CONCURRENCY,
    QUEUE_CAPACITY, TRY_TIMEOUT,
};
pub use engine::{EngineConfig, EngineHandle};

use serde_json::{json, Value};

use crate::store::{parse_timestamp, AlertEventRow};

/// The body posted to a channel of this kind (`webhook` or `slack`) for
/// this event. Metadata only: the event holds nothing else.
pub fn payload(channel_kind: &str, event: &AlertEventRow) -> Vec<u8> {
    let value = if channel_kind == "slack" {
        // Works for Slack and for the incoming webhooks that read the same
        // `text` (Mattermost, Rocket.Chat).
        json!({ "text": one_line(&format!(
            "Alert {} ({}): {}",
            event.state, event.rule_name, event.summary
        )) })
    } else {
        let details: Value = serde_json::from_str(&event.details).unwrap_or_else(|_| json!({}));
        json!({
            "version": 1,
            "id": event.id,
            "state": event.state,
            "rule": { "id": event.rule_id, "name": event.rule_name, "kind": event.kind },
            "subject": event.subject,
            "summary": event.summary,
            "details": details,
            "at": rfc3339(&event.at),
            "gateway": format!("ultrafast {}", env!("CARGO_PKG_VERSION")),
        })
    };
    serde_json::to_vec(&value).expect("a JSON value serializes")
}

/// A stored time (`YYYY-MM-DD HH:MM:SS`, UTC) as RFC 3339.
fn rfc3339(at: &str) -> String {
    match parse_timestamp(at) {
        Some(_) => format!("{}Z", at.replace(' ', "T")),
        None => at.to_string(),
    }
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> AlertEventRow {
        AlertEventRow {
            id: 7,
            rule_id: Some(3),
            rule_name: "Budget".into(),
            kind: "budget".into(),
            subject: "budget:12:2999-01-01".into(),
            state: "firing".into(),
            summary: "Line one\nline two".into(),
            details: r#"{"spent":5}"#.into(),
            at: "2999-01-01 10:20:30".into(),
            deliveries: "[]".into(),
        }
    }

    #[test]
    fn the_webhook_payload_has_the_documented_shape() {
        let p: Value = serde_json::from_slice(&payload("webhook", &event())).unwrap();
        assert_eq!(p["version"], 1);
        assert_eq!(p["id"], 7);
        assert_eq!(p["state"], "firing");
        assert_eq!(
            p["rule"],
            json!({ "id": 3, "name": "Budget", "kind": "budget" })
        );
        assert_eq!(p["subject"], "budget:12:2999-01-01");
        assert_eq!(p["details"], json!({ "spent": 5 }));
        assert_eq!(p["at"], "2999-01-01T10:20:30Z");
        assert!(p["gateway"].as_str().unwrap().starts_with("ultrafast "));
        assert_eq!(p.as_object().unwrap().len(), 9);
    }

    #[test]
    fn the_slack_payload_is_one_line_of_text() {
        let p: Value = serde_json::from_slice(&payload("slack", &event())).unwrap();
        assert_eq!(
            p,
            json!({ "text": "Alert firing (Budget): Line one line two" })
        );
    }
}
