//! The Rust client and the WebAssembly boundary (which the TypeScript client
//! uses) answer every HTTP error identically.

mod common;

use common::*;
use serde_json::Value;
use ultrafast_client::{ChatRequest, Client, Target};
use ultrafast_client_wasm::api;

const BODIES: [&str; 5] = [
    r#"{"error":{"message":"slow down","type":"rate_limit_error"}}"#,
    r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
    r#"{"message":"flat message"}"#,
    "<html>bad gateway</html>",
    "",
];

#[tokio::test]
async fn the_same_answers_give_the_same_error_in_both_clients() {
    let long = "z".repeat(900);
    let mut cases: Vec<(u16, String, Option<&str>)> = Vec::new();
    for status in [
        301, 302, 400, 401, 403, 404, 408, 422, 429, 500, 502, 503, 504,
    ] {
        for body in BODIES {
            for ra in [
                None,
                Some("7"),
                Some("99999999999"),
                Some("Wed, 21 Oct 2015 07:28:00 GMT"),
                Some("-1"),
            ] {
                cases.push((status, body.to_string(), ra));
            }
        }
        cases.push((status, long.clone(), Some("3")));
    }
    for (status, body, ra) in cases {
        let mut script = Script::json(status, &body);
        if let Some(ra) = ra {
            script = script.header("retry-after", ra);
        }
        let s = serve(script).await;
        let e = Client::new(Target::openai_compatible(&s.url, "k"))
            .chat(ChatRequest::new("m").user("hi"))
            .await
            .unwrap_err();
        let w: Value =
            serde_json::from_str(&api::classify_error(status, body.as_bytes(), ra)).unwrap();
        let at = format!(
            "status {status} body {:?} retry-after {ra:?}",
            &body[..body.len().min(30)]
        );
        assert_eq!(w["kind"], e.kind.as_str(), "{at}");
        assert_eq!(w["retryable"], e.retryable, "{at}");
        assert_eq!(w["status"], status, "{at}");
        assert_eq!(w["message"], e.message.as_str(), "{at}");
        assert_eq!(
            w["retry_after_secs"].as_u64(),
            e.retry_after.map(|d| d.as_secs()),
            "{at}"
        );
    }
}
