//! External guardrails: a signed POST to a URL the admin chose, with the texts
//! of a call, answered with allow, block or redact.
//!
//! The request (JSON, signed like an alert webhook: `x-uf-signature:
//! t=<unix seconds>,v1=<hex HMAC-SHA256 of "<t>.<body>">`):
//! `{version: 1, direction, endpoint, model, texts: [...], route, key_id,
//! team_id, user_id}`. Metadata and texts only: no images, no keys, no
//! credentials.
//!
//! The answer (JSON, up to 1 MiB, 2xx): `{action: "allow" | "block" |
//! "redact", reason?, texts?}`. `redact` carries one replacement per text, in
//! order. Anything else (a timeout, a connection error, a status other than
//! 2xx, a redirect, an answer that is too big, not JSON, of another shape, or
//! a redact with another number of texts) is a failure, and the guardrail's
//! fail mode decides: open lets the texts through and flags the call
//! `external_error:<reason>`, closed blocks it (and flags it too).
//!
//! Nothing of the URL beyond its origin and nothing of the texts or of the
//! hook's answer is logged or stored; the hook's `reason` is dropped.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::{Direction, Outcome};
use crate::alerts::sign::{header_value, SIGNATURE_HEADER};
use crate::snapshot::{SnapExternal, SnapGuardrail};

/// The biggest answer read from a hook.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Prefix of the flag a failed call leaves; the reason code follows.
pub const ERROR_FLAG_PREFIX: &str = "external_error:";

/// The label redactions by a hook are counted under.
pub const REDACTION_LABEL: &str = "external";

/// Every reason code a failure can carry (the metric's label values).
pub const REASONS: [&str; 7] = [
    "timeout",
    "connect",
    "status",
    "too_large",
    "invalid",
    "buffer_full",
    "other",
];

/// What the hook is told about the call besides the texts.
#[derive(Clone, Debug)]
pub struct CallMeta {
    /// `chat`, `messages`, `embeddings`, `playground` or `test`.
    pub endpoint: &'static str,
    /// The model or route name the caller asked for.
    pub model: String,
    pub route: Option<String>,
    pub key_id: Option<i64>,
    pub team_id: Option<i64>,
    pub user_id: Option<i64>,
}

#[derive(Serialize)]
struct HookRequest<'a> {
    version: u8,
    direction: Direction,
    endpoint: &'a str,
    model: &'a str,
    texts: &'a [String],
    route: Option<&'a str>,
    key_id: Option<i64>,
    team_id: Option<i64>,
    user_id: Option<i64>,
}

#[derive(Deserialize)]
struct HookResponse {
    action: String,
    #[serde(default)]
    texts: Option<Vec<String>>,
}

/// What a hook decided.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Block,
    /// One replacement per text.
    Redact(Vec<String>),
}

/// Why a call to a hook gave no verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Timeout,
    Connect,
    Status,
    TooLarge,
    Invalid,
    /// A stream grew past what is buffered for a check.
    BufferFull,
    Other,
}

impl Failure {
    pub fn code(self) -> &'static str {
        match self {
            Failure::Timeout => "timeout",
            Failure::Connect => "connect",
            Failure::Status => "status",
            Failure::TooLarge => "too_large",
            Failure::Invalid => "invalid",
            Failure::BufferFull => "buffer_full",
            Failure::Other => "other",
        }
    }
}

/// Reads an answer body into a verdict for `sent` texts.
fn parse(body: &[u8], sent: usize) -> Result<Verdict, Failure> {
    // Only an object: serde would read a struct from a JSON array too.
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| Failure::Invalid)?;
    if !value.is_object() {
        return Err(Failure::Invalid);
    }
    let answer: HookResponse = serde_json::from_value(value).map_err(|_| Failure::Invalid)?;
    match answer.action.as_str() {
        "allow" => Ok(Verdict::Allow),
        "block" => Ok(Verdict::Block),
        "redact" => match answer.texts {
            Some(texts) if texts.len() == sent => Ok(Verdict::Redact(texts)),
            _ => Err(Failure::Invalid),
        },
        _ => Err(Failure::Invalid),
    }
}

/// One signed call. Never takes longer than `ext.timeout`.
pub async fn call(
    http: &reqwest::Client,
    ext: &SnapExternal,
    meta: &CallMeta,
    dir: Direction,
    texts: &[String],
) -> Result<Verdict, Failure> {
    let body = serde_json::to_vec(&HookRequest {
        version: 1,
        direction: dir,
        endpoint: meta.endpoint,
        model: &meta.model,
        texts,
        route: meta.route.as_deref(),
        key_id: meta.key_id,
        team_id: meta.team_id,
        user_id: meta.user_id,
    })
    .map_err(|_| Failure::Other)?;
    let t = OffsetDateTime::now_utc().unix_timestamp();
    let request = http
        .post(&ext.url)
        .timeout(ext.timeout)
        .header("content-type", "application/json")
        .header(
            "user-agent",
            concat!("ultrafast/", env!("CARGO_PKG_VERSION")),
        )
        .header(SIGNATURE_HEADER, header_value(&ext.secret, t, &body))
        .body(body);
    // The client's own timeout covers the same span; this one also bounds a
    // body that trickles in, whatever the client does.
    match tokio::time::timeout(ext.timeout, exchange(request)).await {
        Ok(Ok(bytes)) => parse(&bytes, texts.len()),
        Ok(Err(failure)) => Err(failure),
        Err(_) => Err(Failure::Timeout),
    }
}

async fn exchange(request: reqwest::RequestBuilder) -> Result<Vec<u8>, Failure> {
    let mut response = request.send().await.map_err(failure_of)?;
    if !response.status().is_success() {
        return Err(Failure::Status);
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
    {
        return Err(Failure::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(failure_of)? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(Failure::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn failure_of(e: reqwest::Error) -> Failure {
    if e.is_timeout() {
        Failure::Timeout
    } else if e.is_connect() {
        Failure::Connect
    } else {
        Failure::Other
    }
}

/// Records a failed check of `g` in `outcome`: the flag with its reason, and
/// for a fail-closed guardrail the block. Logs the guardrail and the reason,
/// never where it points.
pub fn fail(g: &SnapGuardrail, ext: &SnapExternal, failure: Failure, outcome: &mut Outcome) {
    tracing::warn!(
        guardrail = %g.name,
        host = %ext.host,
        reason = failure.code(),
        fail_open = ext.fail_open,
        "an external guardrail could not be used"
    );
    outcome.add_flag(g.id, &format!("{ERROR_FLAG_PREFIX}{}", failure.code()));
    if !ext.fail_open && outcome.blocked_by.is_none() {
        outcome.blocked_by = Some((g.id, g.name.clone()));
    }
}

/// Asks the hook of `g` about `texts` and applies what it says to `texts` and
/// `outcome`. A block leaves the texts as they were.
pub async fn run(
    http: &reqwest::Client,
    g: &SnapGuardrail,
    ext: &SnapExternal,
    meta: &CallMeta,
    dir: Direction,
    texts: &mut [String],
    outcome: &mut Outcome,
) {
    match call(http, ext, meta, dir, texts).await {
        Ok(Verdict::Allow) => {}
        Ok(Verdict::Block) => {
            if outcome.blocked_by.is_none() {
                outcome.blocked_by = Some((g.id, g.name.clone()));
            }
        }
        Ok(Verdict::Redact(replacements)) => {
            for (text, replacement) in texts.iter_mut().zip(replacements) {
                if *text != replacement {
                    *text = replacement;
                    outcome.add_redaction(REDACTION_LABEL);
                }
            }
        }
        Err(failure) => fail(g, ext, failure, outcome),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_strictly() {
        assert_eq!(parse(br#"{"action":"allow"}"#, 2), Ok(Verdict::Allow));
        assert_eq!(
            parse(br#"{"action":"allow","texts":["ignored"],"reason":"r"}"#, 2),
            Ok(Verdict::Allow)
        );
        assert_eq!(parse(br#"{"action":"block"}"#, 1), Ok(Verdict::Block));
        assert_eq!(
            parse(br#"{"action":"redact","texts":["a","b"]}"#, 2),
            Ok(Verdict::Redact(vec!["a".into(), "b".into()]))
        );
        for bad in [
            &br#"{"action":"redact","texts":["a"]}"#[..],
            br#"{"action":"redact"}"#,
            br#"{"action":"redact","texts":null}"#,
            br#"{"action":"redact","texts":[1,2]}"#,
            br#"{"action":"ALLOW"}"#,
            br#"{"action":"maybe"}"#,
            br#"{"action":null}"#,
            br#"{}"#,
            br#"[]"#,
            br#"allow"#,
            b"",
        ] {
            assert_eq!(
                parse(bad, 2),
                Err(Failure::Invalid),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn every_reason_code_is_listed_for_the_metric() {
        for f in [
            Failure::Timeout,
            Failure::Connect,
            Failure::Status,
            Failure::TooLarge,
            Failure::Invalid,
            Failure::BufferFull,
            Failure::Other,
        ] {
            assert!(REASONS.contains(&f.code()), "{}", f.code());
        }
    }
}
