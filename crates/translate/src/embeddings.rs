//! Embeddings: the OpenAI request and answer a caller uses, and what the
//! providers that serve them take and give. Text input only, float vectors.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::TranslateError;
use crate::provider::DEFAULT_AZURE_API_VERSION;
use crate::provider::{path_segment, provider_error, saturate, HttpRequest, ProviderKind, Target};

/// Gemini's `batchEmbedContents` takes at most this many texts.
pub const GEMINI_MAX_INPUTS: usize = 100;

/// What a caller is told when the target cannot embed.
pub const NOT_SUPPORTED: &str = "This model does not support embeddings.";

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingsRequest {
    pub model: String,
    pub input: Vec<String>,
    pub dimensions: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingsResponse {
    pub model: String,
    pub vectors: Vec<Vec<f32>>,
    /// Zero when the provider does not report it.
    pub prompt_tokens: u32,
}

impl ProviderKind {
    /// Whether the provider has an embeddings API this gateway speaks.
    pub fn supports_embeddings(self) -> bool {
        !matches!(self, ProviderKind::Anthropic)
    }
}

#[derive(Deserialize)]
struct WireRequest {
    model: String,
    input: WireInput,
    #[serde(default)]
    dimensions: Option<u32>,
    #[serde(default)]
    encoding_format: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireInput {
    One(String),
    Many(Vec<Value>),
}

pub fn parse_request(body: &[u8]) -> Result<EmbeddingsRequest, TranslateError> {
    let wire: WireRequest =
        serde_json::from_slice(body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))?;
    if let Some((field, _)) = wire
        .extra
        .iter()
        .find(|(k, v)| !v.is_null() && k.as_str() != "user")
    {
        return Err(TranslateError::Unsupported(format!(
            "field '{field}' is not supported yet"
        )));
    }
    if wire
        .encoding_format
        .as_deref()
        .is_some_and(|f| f != "float")
    {
        return Err(TranslateError::Unsupported(
            "encoding_format must be 'float'".into(),
        ));
    }
    let input = match wire.input {
        WireInput::One(s) => vec![s],
        WireInput::Many(items) => items
            .into_iter()
            .map(|v| match v {
                Value::String(s) => Ok(s),
                _ => Err(TranslateError::Unsupported(
                    "Only text input is supported.".into(),
                )),
            })
            .collect::<Result<_, _>>()?,
    };
    if input.is_empty() {
        return Err(TranslateError::InvalidRequest(
            "input must not be empty".into(),
        ));
    }
    Ok(EmbeddingsRequest {
        model: wire.model,
        input,
        dimensions: wire.dimensions,
    })
}

pub fn render_response(r: &EmbeddingsResponse) -> Value {
    let data: Vec<Value> = r
        .vectors
        .iter()
        .enumerate()
        .map(|(i, v)| json!({ "object": "embedding", "index": i, "embedding": v }))
        .collect();
    json!({
        "object": "list",
        "data": data,
        "model": r.model,
        "usage": { "prompt_tokens": r.prompt_tokens, "total_tokens": r.prompt_tokens },
    })
}

fn headers_with(key_header: Option<(&str, String)>) -> Vec<(String, String)> {
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    if let Some((name, value)) = key_header {
        headers.push((name.to_string(), value));
    }
    headers
}

fn to_body(v: &Value) -> Result<Vec<u8>, TranslateError> {
    serde_json::to_vec(v).map_err(|e| TranslateError::InvalidRequest(e.to_string()))
}

pub fn build_request(
    target: &Target,
    req: &EmbeddingsRequest,
) -> Result<HttpRequest, TranslateError> {
    let base = target.base_url.trim_end_matches('/');
    let openai_body = |model: Option<&str>| {
        let mut body = json!({ "input": req.input, "encoding_format": "float" });
        if let Some(m) = model {
            body["model"] = json!(m);
        }
        if let Some(d) = req.dimensions {
            body["dimensions"] = json!(d);
        }
        body
    };
    match target.kind {
        ProviderKind::OpenAi => Ok(HttpRequest {
            method: "POST",
            url: format!("{base}/embeddings"),
            headers: headers_with(
                target
                    .api_key
                    .as_ref()
                    .map(|k| ("authorization", format!("Bearer {k}"))),
            ),
            body: to_body(&openai_body(Some(&target.model)))?,
        }),
        ProviderKind::Azure => Ok(HttpRequest {
            method: "POST",
            url: format!(
                "{base}/openai/deployments/{}/embeddings?api-version={}",
                path_segment(&target.model),
                target
                    .api_version
                    .as_deref()
                    .unwrap_or(DEFAULT_AZURE_API_VERSION)
            ),
            headers: headers_with(target.api_key.as_ref().map(|k| ("api-key", k.clone()))),
            body: to_body(&openai_body(None))?,
        }),
        ProviderKind::Gemini => {
            if req.input.len() > GEMINI_MAX_INPUTS {
                return Err(TranslateError::InvalidRequest(format!(
                    "Gemini accepts at most {GEMINI_MAX_INPUTS} inputs per request."
                )));
            }
            let model = target
                .model
                .strip_prefix("models/")
                .unwrap_or(&target.model);
            let requests: Vec<Value> = req
                .input
                .iter()
                .map(|text| {
                    let mut r = json!({
                        "model": format!("models/{model}"),
                        "content": { "parts": [{ "text": text }] },
                    });
                    if let Some(d) = req.dimensions {
                        r["outputDimensionality"] = json!(d);
                    }
                    r
                })
                .collect();
            Ok(HttpRequest {
                method: "POST",
                url: format!(
                    "{base}/v1beta/models/{}:batchEmbedContents",
                    path_segment(model)
                ),
                headers: headers_with(
                    target
                        .api_key
                        .as_ref()
                        .map(|k| ("x-goog-api-key", k.clone())),
                ),
                body: to_body(&json!({ "requests": requests }))?,
            })
        }
        ProviderKind::Anthropic => Err(TranslateError::Unsupported(NOT_SUPPORTED.into())),
    }
}

fn vector(v: &Value) -> Result<Vec<f32>, TranslateError> {
    v.as_array()
        .ok_or_else(|| TranslateError::Malformed("embedding is not an array".into()))?
        .iter()
        .map(|n| {
            n.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| TranslateError::Malformed("embedding holds a non-number".into()))
        })
        .collect()
}

/// `model` is the one the target was asked for: the answer says so when the
/// provider does not name a model.
pub fn parse_response(
    kind: ProviderKind,
    status: u16,
    body: &[u8],
    model: &str,
) -> Result<EmbeddingsResponse, TranslateError> {
    if status >= 400 {
        return Err(provider_error(status, body));
    }
    let v: Value =
        serde_json::from_slice(body).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    match kind {
        ProviderKind::OpenAi | ProviderKind::Azure => {
            let mut items: Vec<(u64, Vec<f32>)> = Vec::new();
            for (i, item) in v["data"]
                .as_array()
                .ok_or_else(|| TranslateError::Malformed("response has no data".into()))?
                .iter()
                .enumerate()
            {
                let index = item["index"].as_u64().unwrap_or(i as u64);
                items.push((index, vector(&item["embedding"])?));
            }
            items.sort_by_key(|(i, _)| *i);
            Ok(EmbeddingsResponse {
                model: v["model"].as_str().unwrap_or_default().to_string(),
                vectors: items.into_iter().map(|(_, e)| e).collect(),
                prompt_tokens: saturate(v["usage"]["prompt_tokens"].as_u64().unwrap_or(0)),
            })
        }
        ProviderKind::Gemini => {
            let vectors = v["embeddings"]
                .as_array()
                .ok_or_else(|| TranslateError::Malformed("response has no embeddings".into()))?
                .iter()
                .map(|e| vector(&e["values"]))
                .collect::<Result<_, _>>()?;
            Ok(EmbeddingsResponse {
                model: model.to_string(),
                vectors,
                prompt_tokens: 0,
            })
        }
        ProviderKind::Anthropic => Err(TranslateError::Unsupported(NOT_SUPPORTED.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(kind: ProviderKind) -> Target {
        Target {
            kind,
            base_url: "https://h.example/".into(),
            api_key: Some("k-secret".into()),
            model: "emb".into(),
            api_version: None,
        }
    }

    fn req() -> EmbeddingsRequest {
        EmbeddingsRequest {
            model: "p/emb".into(),
            input: vec!["a".into(), "b".into()],
            dimensions: Some(8),
        }
    }

    fn body(r: &HttpRequest) -> Value {
        serde_json::from_slice(&r.body).unwrap()
    }

    #[test]
    fn parses_string_and_array_input() {
        let r = parse_request(br#"{"model":"m","input":"hi","user":"u"}"#).unwrap();
        assert_eq!(r.input, vec!["hi"]);
        assert_eq!(r.dimensions, None);
        let r = parse_request(
            br#"{"model":"m","input":["a","b"],"dimensions":4,"encoding_format":"float"}"#,
        )
        .unwrap();
        assert_eq!(r.input, vec!["a", "b"]);
        assert_eq!(r.dimensions, Some(4));
    }

    #[test]
    fn refuses_what_it_cannot_do() {
        for bad in [
            &br#"{"model":"m","input":"x","encoding_format":"base64"}"#[..],
            br#"{"model":"m","input":[[1,2]]}"#,
            br#"{"model":"m","input":"x","foo":1}"#,
        ] {
            assert!(
                matches!(parse_request(bad), Err(TranslateError::Unsupported(_))),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
        for bad in [
            &br#"{"model":"m","input":[]}"#[..],
            b"{",
            br#"{"model":"m"}"#,
        ] {
            assert!(matches!(
                parse_request(bad),
                Err(TranslateError::InvalidRequest(_))
            ));
        }
    }

    #[test]
    fn builds_openai_azure_and_gemini_requests() {
        let r = build_request(&target(ProviderKind::OpenAi), &req()).unwrap();
        assert_eq!(r.url, "https://h.example/embeddings");
        assert!(r
            .headers
            .contains(&("authorization".into(), "Bearer k-secret".into())));
        let b = body(&r);
        assert_eq!(b["model"], "emb");
        assert_eq!(b["input"], json!(["a", "b"]));
        assert_eq!(b["dimensions"], 8);
        assert_eq!(b["encoding_format"], "float");

        let r = build_request(&target(ProviderKind::Azure), &req()).unwrap();
        assert_eq!(
            r.url,
            "https://h.example/openai/deployments/emb/embeddings?api-version=2024-10-21"
        );
        assert!(r.headers.contains(&("api-key".into(), "k-secret".into())));
        assert!(body(&r).get("model").is_none());

        let mut t = target(ProviderKind::Gemini);
        t.model = "models/emb".into();
        let r = build_request(&t, &req()).unwrap();
        assert_eq!(
            r.url,
            "https://h.example/v1beta/models/emb:batchEmbedContents"
        );
        assert!(r
            .headers
            .contains(&("x-goog-api-key".into(), "k-secret".into())));
        let b = body(&r);
        assert_eq!(b["requests"][1]["model"], "models/emb");
        assert_eq!(b["requests"][1]["content"]["parts"][0]["text"], "b");
        assert_eq!(b["requests"][0]["outputDimensionality"], 8);
    }

    #[test]
    fn gemini_takes_at_most_a_hundred_inputs() {
        let mut r = req();
        r.input = vec!["x".into(); GEMINI_MAX_INPUTS];
        assert!(build_request(&target(ProviderKind::Gemini), &r).is_ok());
        r.input.push("x".into());
        let e = build_request(&target(ProviderKind::Gemini), &r).unwrap_err();
        assert_eq!(
            e,
            TranslateError::InvalidRequest("Gemini accepts at most 100 inputs per request.".into())
        );
        // The limit is Gemini's.
        assert!(build_request(&target(ProviderKind::OpenAi), &r).is_ok());
    }

    #[test]
    fn anthropic_cannot_embed() {
        assert!(!ProviderKind::Anthropic.supports_embeddings());
        assert!(matches!(
            build_request(&target(ProviderKind::Anthropic), &req()),
            Err(TranslateError::Unsupported(_))
        ));
    }

    #[test]
    fn parses_answers() {
        let openai = br#"{"data":[{"index":1,"embedding":[3.0]},{"index":0,"embedding":[1.5,2.0]}],"model":"emb-v1","usage":{"prompt_tokens":4,"total_tokens":4}}"#;
        for kind in [ProviderKind::OpenAi, ProviderKind::Azure] {
            let r = parse_response(kind, 200, openai, "asked").unwrap();
            assert_eq!(r.vectors, vec![vec![1.5, 2.0], vec![3.0]]);
            assert_eq!(r.prompt_tokens, 4);
            assert_eq!(r.model, "emb-v1");
        }
        let gemini = br#"{"embeddings":[{"values":[0.5]},{"values":[0.25]}]}"#;
        let r = parse_response(ProviderKind::Gemini, 200, gemini, "asked").unwrap();
        assert_eq!(r.vectors, vec![vec![0.5], vec![0.25]]);
        // The provider names no model: the one asked for is given.
        assert_eq!(r.model, "asked");
        let v = render_response(&r);
        assert_eq!(v["data"][1]["index"], 1);
        assert_eq!(v["data"][1]["embedding"][0], 0.25);
        assert_eq!(v["usage"]["prompt_tokens"], 0);
    }

    #[test]
    fn errors_and_garbage() {
        let e = parse_response(
            ProviderKind::OpenAi,
            429,
            br#"{"error":{"message":"slow"}}"#,
            "m",
        )
        .unwrap_err();
        assert!(matches!(
            e,
            TranslateError::Provider {
                status: 429,
                retryable: true,
                ..
            }
        ));
        for bad in [&b"x"[..], b"{}", br#"{"data":[{"embedding":"no"}]}"#] {
            assert!(matches!(
                parse_response(ProviderKind::OpenAi, 200, bad, "m"),
                Err(TranslateError::Malformed(_))
            ));
        }
    }
}
