//! Image generation: the OpenAI request and answer a caller uses, and the
//! providers that serve them (OpenAI, Azure and OpenAI-compatible ones; the
//! others have no such API this gateway speaks). Only fields OpenAI documents
//! on `POST /v1/images/generations` are passed on.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::TranslateError;
use crate::provider::DEFAULT_AZURE_API_VERSION;
use crate::provider::{path_segment, provider_error, saturate, HttpRequest, ProviderKind, Target};

/// What a caller is told when the target cannot generate images.
pub const NOT_SUPPORTED: &str = "This model does not support image generation.";

/// OpenAI's limit on `prompt` for the GPT image models, in characters.
pub const MAX_PROMPT_CHARS: usize = 32_000;
/// `n` is between 1 and this.
pub const MAX_IMAGES: u32 = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct ImageRequest {
    pub model: String,
    pub prompt: String,
    pub n: Option<u32>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub background: Option<String>,
    pub output_format: Option<String>,
    pub output_compression: Option<u32>,
    pub moderation: Option<String>,
    pub response_format: Option<String>,
    pub style: Option<String>,
    pub user: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageResponse {
    pub created: u64,
    /// Each item as the provider gave it, cut to `b64_json`, `url` and
    /// `revised_prompt`.
    pub data: Vec<Value>,
    /// `background`, `output_format`, `quality` and `size` when given.
    pub info: Map<String, Value>,
    pub usage: Option<ImageUsage>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// The provider's `input_tokens_details`, passed on as it came.
    pub input_tokens_details: Option<Value>,
}

impl ProviderKind {
    /// Whether the provider has an image generation API this gateway speaks.
    pub fn supports_images(self) -> bool {
        matches!(self, ProviderKind::OpenAi | ProviderKind::Azure)
    }
}

#[derive(Deserialize)]
struct WireRequest {
    model: Option<String>,
    prompt: Option<String>,
    #[serde(default)]
    n: Option<i64>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    output_format: Option<String>,
    #[serde(default)]
    output_compression: Option<i64>,
    #[serde(default)]
    moderation: Option<String>,
    #[serde(default)]
    response_format: Option<String>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

fn invalid(m: impl Into<String>) -> TranslateError {
    TranslateError::InvalidRequest(m.into())
}

fn one_of(
    name: &str,
    value: Option<String>,
    allowed: &[&str],
) -> Result<Option<String>, TranslateError> {
    match value {
        Some(v) if !allowed.contains(&v.as_str()) => Err(invalid(format!(
            "{name} must be one of {}",
            allowed.join(", ")
        ))),
        v => Ok(v),
    }
}

pub fn parse_request(body: &[u8]) -> Result<ImageRequest, TranslateError> {
    let wire: WireRequest =
        serde_json::from_slice(body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))?;
    // `stream: false` and nulls are the defaults; anything else is a field
    // this gateway does not carry (streaming images, partial images, ...).
    if let Some((field, _)) = wire.extra.iter().find(|(k, v)| {
        let default = v.is_null() || (k.as_str() == "stream" && v.as_bool() == Some(false));
        !default
    }) {
        return Err(TranslateError::Unsupported(format!(
            "field '{field}' is not supported yet"
        )));
    }
    let model = wire
        .model
        .filter(|m| !m.is_empty())
        .ok_or_else(|| invalid("model is required"))?;
    let prompt = wire
        .prompt
        .filter(|p| !p.is_empty())
        .ok_or_else(|| invalid("prompt is required"))?;
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(invalid(format!(
            "prompt must be at most {MAX_PROMPT_CHARS} characters"
        )));
    }
    let n = match wire.n {
        None => None,
        Some(n) if (1..=i64::from(MAX_IMAGES)).contains(&n) => Some(n as u32),
        Some(_) => return Err(invalid(format!("n must be between 1 and {MAX_IMAGES}"))),
    };
    let output_compression = match wire.output_compression {
        None => None,
        Some(c) if (0..=100).contains(&c) => Some(c as u32),
        Some(_) => return Err(invalid("output_compression must be between 0 and 100")),
    };
    Ok(ImageRequest {
        model,
        prompt,
        n,
        size: wire.size,
        quality: wire.quality,
        background: one_of(
            "background",
            wire.background,
            &["transparent", "opaque", "auto"],
        )?,
        output_format: one_of(
            "output_format",
            wire.output_format,
            &["png", "jpeg", "webp"],
        )?,
        output_compression,
        moderation: one_of("moderation", wire.moderation, &["low", "auto"])?,
        response_format: one_of(
            "response_format",
            wire.response_format,
            &["url", "b64_json"],
        )?,
        style: one_of("style", wire.style, &["vivid", "natural"])?,
        user: wire.user,
    })
}

pub fn render_response(r: &ImageResponse) -> Value {
    let mut v = json!({ "created": r.created, "data": r.data });
    for (k, item) in &r.info {
        v[k] = item.clone();
    }
    if let Some(u) = &r.usage {
        v["usage"] = json!({
            "input_tokens": u.input_tokens,
            "output_tokens": u.output_tokens,
            "total_tokens": u.input_tokens.saturating_add(u.output_tokens),
        });
        if let Some(d) = &u.input_tokens_details {
            v["usage"]["input_tokens_details"] = d.clone();
        }
    }
    v
}

pub fn build_request(target: &Target, req: &ImageRequest) -> Result<HttpRequest, TranslateError> {
    let base = target.base_url.trim_end_matches('/');
    let mut body = Map::new();
    let mut put = |k: &str, v: Value| {
        body.insert(k.to_string(), v);
    };
    put("prompt", json!(req.prompt));
    for (k, v) in [
        ("size", &req.size),
        ("quality", &req.quality),
        ("background", &req.background),
        ("output_format", &req.output_format),
        ("moderation", &req.moderation),
        ("response_format", &req.response_format),
        ("style", &req.style),
        ("user", &req.user),
    ] {
        if let Some(v) = v {
            put(k, json!(v));
        }
    }
    if let Some(n) = req.n {
        put("n", json!(n));
    }
    if let Some(c) = req.output_compression {
        put("output_compression", json!(c));
    }
    let headers = |key: Option<(&str, String)>| {
        let mut h = vec![("content-type".to_string(), "application/json".to_string())];
        if let Some((name, value)) = key {
            h.push((name.to_string(), value));
        }
        h
    };
    let (url, headers) = match target.kind {
        ProviderKind::OpenAi => {
            put("model", json!(target.model));
            (
                format!("{base}/images/generations"),
                headers(
                    target
                        .api_key
                        .as_ref()
                        .map(|k| ("authorization", format!("Bearer {k}"))),
                ),
            )
        }
        ProviderKind::Azure => (
            format!(
                "{base}/openai/deployments/{}/images/generations?api-version={}",
                path_segment(&target.model),
                target
                    .api_version
                    .as_deref()
                    .unwrap_or(DEFAULT_AZURE_API_VERSION)
            ),
            headers(target.api_key.as_ref().map(|k| ("api-key", k.clone()))),
        ),
        ProviderKind::Gemini | ProviderKind::Anthropic => {
            return Err(TranslateError::Unsupported(NOT_SUPPORTED.into()))
        }
    };
    Ok(HttpRequest {
        method: "POST",
        url,
        headers,
        body: serde_json::to_vec(&Value::Object(body))
            .map_err(|e| TranslateError::InvalidRequest(e.to_string()))?,
    })
}

pub fn parse_response(
    kind: ProviderKind,
    status: u16,
    body: &[u8],
) -> Result<ImageResponse, TranslateError> {
    if !kind.supports_images() {
        return Err(TranslateError::Unsupported(NOT_SUPPORTED.into()));
    }
    if status >= 400 {
        return Err(provider_error(status, body));
    }
    let v: Value =
        serde_json::from_slice(body).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    let items = v["data"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| TranslateError::Malformed("response has no images".into()))?;
    let data = items
        .iter()
        .map(|item| {
            let mut out = Map::new();
            for k in ["b64_json", "url", "revised_prompt"] {
                if let Some(s) = item[k].as_str() {
                    out.insert(k.to_string(), json!(s));
                }
            }
            if !out.contains_key("b64_json") && !out.contains_key("url") {
                return Err(TranslateError::Malformed("an image has no content".into()));
            }
            Ok(Value::Object(out))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut info = Map::new();
    for k in ["background", "output_format", "quality", "size"] {
        if let Some(s) = v[k].as_str() {
            info.insert(k.to_string(), json!(s));
        }
    }
    let usage = v["usage"].as_object().map(|u| ImageUsage {
        input_tokens: saturate(u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0)),
        output_tokens: saturate(u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0)),
        input_tokens_details: u
            .get("input_tokens_details")
            .filter(|d| d.is_object())
            .cloned(),
    });
    Ok(ImageResponse {
        created: v["created"].as_u64().unwrap_or(0),
        data,
        info,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(kind: ProviderKind) -> Target {
        Target {
            kind,
            base_url: "https://h.example/".into(),
            api_key: Some("k-secret".into()),
            model: "img".into(),
            api_version: None,
        }
    }

    fn full() -> ImageRequest {
        parse_request(
            br#"{"model":"p/img","prompt":"a cat","n":2,"size":"1024x1024","quality":"high",
            "background":"transparent","output_format":"webp","output_compression":50,
            "moderation":"low","response_format":"b64_json","style":"vivid","user":"u"}"#,
        )
        .unwrap()
    }

    fn body(r: &HttpRequest) -> Value {
        serde_json::from_slice(&r.body).unwrap()
    }

    #[test]
    fn parses_every_documented_field() {
        let r = full();
        assert_eq!(r.model, "p/img");
        assert_eq!(r.prompt, "a cat");
        assert_eq!(r.n, Some(2));
        assert_eq!(r.size.as_deref(), Some("1024x1024"));
        assert_eq!(r.quality.as_deref(), Some("high"));
        assert_eq!(r.background.as_deref(), Some("transparent"));
        assert_eq!(r.output_format.as_deref(), Some("webp"));
        assert_eq!(r.output_compression, Some(50));
        assert_eq!(r.moderation.as_deref(), Some("low"));
        assert_eq!(r.response_format.as_deref(), Some("b64_json"));
        assert_eq!(r.style.as_deref(), Some("vivid"));
        assert_eq!(r.user.as_deref(), Some("u"));
        let min = parse_request(br#"{"model":"m","prompt":"x"}"#).unwrap();
        assert_eq!(min.n, None);
    }

    #[test]
    fn refuses_what_it_cannot_do() {
        let long = format!(r#"{{"model":"m","prompt":"{}"}}"#, "x".repeat(32_001));
        for (bad, invalid) in [
            (r#"{"model":"m","prompt":""}"#.to_string(), true),
            (r#"{"model":"m"}"#.to_string(), true),
            (r#"{"prompt":"x"}"#.to_string(), true),
            (r#"{"model":"m","prompt":"x","n":0}"#.to_string(), true),
            (r#"{"model":"m","prompt":"x","n":11}"#.to_string(), true),
            (
                r#"{"model":"m","prompt":"x","output_compression":101}"#.to_string(),
                true,
            ),
            (
                r#"{"model":"m","prompt":"x","background":"red"}"#.to_string(),
                true,
            ),
            (
                r#"{"model":"m","prompt":"x","output_format":"gif"}"#.to_string(),
                true,
            ),
            (
                r#"{"model":"m","prompt":"x","response_format":"binary"}"#.to_string(),
                true,
            ),
            (
                r#"{"model":"m","prompt":"x","moderation":"high"}"#.to_string(),
                true,
            ),
            (
                r#"{"model":"m","prompt":"x","style":"wild"}"#.to_string(),
                true,
            ),
            (long, true),
            (
                r#"{"model":"m","prompt":"x","stream":true}"#.to_string(),
                false,
            ),
            (
                r#"{"model":"m","prompt":"x","partial_images":2}"#.to_string(),
                false,
            ),
            (r#"{"model":"m","prompt":"x","foo":1}"#.to_string(), false),
        ] {
            let e = parse_request(bad.as_bytes()).unwrap_err();
            match (invalid, &e) {
                (true, TranslateError::InvalidRequest(_)) => {}
                (false, TranslateError::Unsupported(_)) => {}
                _ => panic!("{bad}: {e:?}"),
            }
        }
        // Falsy extras are accepted: stream false, nulls.
        parse_request(br#"{"model":"m","prompt":"x","stream":false,"size":null,"foo":null}"#)
            .unwrap();
    }

    #[test]
    fn an_openai_request_is_the_documented_body() {
        let out = build_request(&target(ProviderKind::OpenAi), &full()).unwrap();
        assert_eq!(out.url, "https://h.example/images/generations");
        assert!(out
            .headers
            .contains(&("authorization".into(), "Bearer k-secret".into())));
        assert_eq!(
            body(&out),
            json!({
                "model": "img", "prompt": "a cat", "n": 2, "size": "1024x1024",
                "quality": "high", "background": "transparent", "output_format": "webp",
                "output_compression": 50, "moderation": "low",
                "response_format": "b64_json", "style": "vivid", "user": "u"
            })
        );
        let min = parse_request(br#"{"model":"p/img","prompt":"x"}"#).unwrap();
        let out = build_request(&target(ProviderKind::OpenAi), &min).unwrap();
        assert_eq!(body(&out), json!({ "model": "img", "prompt": "x" }));
    }

    #[test]
    fn an_azure_request_names_the_deployment_and_leaves_out_the_model() {
        let out = build_request(&target(ProviderKind::Azure), &full()).unwrap();
        assert_eq!(
            out.url,
            format!(
                "https://h.example/openai/deployments/img/images/generations?api-version={DEFAULT_AZURE_API_VERSION}"
            )
        );
        assert!(out.headers.contains(&("api-key".into(), "k-secret".into())));
        assert!(body(&out).get("model").is_none());
        assert_eq!(body(&out)["prompt"], "a cat");
        let mut t = target(ProviderKind::Azure);
        t.api_version = Some("2030-01-01".into());
        let out = build_request(&t, &full()).unwrap();
        assert!(out.url.ends_with("?api-version=2030-01-01"));
    }

    #[test]
    fn anthropic_and_gemini_cannot_generate() {
        for kind in [ProviderKind::Anthropic, ProviderKind::Gemini] {
            assert!(!kind.supports_images());
            let e = build_request(&target(kind), &full()).unwrap_err();
            assert!(matches!(e, TranslateError::Unsupported(m) if m == NOT_SUPPORTED));
            let e = parse_response(kind, 200, b"{}").unwrap_err();
            assert!(matches!(e, TranslateError::Unsupported(_)));
        }
        assert!(ProviderKind::OpenAi.supports_images());
        assert!(ProviderKind::Azure.supports_images());
    }

    #[test]
    fn parses_and_renders_an_answer_with_usage() {
        let wire = json!({
            "created": 1700000000, "size": "1024x1024", "quality": "high",
            "background": "opaque", "output_format": "png", "junk": 1,
            "data": [{ "b64_json": "AAAA", "revised_prompt": "r", "junk": 2 }, { "url": "https://i/x.png" }],
            "usage": { "input_tokens": 10, "output_tokens": 4000, "total_tokens": 4010,
                "input_tokens_details": { "text_tokens": 10, "image_tokens": 0 } }
        });
        let r = parse_response(ProviderKind::OpenAi, 200, wire.to_string().as_bytes()).unwrap();
        assert_eq!(r.created, 1_700_000_000);
        assert_eq!(
            r.usage.as_ref().map(|u| (u.input_tokens, u.output_tokens)),
            Some((10, 4000))
        );
        let v = render_response(&r);
        assert_eq!(v["created"], 1_700_000_000u64);
        assert_eq!(
            v["data"][0],
            json!({ "b64_json": "AAAA", "revised_prompt": "r" })
        );
        assert_eq!(v["data"][1], json!({ "url": "https://i/x.png" }));
        assert_eq!(v["size"], "1024x1024");
        assert_eq!(v["quality"], "high");
        assert_eq!(v["background"], "opaque");
        assert_eq!(v["output_format"], "png");
        assert!(v.get("junk").is_none());
        assert_eq!(v["usage"]["input_tokens"], 10);
        assert_eq!(v["usage"]["output_tokens"], 4000);
        assert_eq!(v["usage"]["total_tokens"], 4010);
        assert_eq!(v["usage"]["input_tokens_details"]["text_tokens"], 10);
    }

    #[test]
    fn an_answer_without_usage_has_none() {
        let wire = json!({ "created": 5, "data": [{ "b64_json": "AA" }] });
        let r = parse_response(ProviderKind::Azure, 200, wire.to_string().as_bytes()).unwrap();
        assert!(r.usage.is_none());
        assert!(render_response(&r).get("usage").is_none());
    }

    #[test]
    fn errors_and_malformed_answers() {
        let e = parse_response(
            ProviderKind::OpenAi,
            400,
            br#"{"error":{"message":"bad prompt"}}"#,
        )
        .unwrap_err();
        assert!(
            matches!(e, TranslateError::Provider { status: 400, .. }),
            "{e:?}"
        );
        for bad in [&b"nope"[..], br#"{"created":1}"#, br#"{"data":[]}"#] {
            let e = parse_response(ProviderKind::OpenAi, 200, bad).unwrap_err();
            assert!(matches!(e, TranslateError::Malformed(_)), "{e:?}");
        }
    }
}
