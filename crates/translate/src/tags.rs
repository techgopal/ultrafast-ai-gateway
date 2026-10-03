//! The `x-uf-tags` header a gateway reads. Shared by every client.

use std::collections::BTreeMap;

use crate::classify::{Classified, ErrorKind};

/// The most the `x-uf-tags` header may hold.
pub const MAX_TAGS_BYTES: usize = 1024;

/// The header name; sent to a gateway only, never to a provider.
pub const HEADER: &str = "x-uf-tags";

/// The `x-uf-tags` value: compact JSON in ASCII, at most 1 KiB. None when
/// there are no tags.
pub fn tags_header(tags: &BTreeMap<String, String>) -> Result<Option<String>, Classified> {
    if tags.is_empty() {
        return Ok(None);
    }
    let json = serde_json::to_string(tags)
        .map_err(|_| Classified::new(ErrorKind::InvalidRequest, "tags could not be encoded"))?;
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if (' '..'\u{7f}').contains(&c) {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for u in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{u:04x}"));
            }
        }
    }
    if out.len() > MAX_TAGS_BYTES {
        return Err(Classified::new(
            ErrorKind::InvalidRequest,
            format!("tags exceed {MAX_TAGS_BYTES} bytes"),
        ));
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn none_without_tags_compact_json_with() {
        assert_eq!(tags_header(&BTreeMap::new()).unwrap(), None);
        assert_eq!(
            tags_header(&map(&[("team", "x"), ("a", "b")]))
                .unwrap()
                .as_deref(),
            Some(r#"{"a":"b","team":"x"}"#)
        );
    }

    #[test]
    fn non_ascii_is_escaped_including_surrogate_pairs() {
        let h = tags_header(&map(&[("k", "é😀")])).unwrap().unwrap();
        assert!(h.is_ascii(), "{h}");
        assert_eq!(h, "{\"k\":\"\\u00e9\\ud83d\\ude00\"}");
    }

    #[test]
    fn over_one_kib_is_refused() {
        let e = tags_header(&map(&[("k", &"x".repeat(MAX_TAGS_BYTES))])).unwrap_err();
        assert_eq!(e.kind, ErrorKind::InvalidRequest);
        assert!(e.message.contains("1024"));
        assert!(tags_header(&map(&[("k", &"x".repeat(MAX_TAGS_BYTES - 10))])).is_ok());
    }
}
