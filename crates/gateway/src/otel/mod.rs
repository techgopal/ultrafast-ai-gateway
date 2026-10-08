//! Trace export: every `/v1` call becomes a trace sent over OTLP/HTTP (JSON).
//!
//! The spans are built from the call's [`RequestRecord`] once it is over, so
//! the request path pays for one clone and one non-blocking channel send. A
//! span holds no prompt, no answer and no credential: only names, ids,
//! statuses, durations and token counts.

mod export;
mod span;

pub use export::{Exporter, OtelConfig, QUEUE_CAPACITY};
pub use span::{span_count, spans_of};

/// The `traceparent` of a call: W3C Trace Context, version 00.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceParent {
    pub trace_id: [u8; 16],
    pub parent_span_id: [u8; 8],
    pub sampled: bool,
}

impl TraceParent {
    /// `00-<32 hex>-<16 hex>-<2 hex flags>`, lower case. Any other version,
    /// bad hex or an all-zero id is not a trace parent.
    pub fn parse(header: &str) -> Option<Self> {
        if !header.is_ascii() || header.len() != 55 {
            return None;
        }
        let b = header.as_bytes();
        if b[2] != b'-' || b[35] != b'-' || b[52] != b'-' || &header[..2] != "00" {
            return None;
        }
        let mut trace_id = [0u8; 16];
        let mut parent_span_id = [0u8; 8];
        let mut flags = [0u8; 1];
        lower_hex(&header[3..35], &mut trace_id)?;
        lower_hex(&header[36..52], &mut parent_span_id)?;
        lower_hex(&header[53..55], &mut flags)?;
        if trace_id == [0; 16] || parent_span_id == [0; 8] {
            return None;
        }
        Some(Self {
            trace_id,
            parent_span_id,
            sampled: flags[0] & 1 == 1,
        })
    }
}

/// Decodes lower-case hex only, as the W3C format requires.
fn lower_hex(text: &str, out: &mut [u8]) -> Option<()> {
    if text.bytes().any(|c| c.is_ascii_uppercase()) {
        return None;
    }
    hex::decode_to_slice(text, out).ok()
}

/// `k=v,k2=v2` into pairs. A pair with no `=`, an empty name or a name or
/// value that cannot be a header is an error; the message never holds a value.
pub fn parse_headers(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for pair in text.split(',').filter(|p| !p.trim().is_empty()) {
        let Some((name, value)) = pair.split_once('=') else {
            return Err("each OTLP header must be written name=value".into());
        };
        let (name, value) = (name.trim(), value.trim());
        if axum::http::HeaderName::from_bytes(name.as_bytes()).is_err()
            || axum::http::HeaderValue::from_str(value).is_err()
        {
            return Err("an OTLP header has a name or value that is not valid".into());
        }
        out.push((name.to_string(), value.to_string()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_split_on_commas_and_first_equals() {
        let h = parse_headers("a=1, b=x=y,").unwrap();
        assert_eq!(h, [("a".into(), "1".into()), ("b".into(), "x=y".into())]);
        assert!(parse_headers("novalue").is_err());
        assert!(parse_headers("=v").is_err());
        assert!(parse_headers("bad name=v").is_err());
        assert!(parse_headers("").unwrap().is_empty());
    }
}
