//! Tags: names and values a caller or an admin puts on a call or a key, to
//! tell calls apart in the logs and the usage reports.
//!
//! One set of rules serves the `x-uf-tags` header, the tags of a key and the
//! filters of the logs, so a tag that can be sent can be stored and found.

use std::collections::BTreeMap;

use axum::http::HeaderMap;
use ultrafast_translate::tags::{HEADER, MAX_TAGS_BYTES};

/// The most tags a call, a key or a header may have.
pub const MAX_TAGS: usize = 20;
/// The longest name or value, in characters.
pub const MAX_PART_CHARS: usize = 64;

pub type Tags = BTreeMap<String, String>;

const NOT_JSON: &str = "it is not JSON";
const NOT_STRINGS: &str = "it must be a JSON object of strings";

/// Why a set of tags is refused, for the sentence that names it. Checks the
/// count, then each name and value. `None` when it is fine.
pub fn refusal(tags: &Tags) -> Option<&'static str> {
    if tags.len() > MAX_TAGS {
        return Some("it has more than 20 entries");
    }
    for (name, value) in tags {
        if let Some(reason) = refusal_of_name(name).or_else(|| refusal_of_value(value)) {
            return Some(reason);
        }
    }
    None
}

/// Why a name is refused.
pub fn refusal_of_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("a name or value is empty");
    }
    if name.chars().count() > MAX_PART_CHARS {
        return Some("a name or value is longer than 64 characters");
    }
    if !name.chars().all(name_char) {
        return Some("a name may use only A-Z a-z 0-9 _ . -");
    }
    None
}

/// Why a value is refused.
pub fn refusal_of_value(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        return Some("a name or value is empty");
    }
    if value.chars().count() > MAX_PART_CHARS {
        return Some("a name or value is longer than 64 characters");
    }
    None
}

fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')
}

/// The tags of the `x-uf-tags` header: none when it is absent, else the
/// object it holds. The error is the sentence for the caller.
pub fn from_headers(headers: &HeaderMap) -> Result<Tags, String> {
    let mut values = headers.get_all(HEADER).iter();
    let Some(value) = values.next() else {
        return Ok(Tags::new());
    };
    let refused = |reason: &str| format!("The {HEADER} header is not valid: {reason}.");
    if values.next().is_some() {
        return Err(refused("it was sent more than once"));
    }
    if value.len() > MAX_TAGS_BYTES {
        return Err(refused(&format!(
            "it is longer than {MAX_TAGS_BYTES} bytes"
        )));
    }
    // The clients send ASCII; UTF-8 is accepted as it is.
    let Ok(text) = std::str::from_utf8(value.as_bytes()) else {
        return Err(refused(NOT_JSON));
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return Err(refused(NOT_JSON));
    };
    let Some(object) = json.as_object() else {
        return Err(refused(NOT_STRINGS));
    };
    let mut tags = Tags::new();
    for (name, value) in object {
        let Some(value) = value.as_str() else {
            return Err(refused(NOT_STRINGS));
        };
        tags.insert(name.clone(), value.to_string());
    }
    match refusal(&tags) {
        Some(reason) => Err(refused(reason)),
        None => Ok(tags),
    }
}

/// The tags a call is recorded with: those it sent, overlaid by its key's.
/// The key wins on a name both have, so a caller cannot relabel what an
/// admin fixed.
pub fn effective(call: Tags, key: &Tags) -> Tags {
    let mut tags = call;
    tags.extend(key.iter().map(|(k, v)| (k.clone(), v.clone())));
    tags
}

/// Reads stored tags. A value that cannot be read is no tags.
pub fn parse_stored(raw: Option<&str>) -> Tags {
    raw.and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_default()
}

/// Stored form: `None` when there are no tags.
pub fn to_stored(tags: &Tags) -> Option<String> {
    (!tags.is_empty()).then(|| serde_json::to_string(tags).expect("strings serialize"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_lets_the_key_win() {
        let call: Tags = [("a", "call"), ("b", "call")]
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .into();
        let key: Tags = [("b", "key"), ("c", "key")]
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .into();
        let got = effective(call, &key);
        assert_eq!(got["a"], "call");
        assert_eq!(got["b"], "key");
        assert_eq!(got["c"], "key");
    }

    #[test]
    fn stored_form_round_trips_and_none_is_null() {
        assert_eq!(to_stored(&Tags::new()), None);
        let t: Tags = [("a".to_string(), "b".to_string())].into();
        assert_eq!(parse_stored(to_stored(&t).as_deref()), t);
        assert!(parse_stored(Some("garbage")).is_empty());
        assert!(parse_stored(None).is_empty());
    }
}
