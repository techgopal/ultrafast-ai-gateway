mod common;

use common::*;
use ultrafast_client::types::{FinishReason, Usage};
use ultrafast_client::{ChatRequest, Client, Target};

fn req(model: &str) -> ChatRequest {
    ChatRequest::new(model)
        .system("be brief")
        .user("hi")
        .max_tokens(16)
        .temperature(0.5)
}

fn check(r: &ultrafast_client::types::ChatResponse) {
    assert_eq!(r.content, "hello");
    assert_eq!(r.finish_reason, Some(FinishReason::Stop));
    assert_eq!(
        r.usage,
        Some(Usage {
            input_tokens: 3,
            output_tokens: 2
        })
    );
}

#[tokio::test]
async fn gateway_chat_uses_the_openai_format_under_v1() {
    let s = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::gateway(&s.url, "uf-key"));
    check(&c.chat(req("gpt-4o")).await.unwrap());
    let r = s.only();
    assert!(r.starts_with("post /v1/chat/completions "), "{r}");
    assert!(r.contains("authorization: bearer uf-key"), "{r}");
    let body: serde_json::Value =
        serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["model"], "gpt-4o");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["max_tokens"], 16);
    assert!(body.get("stream").is_none());
}

#[tokio::test]
async fn gateway_base_url_with_trailing_slash_works() {
    let s = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::gateway(format!("{}/", s.url), "k"));
    check(&c.chat(req("m")).await.unwrap());
    assert!(s.only().starts_with("post /v1/chat/completions "));
}

#[tokio::test]
async fn openai_compatible_chat() {
    let s = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::openai_compatible(format!("{}/v1", s.url), "sk-x"));
    check(&c.chat(req("gpt-4o")).await.unwrap());
    let r = s.only();
    assert!(r.starts_with("post /v1/chat/completions "), "{r}");
    assert!(r.contains("authorization: bearer sk-x"), "{r}");
}

#[tokio::test]
async fn openai_target_base_can_be_replaced() {
    let s = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::openai("sk-x").with_base_url(format!("{}/v1", s.url)));
    check(&c.chat(req("gpt-4o")).await.unwrap());
}

#[tokio::test]
async fn anthropic_chat() {
    let s = serve(Script::json(200, ANTHROPIC_CHAT)).await;
    let c = Client::new(Target::anthropic("ak-1").with_base_url(&s.url));
    check(&c.chat(req("claude-sonnet-5")).await.unwrap());
    let r = s.only();
    assert!(r.starts_with("post /v1/messages "), "{r}");
    assert!(r.contains("x-api-key: ak-1"), "{r}");
}

#[tokio::test]
async fn gemini_chat() {
    let s = serve(Script::json(200, GEMINI_CHAT)).await;
    let c = Client::new(Target::gemini("gk-1").with_base_url(&s.url));
    check(&c.chat(req("gemini-2.0-flash")).await.unwrap());
    let r = s.only();
    assert!(
        r.starts_with("post /v1beta/models/gemini-2.0-flash:generatecontent "),
        "{r}"
    );
    assert!(r.contains("x-goog-api-key: gk-1"), "{r}");
}

#[tokio::test]
async fn azure_chat_names_the_deployment_in_the_url() {
    let s = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::azure(&s.url, "zk-1").with_api_version("2024-06-01"));
    check(&c.chat(req("my-deploy")).await.unwrap());
    let r = s.only();
    assert!(
        r.starts_with(
            "post /openai/deployments/my-deploy/chat/completions?api-version=2024-06-01 "
        ),
        "{r}"
    );
    assert!(r.contains("api-key: zk-1"), "{r}");
}

#[tokio::test]
async fn tags_go_to_a_gateway_and_never_to_a_provider() {
    let g = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::gateway(&g.url, "k"));
    c.chat(req("m").tag("team", "search")).await.unwrap();
    let r = g.only();
    assert!(r.contains(r#"x-uf-tags: {"team":"search"}"#), "{r}");

    type Make = fn(&str) -> Target;
    let cases: [(Make, &str); 4] = [
        (|u| Target::openai_compatible(u, "k"), OPENAI_CHAT),
        (|u| Target::anthropic("k").with_base_url(u), ANTHROPIC_CHAT),
        (|u| Target::gemini("k").with_base_url(u), GEMINI_CHAT),
        (|u| Target::azure(u, "k"), OPENAI_CHAT),
    ];
    for (target, body) in cases {
        let p = serve(Script::json(200, body)).await;
        let c = Client::new(target(&p.url));
        c.chat(req("m").tag("team", "search")).await.unwrap();
        let r = p.only();
        assert!(!r.contains("x-uf-tags"), "{r}");
        assert!(!r.contains("search"), "{r}");
    }
}

#[tokio::test]
async fn no_tags_means_no_header() {
    let g = serve(Script::json(200, OPENAI_CHAT)).await;
    Client::new(Target::gateway(&g.url, "k"))
        .chat(req("m"))
        .await
        .unwrap();
    assert!(!g.only().contains("x-uf-tags"));
}

#[tokio::test]
async fn tags_over_one_kib_are_refused_before_sending() {
    let g = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::gateway(&g.url, "k"));
    let e = c
        .chat(req("m").tag("k", "x".repeat(2000)))
        .await
        .unwrap_err();
    assert_eq!(e.kind, ultrafast_client::ErrorKind::InvalidRequest);
    assert!(g.requests().is_empty());
}

#[tokio::test]
async fn a_translate_request_converts() {
    let s = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::gateway(&s.url, "k"));
    let t = ultrafast_client::types::ChatRequest {
        model: "m".into(),
        messages: vec![ultrafast_client::types::Message::text(
            ultrafast_client::types::Role::User,
            "hi",
        )],
        max_tokens: None,
        temperature: None,
        top_p: None,
        stop: None,
        stream: false,
        tools: Vec::new(),
        tool_choice: None,
        parallel_tool_calls: None,
        response_format: None,
    };
    check(&c.chat(t).await.unwrap());
}

#[tokio::test]
async fn a_gateway_base_url_ending_in_v1_is_not_doubled() {
    for suffix in ["/v1", "/v1/", "/", ""] {
        let s = serve(Script::json(200, OPENAI_CHAT)).await;
        let c = Client::new(Target::gateway(format!("{}{suffix}", s.url), "k"));
        check(&c.chat(req("m")).await.unwrap());
        assert!(
            s.only().starts_with("post /v1/chat/completions "),
            "suffix {suffix:?}"
        );
    }
}

const OPENAI_TOOL_CALL: &str = r#"{"id":"c2","model":"gpt-4o","choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":9,"completion_tokens":4}}"#;

fn weather() -> ultrafast_client::Tool {
    ultrafast_client::Tool {
        name: "weather".into(),
        description: Some("Current weather".into()),
        parameters: serde_json::json!({"type":"object","properties":{"city":{"type":"string"}}}),
        strict: None,
    }
}

fn body_of(r: &str) -> serde_json::Value {
    serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap()).unwrap()
}

#[tokio::test]
async fn a_gateway_tool_call_answer_and_the_result_round_trip() {
    let s = serve(Script::json(200, OPENAI_TOOL_CALL)).await;
    let c = Client::new(Target::gateway(&s.url, "k"));
    let r = c
        .chat(
            ChatRequest::new("gpt-4o")
                .user("weather in Paris?")
                .tool(weather())
                .tool_choice(ultrafast_client::ToolChoice::Tool("weather".into()))
                .parallel_tool_calls(false),
        )
        .await
        .unwrap();
    assert_eq!(r.finish_reason, Some(FinishReason::ToolCalls));
    assert_eq!(r.content, "");
    assert_eq!(
        r.tool_calls,
        vec![ultrafast_client::ToolCall {
            id: "call_1".into(),
            name: "weather".into(),
            arguments: r#"{"city":"Paris"}"#.into()
        }]
    );
    let body = body_of(&s.only());
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "weather");
    assert_eq!(body["tool_choice"]["function"]["name"], "weather");
    assert_eq!(body["parallel_tool_calls"], false);

    // The follow-up turn carries the call and its result.
    let s = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::gateway(&s.url, "k"));
    c.chat(
        ChatRequest::new("gpt-4o")
            .user("weather in Paris?")
            .tool(weather())
            .assistant_tool_calls("", r.tool_calls)
            .tool_result("call_1", "sunny"),
    )
    .await
    .unwrap();
    let body = body_of(&s.only());
    assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "call_1");
    assert_eq!(body["messages"][2]["role"], "tool");
    assert_eq!(body["messages"][2]["tool_call_id"], "call_1");
}

#[tokio::test]
async fn strict_on_a_tool_reaches_the_wire() {
    let s = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::gateway(&s.url, "k"));
    let mut strict = weather();
    strict.strict = Some(true);
    c.chat(
        ChatRequest::new("gpt-4o")
            .user("x")
            .tool(strict)
            .tool(weather()),
    )
    .await
    .unwrap();
    let body = body_of(&s.only());
    assert_eq!(body["tools"][0]["function"]["strict"], true);
    assert!(body["tools"][1]["function"].get("strict").is_none());
}

#[tokio::test]
async fn a_direct_provider_target_gets_tools_and_images_too() {
    let req = || {
        ChatRequest::new("claude-sonnet-5")
            .user("what is in this picture?")
            .image("data:image/png;base64,AAAA")
            .unwrap()
            .tool(weather())
    };
    let s = serve(Script::json(200, ANTHROPIC_CHAT)).await;
    let c = Client::new(Target::anthropic("ak-1").with_base_url(&s.url));
    c.chat(req()).await.unwrap();
    let body = body_of(&s.only());
    assert_eq!(body["tools"][0]["name"], "weather");
    assert_eq!(body["messages"][0]["content"][1]["type"], "image");

    let s = serve(Script::json(200, OPENAI_TOOL_CALL)).await;
    let c = Client::new(Target::openai_compatible(format!("{}/v1", s.url), "sk-x"));
    let r = c.chat(req()).await.unwrap();
    assert_eq!(r.tool_calls[0].name, "weather");
    let body = body_of(&s.only());
    assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
}

#[tokio::test]
async fn response_format_reaches_a_gateway_and_each_provider_in_its_form() {
    use ultrafast_client::ResponseFormat;
    let schema = serde_json::json!({"type":"object","properties":{"a":{"type":"string"}}});
    let format = ResponseFormat::JsonSchema {
        name: "n".into(),
        schema: schema.clone(),
        strict: Some(true),
        description: None,
    };
    let s = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(Target::gateway(&s.url, "k"));
    check(
        &c.chat(req("m").response_format(format.clone()))
            .await
            .unwrap(),
    );
    let body = body_of(&s.only());
    assert_eq!(
        body["response_format"],
        serde_json::json!({"type":"json_schema","json_schema":{"name":"n","schema":schema,"strict":true}})
    );
}
