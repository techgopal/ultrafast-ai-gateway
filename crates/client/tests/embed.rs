mod common;

use common::*;
use ultrafast_client::{Client, EmbeddingsRequest, ErrorKind, Target};

const OPENAI_EMB: &str = r#"{"model":"te3","data":[{"index":1,"embedding":[0.25]},{"index":0,"embedding":[0.5]}],"usage":{"prompt_tokens":4}}"#;
const GEMINI_EMB: &str = r#"{"embeddings":[{"values":[0.5]},{"values":[0.25]}]}"#;

fn req() -> EmbeddingsRequest {
    EmbeddingsRequest::new("te3", ["a", "b"]).dimensions(1)
}

#[tokio::test]
async fn gateway_embed() {
    let s = serve(Script::json(200, OPENAI_EMB)).await;
    let c = Client::new(Target::gateway(&s.url, "uf"));
    let r = c.embed(req().tag("team", "x")).await.unwrap();
    assert_eq!(r.vectors, vec![vec![0.5], vec![0.25]]);
    assert_eq!(r.prompt_tokens, 4);
    let q = s.only();
    assert!(q.starts_with("post /v1/embeddings "), "{q}");
    assert!(q.contains("authorization: bearer uf"), "{q}");
    assert!(q.contains(r#"x-uf-tags: {"team":"x"}"#), "{q}");
    assert!(q.contains("\"dimensions\":1"), "{q}");
}

#[tokio::test]
async fn openai_and_azure_and_gemini_embed_without_tags() {
    let s = serve(Script::json(200, OPENAI_EMB)).await;
    let r = Client::new(Target::openai_compatible(format!("{}/v1", s.url), "k"))
        .embed(req().tag("a", "b"))
        .await
        .unwrap();
    assert_eq!(r.vectors.len(), 2);
    let q = s.only();
    assert!(q.starts_with("post /v1/embeddings "), "{q}");
    assert!(!q.contains("x-uf-tags"), "{q}");

    let s = serve(Script::json(200, OPENAI_EMB)).await;
    Client::new(Target::azure(&s.url, "k"))
        .embed(req())
        .await
        .unwrap();
    assert!(s
        .only()
        .starts_with("post /openai/deployments/te3/embeddings?api-version="));

    let s = serve(Script::json(200, GEMINI_EMB)).await;
    let r = Client::new(Target::gemini("k").with_base_url(&s.url))
        .embed(req().tag("a", "b"))
        .await
        .unwrap();
    assert_eq!(r.vectors, vec![vec![0.5], vec![0.25]]);
    let q = s.only();
    assert!(q.contains(":batchembedcontents"), "{q}");
    assert!(!q.contains("x-uf-tags"), "{q}");
}

#[tokio::test]
async fn anthropic_cannot_embed_and_nothing_is_sent() {
    let s = serve(Script::json(200, "{}")).await;
    let e = Client::new(Target::anthropic("k").with_base_url(&s.url))
        .embed(req())
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::InvalidRequest);
    assert!(s.requests().is_empty());
}
