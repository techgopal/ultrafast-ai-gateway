mod common;

use common::*;
use futures::StreamExt;
use ultrafast_client::types::{FinishReason, StreamEvent, Usage};
use ultrafast_client::{ChatRequest, Client, ErrorKind, Target};

const OPENAI_STREAM: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"h\u{e9}\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"llo\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n",
    "data: [DONE]\n\n",
);

fn expected() -> Vec<StreamEvent> {
    vec![
        StreamEvent::Delta {
            text: "h\u{e9}".into(),
        },
        StreamEvent::Delta { text: "llo".into() },
        StreamEvent::Done {
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage {
                input_tokens: 3,
                output_tokens: 2,
            }),
        },
    ]
}

async fn collect(c: &Client) -> (Vec<StreamEvent>, Option<ultrafast_client::Error>) {
    let mut s = Box::pin(
        c.chat_stream(ChatRequest::new("m").user("hi"))
            .await
            .unwrap(),
    );
    let mut ok = Vec::new();
    while let Some(item) = s.next().await {
        match item {
            Ok(e) => ok.push(e),
            Err(e) => {
                assert!(s.next().await.is_none(), "nothing follows an error");
                return (ok, Some(e));
            }
        }
    }
    (ok, None)
}

#[tokio::test]
async fn gateway_stream_split_at_every_byte() {
    let bytes = OPENAI_STREAM.as_bytes();
    // One HTTP client for all the runs: building one is the slow part.
    let http = reqwest::Client::new();
    for i in 1..bytes.len() {
        let s = serve(Script::sse(vec![bytes[..i].to_vec(), bytes[i..].to_vec()])).await;
        let c = Client::new(Target::gateway(&s.url, "k")).with_http(http.clone());
        let (got, err) = collect(&c).await;
        assert!(err.is_none(), "split {i}: {err:?}");
        assert_eq!(got, expected(), "split at byte {i}");
        let r = s.only();
        assert!(r.contains("\"stream\":true"), "{r}");
        assert!(r.contains("accept: text/event-stream"), "{r}");
    }
}

#[tokio::test]
async fn anthropic_stream() {
    let body = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"c\",\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    let s = serve(Script::sse(vec![body.as_bytes().to_vec()])).await;
    let c = Client::new(Target::anthropic("k").with_base_url(&s.url));
    let (got, err) = collect(&c).await;
    assert!(err.is_none(), "{err:?}");
    assert_eq!(got[0], StreamEvent::Delta { text: "hi".into() });
    assert!(matches!(got.last(), Some(StreamEvent::Done { .. })));
}

#[tokio::test]
async fn gemini_stream_gets_its_final_event_from_finish() {
    let body = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hi\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":7,\"candidatesTokenCount\":1}}\n\n";
    let s = serve(Script::sse(vec![body.as_bytes().to_vec()])).await;
    let c = Client::new(Target::gemini("k").with_base_url(&s.url));
    let (got, err) = collect(&c).await;
    assert!(err.is_none(), "{err:?}");
    assert_eq!(got[0], StreamEvent::Delta { text: "Hi".into() });
    assert_eq!(
        got.last(),
        Some(&StreamEvent::Done {
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage {
                input_tokens: 7,
                output_tokens: 1
            })
        })
    );
    assert!(s.only().contains("streamgeneratecontent?alt=sse"));
}

#[tokio::test]
async fn an_error_event_after_text_arrives_as_text_then_a_typed_error() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"par\"},\"finish_reason\":null}]}\n\n",
        "data: {\"error\":{\"message\":\"overloaded\",\"type\":\"upstream_error\"}}\n\n",
    );
    let s = serve(Script::sse(vec![body.as_bytes().to_vec()])).await;
    let c = Client::new(Target::gateway(&s.url, "k"));
    let (got, err) = collect(&c).await;
    assert_eq!(got, vec![StreamEvent::Delta { text: "par".into() }]);
    let e = err.expect("an error after the text");
    assert_eq!(e.kind, ErrorKind::Upstream);
    assert!(e.message.contains("overloaded"));
}

#[tokio::test]
async fn a_stream_cut_after_text_is_an_error_not_a_silent_end() {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"par\"},\"finish_reason\":null}]}\n\n";
    let s = serve(Script::sse(vec![body.as_bytes().to_vec()]).cut()).await;
    let c = Client::new(Target::gateway(&s.url, "k"));
    let (got, err) = collect(&c).await;
    assert_eq!(got, vec![StreamEvent::Delta { text: "par".into() }]);
    assert_eq!(
        err.expect("truncation is an error").kind,
        ErrorKind::Network
    );
}

#[tokio::test]
async fn a_stream_that_closes_cleanly_without_done_is_an_error() {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"par\"},\"finish_reason\":null}]}\n\n";
    let s = serve(Script::sse(vec![body.as_bytes().to_vec()])).await;
    let c = Client::new(Target::gateway(&s.url, "k"));
    let (got, err) = collect(&c).await;
    assert_eq!(got.len(), 1);
    assert_eq!(err.expect("no Done is an error").kind, ErrorKind::Malformed);
}

#[tokio::test]
async fn a_refused_stream_is_an_error_before_any_event() {
    let s = serve(
        Script::json(
            429,
            r#"{"error":{"message":"slow down","type":"rate_limit_error"}}"#,
        )
        .header("retry-after", "7"),
    )
    .await;
    let c = Client::new(Target::gateway(&s.url, "k"));
    let e = c
        .chat_stream(ChatRequest::new("m").user("hi"))
        .await
        .err()
        .expect("refused");
    assert_eq!(e.kind, ErrorKind::RateLimited);
    assert_eq!(e.retry_after, Some(std::time::Duration::from_secs(7)));
}

#[tokio::test]
async fn tags_reach_a_gateway_stream_but_not_a_provider_stream() {
    let s = serve(Script::sse(vec![OPENAI_STREAM.as_bytes().to_vec()])).await;
    let c = Client::new(Target::gateway(&s.url, "k"));
    let _ = collect_tagged(&c).await;
    assert!(s.only().contains(r#"x-uf-tags: {"a":"b"}"#));

    let p = serve(Script::sse(vec![OPENAI_STREAM.as_bytes().to_vec()])).await;
    let c = Client::new(Target::openai_compatible(&p.url, "k"));
    let _ = collect_tagged(&c).await;
    assert!(!p.only().contains("x-uf-tags"));
}

async fn collect_tagged(c: &Client) -> usize {
    let s = c
        .chat_stream(ChatRequest::new("m").user("hi").tag("a", "b"))
        .await
        .unwrap();
    Box::pin(s).count().await
}
