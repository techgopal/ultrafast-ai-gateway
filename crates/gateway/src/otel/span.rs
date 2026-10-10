//! The spans of one call, in the OTLP/JSON encoding.

use serde_json::{json, Value};

use crate::telemetry::{Attempt, AttemptOutcome, RequestRecord};

/// OTLP `SpanKind` and `StatusCode` values.
const KIND_SERVER: u8 = 2;
const KIND_CLIENT: u8 = 3;
const STATUS_ERROR: u8 = 2;

fn string(key: &str, value: &str) -> Value {
    json!({ "key": key, "value": { "stringValue": value } })
}

/// 64-bit integers are strings in the JSON encoding.
fn int(key: &str, value: impl ToString) -> Value {
    json!({ "key": key, "value": { "intValue": value.to_string() } })
}

fn boolean(key: &str, value: bool) -> Value {
    json!({ "key": key, "value": { "boolValue": value } })
}

/// Nanoseconds since the epoch of the call's start, as a `u128` (the year
/// 2999 does not fit 64 bits of nanoseconds).
fn start_nanos(record: &RequestRecord) -> u128 {
    u128::from(record.started_unix_ms) * 1_000_000
}

fn at(start: u128, ms: u64) -> String {
    (start + u128::from(ms) * 1_000_000).to_string()
}

fn outcome_name(o: AttemptOutcome) -> &'static str {
    match o {
        AttemptOutcome::Ok => "ok",
        AttemptOutcome::Retryable => "retryable",
        AttemptOutcome::Fatal => "fatal",
        AttemptOutcome::CircuitOpen => "circuit_open",
        AttemptOutcome::Skipped => "skipped",
        AttemptOutcome::Cached => "cached",
    }
}

/// How many spans [`spans_of`] makes of `record`.
pub fn span_count(record: &RequestRecord) -> u64 {
    if record.cached {
        return 1;
    }
    1 + record
        .attempts
        .iter()
        .filter(|a| {
            matches!(
                a.outcome,
                AttemptOutcome::Ok | AttemptOutcome::Retryable | AttemptOutcome::Fatal
            )
        })
        .count() as u64
}

/// The spans of a finished call: the server span first, then one client
/// span per attempt that reached (or tried to reach) a provider. `ids`
/// supplies span ids; the trace id is that of the incoming `traceparent`,
/// or a new one, chosen by the caller.
pub fn spans_of(
    record: &RequestRecord,
    ids: &mut impl FnMut() -> [u8; 8],
    trace_id: [u8; 16],
) -> Vec<Value> {
    let start = start_nanos(record);
    let trace = hex::encode(trace_id);
    let server_id = ids();

    let mut attributes = vec![
        string("uf.endpoint", record.endpoint),
        string("uf.requested", &record.requested),
        int("http.response.status_code", record.status),
        boolean("uf.stream", record.stream),
        boolean("uf.cached", record.cached),
        boolean("uf.estimated", record.estimated),
    ];
    for (key, id) in [
        ("uf.key_id", record.key_id),
        ("uf.user_id", record.user_id),
        ("uf.team_id", record.team_id),
    ] {
        if let Some(id) = id {
            attributes.push(int(key, id));
        }
    }
    if let Some(u) = record.usage {
        attributes.push(int("gen_ai.usage.input_tokens", u.input_tokens));
        attributes.push(int("gen_ai.usage.output_tokens", u.output_tokens));
    }
    for (name, value) in &record.tags {
        attributes.push(string(&format!("uf.tags.{name}"), value));
    }
    // Which template (`name@version`) the call used: its label, never the
    // text it holds.
    if let Some(label) = &record.prompt {
        attributes.push(string("uf.prompt_template", label));
    }
    // The worst thing the guardrails did to the call; absent when nothing.
    if let Some(g) = &record.guardrails {
        attributes.push(string("uf.guardrail.action", g.action.as_str()));
    }

    let kind_of = |provider: &str| {
        record
            .provider_kinds
            .iter()
            .find(|(p, _)| p == provider)
            .map_or("", |(_, k)| *k)
    };
    let mut events = Vec::new();
    let mut children = Vec::new();
    for a in &record.attempts {
        match a.outcome {
            AttemptOutcome::Skipped | AttemptOutcome::CircuitOpen => {
                let name = if a.outcome == AttemptOutcome::Skipped {
                    "uf.skipped"
                } else {
                    "uf.circuit_open"
                };
                events.push(json!({
                    "timeUnixNano": at(start, a.offset_ms),
                    "name": name,
                    "attributes": [string("uf.provider", &a.provider), string("gen_ai.request.model", &a.model)],
                }));
            }
            AttemptOutcome::Cached => {}
            AttemptOutcome::Ok | AttemptOutcome::Retryable | AttemptOutcome::Fatal => {
                children.push(attempt_span(
                    a,
                    &trace,
                    &hex::encode(server_id),
                    ids(),
                    start,
                    kind_of(&a.provider),
                ));
            }
        }
    }

    let mut server = json!({
        "traceId": trace,
        "spanId": hex::encode(server_id),
        "name": format!("uf.{}", record.endpoint),
        "kind": KIND_SERVER,
        "startTimeUnixNano": at(start, 0),
        "endTimeUnixNano": at(start, record.duration_ms),
        "attributes": attributes,
    });
    if let Some(parent) = &record.trace_parent {
        server["parentSpanId"] = Value::String(hex::encode(parent.parent_span_id));
    }
    if record.status >= 500 {
        server["status"] = json!({ "code": STATUS_ERROR });
    }
    if !events.is_empty() {
        server["events"] = Value::Array(events);
    }
    let mut spans = vec![server];
    if !record.cached {
        spans.extend(children);
    }
    spans
}

fn attempt_span(
    a: &Attempt,
    trace: &str,
    parent: &str,
    id: [u8; 8],
    start: u128,
    kind: &str,
) -> Value {
    let mut attributes = Vec::new();
    // Only when the kind is known (a provider removed mid-call has none).
    if !kind.is_empty() {
        attributes.push(string("gen_ai.system", kind));
        attributes.push(string("gen_ai.provider.name", kind));
    }
    attributes.push(string("uf.provider", &a.provider));
    attributes.push(string("gen_ai.request.model", &a.model));
    if let Some(status) = a.status {
        attributes.push(int("http.response.status_code", status));
    }
    attributes.push(string("uf.outcome", outcome_name(a.outcome)));
    let mut span = json!({
        "traceId": trace,
        "spanId": hex::encode(id),
        "parentSpanId": parent,
        "name": format!("uf.attempt {}", a.provider),
        "kind": KIND_CLIENT,
        "startTimeUnixNano": at(start, a.offset_ms),
        "endTimeUnixNano": at(start, a.offset_ms.saturating_add(a.duration_ms)),
        "attributes": attributes,
    });
    if a.outcome != AttemptOutcome::Ok {
        span["status"] = json!({ "code": STATUS_ERROR });
    }
    span
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::otel::TraceParent;
    use crate::telemetry::{Attempt, AttemptOutcome};

    fn attempt(
        p: &str,
        outcome: AttemptOutcome,
        status: Option<u16>,
        off: u64,
        dur: u64,
    ) -> Attempt {
        Attempt {
            provider: p.into(),
            model: "m".into(),
            outcome,
            status,
            duration_ms: dur,
            offset_ms: off,
            skipped: None,
        }
    }

    fn base() -> RequestRecord {
        RequestRecord {
            key_id: Some(7),
            user_id: Some(1),
            team_id: None,
            requested: "route-a".into(),
            endpoint: "chat",
            stream: false,
            status: 200,
            usage: Some(ultrafast_translate::types::Usage {
                input_tokens: 10,
                output_tokens: 5,
            }),
            attempts: vec![],
            cached: false,
            estimated: false,
            started_at: "2999-01-01 00:00:00".into(),
            duration_ms: 900,
            tags: [("team".to_string(), "red".to_string())].into(),
            trace_parent: None,
            provider_kinds: vec![("a".into(), "openai"), ("b".into(), "anthropic")],
            started_unix_ms: 32_472_144_000_000,
            guardrails: None,
            prompt: None,
        }
    }

    fn counter() -> impl FnMut() -> [u8; 8] {
        let mut n = 0u64;
        move || {
            n += 1;
            (0x1000 + n).to_be_bytes()
        }
    }

    fn attr<'a>(span: &'a Value, key: &str) -> Option<&'a Value> {
        span["attributes"]
            .as_array()?
            .iter()
            .find(|a| a["key"] == key)
            .map(|a| &a["value"])
    }

    const T0: u128 = 32_472_144_000_000_000_000; // started_unix_ms in base() // 2999-01-01 00:00:00 UTC

    #[test]
    fn a_huge_requested_name_reaches_the_span_cut() {
        use crate::telemetry::{RequestSink, Scope};
        use std::sync::{Arc, Mutex};
        #[derive(Default)]
        struct Mem(Mutex<Vec<RequestRecord>>);
        impl RequestSink for Mem {
            fn record(&self, record: RequestRecord) {
                self.0.lock().unwrap().push(record);
            }
        }
        let sink = Arc::new(Mem::default());
        let mut scope = Scope::begin(sink.clone(), Some(7), Some(1), None, "chat");
        scope.requested(&"m".repeat(1 << 20), false);
        scope.finish(404);
        let record = sink.0.lock().unwrap().remove(0);
        let spans = spans_of(&record, &mut counter(), [9; 16]);
        let value = attr(&spans[0], "uf.requested").unwrap()["stringValue"]
            .as_str()
            .unwrap()
            .len();
        assert!(value <= 256, "{value}");
    }

    #[test]
    fn spans_of_a_call_with_retry_and_fallback() {
        let mut r = base();
        r.trace_parent = Some(TraceParent {
            trace_id: [9; 16],
            parent_span_id: [4; 8],
            sampled: true,
        });
        r.attempts = vec![
            attempt("a", AttemptOutcome::Retryable, Some(503), 0, 200),
            attempt("a", AttemptOutcome::Retryable, None, 450, 100),
            attempt("b", AttemptOutcome::Ok, Some(200), 600, 300),
        ];
        let spans = spans_of(&r, &mut counter(), [9; 16]);
        assert_eq!(spans.len(), 4);
        let server = &spans[0];
        assert_eq!(server["name"], "uf.chat");
        assert_eq!(server["kind"], 2);
        assert_eq!(server["parentSpanId"], "0404040404040404");
        assert_eq!(server["traceId"], hex::encode([9u8; 16]));
        assert_eq!(server["startTimeUnixNano"], T0.to_string());
        assert_eq!(server["endTimeUnixNano"], (T0 + 900_000_000).to_string());
        assert!(server.get("status").is_none_or(|s| s["code"] == 0));
        assert_eq!(
            attr(server, "uf.requested").unwrap()["stringValue"],
            "route-a"
        );
        assert_eq!(attr(server, "uf.key_id").unwrap()["intValue"], "7");
        assert!(attr(server, "uf.team_id").is_none());
        assert_eq!(attr(server, "uf.tags.team").unwrap()["stringValue"], "red");
        assert_eq!(
            attr(server, "gen_ai.usage.input_tokens").unwrap()["intValue"],
            "10"
        );
        let sid = server["spanId"].clone();
        let c = &spans[1..];
        for (i, (off, dur)) in [(0u64, 200u64), (450, 100), (600, 300)].iter().enumerate() {
            assert_eq!(c[i]["kind"], 3);
            assert_eq!(c[i]["parentSpanId"], sid);
            assert_eq!(
                c[i]["startTimeUnixNano"],
                (T0 + u128::from(*off) * 1_000_000).to_string()
            );
            assert_eq!(
                c[i]["endTimeUnixNano"],
                (T0 + u128::from(off + dur) * 1_000_000).to_string()
            );
        }
        assert_eq!(c[0]["name"], "uf.attempt a");
        assert_eq!(c[2]["name"], "uf.attempt b");
        assert_eq!(c[0]["status"]["code"], 2);
        assert_eq!(c[1]["status"]["code"], 2);
        assert!(c[2].get("status").is_none_or(|s| s["code"] == 0));
        assert_eq!(
            attr(&c[0], "gen_ai.system").unwrap()["stringValue"],
            "openai"
        );
        assert_eq!(
            attr(&c[2], "gen_ai.system").unwrap()["stringValue"],
            "anthropic"
        );
        assert_eq!(attr(&c[2], "uf.provider").unwrap()["stringValue"], "b");
        assert_eq!(
            attr(&c[0], "http.response.status_code").unwrap()["intValue"],
            "503"
        );
        assert!(attr(&c[1], "http.response.status_code").is_none());
        assert_eq!(attr(&c[2], "uf.outcome").unwrap()["stringValue"], "ok");
    }

    #[test]
    fn the_server_span_encloses_its_children() {
        let mut r = base();
        r.duration_ms = 500;
        r.attempts = vec![
            attempt("a", AttemptOutcome::Retryable, Some(500), 5, 100),
            attempt("b", AttemptOutcome::Ok, Some(200), 120, 380),
        ];
        let spans = spans_of(&r, &mut counter(), [1; 16]);
        let n = |v: &Value, k: &str| v[k].as_str().unwrap().parse::<u128>().unwrap();
        for c in &spans[1..] {
            assert!(n(&spans[0], "startTimeUnixNano") <= n(c, "startTimeUnixNano"));
            assert!(n(&spans[0], "endTimeUnixNano") >= n(c, "endTimeUnixNano"));
        }
    }

    #[test]
    fn the_provider_kind_is_named_only_when_known() {
        let mut r = base();
        r.attempts = vec![
            attempt("a", AttemptOutcome::Ok, Some(200), 0, 5),
            attempt("gone", AttemptOutcome::Retryable, None, 6, 5),
        ];
        let spans = spans_of(&r, &mut counter(), [1; 16]);
        assert_eq!(
            attr(&spans[1], "gen_ai.system").unwrap()["stringValue"],
            "openai"
        );
        assert_eq!(
            attr(&spans[1], "gen_ai.provider.name").unwrap()["stringValue"],
            "openai"
        );
        assert!(attr(&spans[2], "gen_ai.system").is_none());
        assert!(attr(&spans[2], "gen_ai.provider.name").is_none());
    }

    #[test]
    fn a_server_error_is_an_error_span() {
        let mut r = base();
        r.status = 502;
        let spans = spans_of(&r, &mut counter(), [1; 16]);
        assert_eq!(spans[0]["status"]["code"], 2);
        assert!(spans[0].get("parentSpanId").is_none());
    }

    #[test]
    fn cached_call_is_one_span() {
        let mut r = base();
        r.cached = true;
        r.attempts = vec![attempt("a", AttemptOutcome::Cached, None, 0, 0)];
        let spans = spans_of(&r, &mut counter(), [1; 16]);
        assert_eq!(spans.len(), 1);
        assert_eq!(attr(&spans[0], "uf.cached").unwrap()["boolValue"], true);
    }

    #[test]
    fn skipped_and_circuit_open_are_events() {
        let mut r = base();
        r.attempts = vec![
            attempt("a", AttemptOutcome::CircuitOpen, None, 0, 0),
            attempt("b", AttemptOutcome::Ok, Some(200), 3, 10),
            attempt("c", AttemptOutcome::Skipped, None, 0, 0),
        ];
        let spans = spans_of(&r, &mut counter(), [1; 16]);
        assert_eq!(spans.len(), 2);
        let names: Vec<_> = spans[0]["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["uf.circuit_open", "uf.skipped"]);
    }

    #[test]
    fn a_span_names_the_prompt_template_the_call_used() {
        let mut r = base();
        r.attempts = vec![attempt("a", AttemptOutcome::Ok, Some(200), 0, 5)];
        let value_of = |r: &RequestRecord| {
            spans_of(r, &mut counter(), [1; 16])[0]["attributes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["key"] == "uf.prompt_template")
                .map(|a| a["value"]["stringValue"].as_str().unwrap().to_string())
        };
        assert_eq!(value_of(&r), None);
        r.prompt = Some("greet@3".into());
        assert_eq!(value_of(&r).as_deref(), Some("greet@3"));
    }

    #[test]
    fn no_prompt_or_secret_in_spans() {
        let mut r = base();
        r.attempts = vec![attempt("a", AttemptOutcome::Ok, Some(200), 0, 5)];
        let text = serde_json::to_string(&spans_of(&r, &mut counter(), [1; 16])).unwrap();
        for forbidden in [
            "prompt",
            "content",
            "api_key",
            "authorization",
            "sk-",
            "uf_",
        ] {
            assert!(!text.contains(forbidden), "{forbidden} in {text}");
        }
    }

    #[test]
    fn traceparent_parse() {
        let ok = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let p = TraceParent::parse(ok).unwrap();
        assert_eq!(hex::encode(p.trace_id), "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(hex::encode(p.parent_span_id), "b7ad6b7169203331");
        assert!(p.sampled);
        assert!(
            !TraceParent::parse(&ok.replace("-01", "-00"))
                .unwrap()
                .sampled
        );
        assert!(TraceParent::parse(&ok.replacen("00-", "01-", 1)).is_none());
        assert!(TraceParent::parse(&ok.replacen("00-", "ff-", 1)).is_none());
        assert!(TraceParent::parse(&ok.replace('c', "g")).is_none());
        assert!(TraceParent::parse(&ok.to_uppercase()).is_none());
        assert!(
            TraceParent::parse(&format!("00-{}-b7ad6b7169203331-01", "0".repeat(32))).is_none()
        );
        assert!(TraceParent::parse(&format!(
            "00-0af7651916cd43dd8448eb211c80319c-{}-01",
            "0".repeat(16)
        ))
        .is_none());
        assert!(TraceParent::parse("").is_none());
        assert!(TraceParent::parse(&format!("{ok}-extra")).is_none());
        assert!(TraceParent::parse(&ok[..54]).is_none());
    }

    #[test]
    fn the_server_span_names_the_worst_guardrail_action_only_when_there_was_one() {
        use crate::guardrails::log::{GuardrailLog, LoggedAction};
        let mut record = base();
        let spans = spans_of(&record, &mut counter(), [1; 16]);
        assert!(attr(&spans[0], "uf.guardrail.action").is_none());
        record.guardrails = Some(GuardrailLog {
            action: LoggedAction::Redacted,
            input: None,
            output: None,
        });
        let spans = spans_of(&record, &mut counter(), [1; 16]);
        assert_eq!(
            attr(&spans[0], "uf.guardrail.action").unwrap()["stringValue"],
            "redacted"
        );
    }
}
