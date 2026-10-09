//! What the gateway logs about a stored rules guardrail that no longer
//! compiles. This file is its own test binary: the subscriber is global.

mod common;

use std::sync::{Arc, Mutex};

use common::harness;
use ultrafast_gateway::snapshot::Snapshot;
use ultrafast_gateway::store::NewGuardrail;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

#[tokio::test]
async fn a_stored_rules_guardrail_that_no_longer_compiles_is_warned_about_by_name_and_id() {
    let capture = Capture::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(capture.clone())
            .finish(),
    )
    .unwrap();
    let h = harness("openai").await;
    // Rules the API would have refused, with a marker that must not be logged.
    let broken = r#"[{"id":"MARKER-RULE","matcher":{"nothing":"MARKER-BODY"}}"#;
    let mut tx = h.store.begin().await.unwrap();
    let id = tx
        .insert_guardrail(NewGuardrail {
            name: "stale-rules",
            description: "",
            kind: "rules",
            rules: broken,
            url: None,
            secret_enc: None,
            timeout_ms: 3000,
            fail_mode: "open",
            directions: "both",
            enabled: true,
            is_default: false,
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let snap = Snapshot::load(&h.store, &h.state.cipher).await.unwrap();
    assert!(snap.guardrail(id).is_none());

    let log = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    let line = log
        .lines()
        .find(|l| l.contains("WARN") && l.contains("stale-rules"))
        .unwrap_or_else(|| panic!("no warning about the guardrail in:\n{log}"));
    assert!(line.contains(&format!("guardrail_id={id}")), "{line}");
    assert!(!line.contains("MARKER"), "{line}");
}
