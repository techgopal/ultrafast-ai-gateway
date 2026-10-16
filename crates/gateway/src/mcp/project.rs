//! Projection: what of a tool's result an agent is given.
//!
//! A tool that answers with a whole API response costs the agent tokens it
//! does not need. An admin gives the tool a projection: JSONPath queries of
//! what to `keep`, of what to `drop`, the most items an array may keep and the
//! most bytes a text may have. It is applied to the text blocks that hold JSON
//! and to `structuredContent`, before the guardrails read the result and
//! before its size is recorded. A result it changed says so:
//! `_meta.ultrafast.projected` is `true`, and `truncated` when a limit cut it.
//! A projection that removes everything leaves `{}`, never an empty body.

use std::collections::HashSet;

use serde_json::{json, Map, Value};
use serde_json_path::JsonPath;

/// The most queries a list may hold, and the longest one.
pub const MAX_PATHS: usize = 20;
pub const MAX_PATH_BYTES: usize = 256;
pub const MAX_ITEMS_LIMIT: u64 = 100_000;
pub const MAX_BYTES_LIMIT: u64 = 16 * 1024 * 1024;
/// What a text cut by `max_bytes` ends with.
pub const TRUNCATED: &str = "\n[truncated by the gateway]";

/// A projection as an admin writes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spec {
    pub keep: Vec<String>,
    pub drop: Vec<String>,
    pub max_items: Option<u64>,
    pub max_bytes: Option<u64>,
}

impl Spec {
    /// Nothing is asked for.
    pub fn is_empty(&self) -> bool {
        self.keep.is_empty()
            && self.drop.is_empty()
            && self.max_items.is_none()
            && self.max_bytes.is_none()
    }

    /// Checks every query and limit; the error is for a field of the request
    /// (`keep`, `drop`, `max_items`, `max_bytes`) and never repeats a query.
    pub fn check(&self) -> Result<(), (&'static str, String)> {
        for (field, list) in [("keep", &self.keep), ("drop", &self.drop)] {
            if list.len() > MAX_PATHS {
                return Err((field, format!("at most {MAX_PATHS} queries")));
            }
            for q in list {
                if q.is_empty() || q.len() > MAX_PATH_BYTES {
                    return Err((field, format!("every query is 1 to {MAX_PATH_BYTES} bytes")));
                }
                if JsonPath::parse(q).is_err() {
                    return Err((field, "a query is not a valid JSONPath".to_string()));
                }
            }
        }
        if self
            .max_items
            .is_some_and(|n| n == 0 || n > MAX_ITEMS_LIMIT)
        {
            return Err(("max_items", format!("must be from 1 to {MAX_ITEMS_LIMIT}")));
        }
        if self
            .max_bytes
            .is_some_and(|n| n == 0 || n > MAX_BYTES_LIMIT)
        {
            return Err(("max_bytes", format!("must be from 1 to {MAX_BYTES_LIMIT}")));
        }
        Ok(())
    }
}

/// A projection with its queries parsed, ready to run on every call.
#[derive(Debug, Clone)]
pub struct Projection {
    keep: Vec<JsonPath>,
    drop: Vec<JsonPath>,
    max_items: Option<usize>,
    max_bytes: Option<usize>,
}

/// What applying a projection did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    pub changed: bool,
    pub truncated: bool,
    /// The size of the content before and after, in bytes.
    pub before: usize,
    pub after: usize,
}

impl Projection {
    /// `None` when a query does not parse (the spec was checked when it was
    /// stored, so this is a damaged row).
    pub fn compile(spec: &Spec) -> Option<Self> {
        let parse = |list: &[String]| -> Option<Vec<JsonPath>> {
            list.iter().map(|q| JsonPath::parse(q).ok()).collect()
        };
        Some(Self {
            keep: parse(&spec.keep)?,
            drop: parse(&spec.drop)?,
            max_items: spec.max_items.and_then(|n| usize::try_from(n).ok()),
            max_bytes: spec.max_bytes.and_then(|n| usize::try_from(n).ok()),
        })
    }

    /// Projects a JSON value.
    fn value(&self, value: Value, truncated: &mut bool) -> Value {
        let mut value = value;
        if !self.keep.is_empty() {
            let mut kept = HashSet::new();
            for path in &self.keep {
                for node in path.query_located(&value).iter() {
                    kept.insert(node.location().to_json_pointer());
                }
            }
            value = prune(&value, "", &kept).unwrap_or_else(|| Value::Object(Map::new()));
        }
        if !self.drop.is_empty() {
            let mut dropped = HashSet::new();
            for path in &self.drop {
                for node in path.query_located(&value).iter() {
                    dropped.insert(node.location().to_json_pointer());
                }
            }
            // Dropping the root drops everything.
            value = if dropped.contains("") {
                Value::Object(Map::new())
            } else {
                remove(value, "", &dropped)
            };
        }
        if let Some(n) = self.max_items {
            value = limit_items(value, n, truncated);
        }
        value
    }

    /// Text cut to the byte limit, at a character boundary.
    fn text(&self, text: String, truncated: &mut bool) -> String {
        match self.max_bytes {
            Some(max) if text.len() > max => {
                *truncated = true;
                let mut end = max;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}{TRUNCATED}", &text[..end])
            }
            _ => text,
        }
    }

    /// Applies the projection to a result (`content`, `structuredContent`),
    /// in place, and marks it when it changed.
    pub fn apply(&self, result: &mut Value) -> Outcome {
        let before = serde_json::to_vec(result).map_or(0, |v| v.len());
        let original = result.clone();
        let mut truncated = false;
        if let Some(blocks) = result["content"].as_array_mut() {
            for block in blocks {
                if block["type"] != "text" {
                    continue;
                }
                let Some(text) = block["text"].as_str().map(String::from) else {
                    continue;
                };
                // Text that is JSON is projected; any other text only cut.
                let projected = match serde_json::from_str::<Value>(&text) {
                    Ok(v) if v.is_object() || v.is_array() => {
                        self.value(v, &mut truncated).to_string()
                    }
                    _ => text,
                };
                block["text"] = Value::String(self.text(projected, &mut truncated));
            }
        }
        if let Some(structured) = result
            .get("structuredContent")
            .filter(|v| !v.is_null())
            .cloned()
        {
            let projected = self.value(structured, &mut truncated);
            let too_big = self
                .max_bytes
                .is_some_and(|max| projected.to_string().len() > max);
            if too_big {
                // A cut structure would not be one: the text blocks carry the answer.
                truncated = true;
                if let Some(o) = result.as_object_mut() {
                    o.remove("structuredContent");
                }
            } else {
                result["structuredContent"] = projected;
            }
        }
        let changed = *result != original;
        let after = serde_json::to_vec(result).map_or(0, |v| v.len());
        if changed {
            let mut meta = json!({"projected": true});
            if truncated {
                meta["truncated"] = Value::Bool(true);
            }
            if let Some(o) = result.as_object_mut() {
                let entry = o.entry("_meta").or_insert_with(|| json!({}));
                entry["ultrafast"] = meta;
            }
        }
        Outcome {
            changed,
            truncated,
            before,
            after,
        }
    }
}

/// The JSON pointer of a child.
fn child(here: &str, token: &str) -> String {
    format!("{here}/{}", token.replace('~', "~0").replace('/', "~1"))
}

/// The part of `value` the pointers select (a selected node is kept whole,
/// and so is everything below it); `None` when nothing in it is selected.
fn prune(value: &Value, here: &str, kept: &HashSet<String>) -> Option<Value> {
    if kept.contains(here) {
        return Some(value.clone());
    }
    let below = format!("{here}/");
    if !kept.iter().any(|p| p.starts_with(&below)) {
        return None;
    }
    match value {
        Value::Object(map) => {
            let out: Map<String, Value> = map
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), prune(v, &child(here, k), kept)?)))
                .collect();
            (!out.is_empty()).then_some(Value::Object(out))
        }
        Value::Array(items) => {
            let out: Vec<Value> = items
                .iter()
                .enumerate()
                .filter_map(|(i, v)| prune(v, &child(here, &i.to_string()), kept))
                .collect();
            (!out.is_empty()).then_some(Value::Array(out))
        }
        _ => None,
    }
}

/// `value` without the nodes the pointers select.
fn remove(value: Value, here: &str, dropped: &HashSet<String>) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(k, _)| !dropped.contains(&child(here, k)))
                .map(|(k, v)| {
                    let v = remove(v, &child(here, &k), dropped);
                    (k, v)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .enumerate()
                .filter(|(i, _)| !dropped.contains(&child(here, &i.to_string())))
                .map(|(i, v)| remove(v, &child(here, &i.to_string()), dropped))
                .collect(),
        ),
        other => other,
    }
}

/// Every array cut to `n` items.
fn limit_items(value: Value, n: usize, truncated: &mut bool) -> Value {
    match value {
        Value::Array(mut items) => {
            if items.len() > n {
                items.truncate(n);
                *truncated = true;
            }
            Value::Array(
                items
                    .into_iter()
                    .map(|v| limit_items(v, n, truncated))
                    .collect(),
            )
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, limit_items(v, n, truncated)))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(keep: &[&str], drop: &[&str], items: Option<u64>, bytes: Option<u64>) -> Projection {
        let s = Spec {
            keep: keep.iter().map(|s| (*s).to_string()).collect(),
            drop: drop.iter().map(|s| (*s).to_string()).collect(),
            max_items: items,
            max_bytes: bytes,
        };
        s.check().unwrap();
        Projection::compile(&s).unwrap()
    }

    fn text_result(value: &Value) -> Value {
        json!({"content": [{"type": "text", "text": value.to_string()}], "isError": false})
    }

    fn text_of(result: &Value) -> Value {
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    fn sample() -> Value {
        json!({
            "total": 3,
            "items": [
                {"id": 1, "name": "a", "body": "long text", "meta": {"x": 1}},
                {"id": 2, "name": "b", "body": "long text", "meta": {"x": 2}},
                {"id": 3, "name": "c", "body": "long text", "meta": {"x": 3}},
            ],
            "links": {"next": "u", "prev": "v"},
        })
    }

    #[test]
    fn keep_selects_nested_nodes_and_keeps_the_shape() {
        let p = spec(
            &["$.items[*].id", "$.items[*].name", "$.total"],
            &[],
            None,
            None,
        );
        let mut r = text_result(&sample());
        let out = p.apply(&mut r);
        assert!(out.changed && !out.truncated);
        assert!(out.after < out.before);
        assert_eq!(
            text_of(&r),
            json!({"total": 3, "items": [{"id": 1, "name": "a"}, {"id": 2, "name": "b"}, {"id": 3, "name": "c"}]})
        );
        assert_eq!(r["_meta"]["ultrafast"]["projected"], true);
        assert!(r["_meta"]["ultrafast"].get("truncated").is_none());
    }

    #[test]
    fn drop_removes_nodes_and_array_items() {
        let p = spec(&[], &["$..body", "$.links", "$.items[1]"], None, None);
        let mut r = text_result(&sample());
        p.apply(&mut r);
        assert_eq!(
            text_of(&r),
            json!({"total": 3, "items": [
                {"id": 1, "name": "a", "meta": {"x": 1}},
                {"id": 3, "name": "c", "meta": {"x": 3}},
            ]})
        );
    }

    #[test]
    fn keep_runs_before_drop() {
        let p = spec(
            &["$.items[*]"],
            &["$.items[*].meta", "$.items[*].body"],
            None,
            None,
        );
        let mut r = text_result(&sample());
        p.apply(&mut r);
        assert_eq!(
            text_of(&r),
            json!({"items": [{"id": 1, "name": "a"}, {"id": 2, "name": "b"}, {"id": 3, "name": "c"}]})
        );
    }

    #[test]
    fn max_items_cuts_every_array_and_says_so() {
        let p = spec(&[], &[], Some(2), None);
        let mut r = text_result(&sample());
        let out = p.apply(&mut r);
        assert!(out.truncated);
        assert_eq!(text_of(&r)["items"].as_array().unwrap().len(), 2);
        assert_eq!(r["_meta"]["ultrafast"]["truncated"], true);
        // Not over the limit: untouched, no marker.
        let mut small = text_result(&json!({"a": [1, 2]}));
        assert!(!p.apply(&mut small).changed);
        assert!(small.get("_meta").is_none());
    }

    #[test]
    fn max_bytes_cuts_text_at_a_character_and_marks_the_cut() {
        let p = spec(&[], &[], None, Some(5));
        let mut r = json!({"content": [{"type": "text", "text": "héllo wörld"}], "isError": false});
        let out = p.apply(&mut r);
        assert!(out.truncated);
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(
            text.starts_with("héll") && text.ends_with(TRUNCATED),
            "{text}"
        );
        assert_eq!(r["_meta"]["ultrafast"]["projected"], true);
        // A structured result too big for the limit is left out; the text carries it.
        let mut r = json!({"content": [{"type": "text", "text": "ok"}],
                           "structuredContent": {"k": "a long value"}, "isError": false});
        p.apply(&mut r);
        assert!(r.get("structuredContent").is_none());
        assert_eq!(r["content"][0]["text"], "ok");
    }

    #[test]
    fn a_projection_that_removes_everything_leaves_an_object_and_the_marker() {
        for p in [
            spec(&["$.nothing"], &[], None, None),
            spec(&[], &["$"], None, None),
        ] {
            let mut r = text_result(&sample());
            p.apply(&mut r);
            assert_eq!(r["content"][0]["text"], "{}");
            assert_eq!(r["_meta"]["ultrafast"]["projected"], true);
        }
    }

    #[test]
    fn structured_content_is_projected_and_other_blocks_and_plain_text_pass() {
        let p = spec(&["$.total"], &[], None, None);
        let mut r = json!({
            "content": [
                {"type": "text", "text": "not json at all"},
                {"type": "image", "data": "xx", "mimeType": "image/png"},
            ],
            "structuredContent": sample(),
            "isError": false,
        });
        p.apply(&mut r);
        assert_eq!(r["content"][0]["text"], "not json at all");
        assert_eq!(r["content"][1]["type"], "image");
        assert_eq!(r["structuredContent"], json!({"total": 3}));
        // A scalar JSON text is not projected.
        let mut r = json!({"content": [{"type": "text", "text": "42"}], "isError": false});
        assert!(!p.apply(&mut r).changed);
    }

    #[test]
    fn a_projection_that_changes_nothing_leaves_the_result_as_it_was() {
        let p = spec(&["$"], &[], Some(100), Some(1_000_000));
        let mut r = text_result(&sample());
        let before = r.clone();
        assert!(!p.apply(&mut r).changed);
        assert_eq!(r, before);
    }

    #[test]
    fn queries_and_limits_are_checked() {
        let bad = |s: Spec| s.check().unwrap_err().0;
        assert_eq!(
            bad(Spec {
                keep: vec!["$[".into()],
                ..Spec::default()
            }),
            "keep"
        );
        assert_eq!(
            bad(Spec {
                drop: vec![String::new()],
                ..Spec::default()
            }),
            "drop"
        );
        assert_eq!(
            bad(Spec {
                keep: vec!["$.a".into(); 21],
                ..Spec::default()
            }),
            "keep"
        );
        assert_eq!(
            bad(Spec {
                max_items: Some(0),
                ..Spec::default()
            }),
            "max_items"
        );
        assert_eq!(
            bad(Spec {
                max_bytes: Some(MAX_BYTES_LIMIT + 1),
                ..Spec::default()
            }),
            "max_bytes"
        );
        assert!(Spec::default().is_empty());
        assert!(Spec {
            keep: vec!["$..a".into()],
            ..Spec::default()
        }
        .check()
        .is_ok());
    }
}
