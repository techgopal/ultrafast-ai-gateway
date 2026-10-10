//! Tool discovery against a mock MCP server. This file is its own test
//! binary: the log subscriber is global.

#[path = "support/mock_mcp.rs"]
mod mock_mcp;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mock_mcp::{tool, Failure, MockMcp};
use ultrafast_gateway::mcp::upstream::{
    discover_tools, discover_tools_with, Options, UpstreamError, MAX_PAGES,
};
use ultrafast_gateway::store::McpServerRow;

const SECRET: &str = "mcp-secret-never-logged-4d2f";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

fn capture() -> &'static Capture {
    static C: std::sync::OnceLock<Capture> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        let c = Capture::default();
        tracing::subscriber::set_global_default(
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .with_ansi(false)
                .with_writer(c.clone())
                .finish(),
        )
        .unwrap();
        c
    })
}

fn server(url: &str, kind: &str, header: Option<&str>) -> McpServerRow {
    McpServerRow {
        id: 1,
        name: "mock".into(),
        url: url.into(),
        auth_kind: kind.into(),
        auth_header: header.map(String::from),
        auth_secret_enc: None,
        enabled: true,
        description: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
        last_sync_at: None,
        last_sync_error: None,
    }
}

fn names(n: usize) -> Vec<serde_json::Value> {
    (0..n).map(|i| tool(&format!("tool_{i}"), "d")).collect()
}

#[tokio::test]
async fn lists_every_page_and_maps_fields() {
    let mut first = tool("alpha", "first");
    first["title"] = "Alpha".into();
    first["outputSchema"] = serde_json::json!({"type": "object"});
    first["annotations"] = serde_json::json!({"readOnlyHint": true});
    let mut tools = names(4);
    tools.insert(0, first);
    let mock = MockMcp::builder()
        .tools(tools)
        .page_size(2)
        .ttl_ms(60_000)
        .start()
        .await;
    let found = discover_tools(
        &reqwest::Client::new(),
        &server(&mock.url, "none", None),
        None,
    )
    .await
    .unwrap();
    assert_eq!(found.tools.len(), 5);
    assert_eq!(found.tools[0].name, "alpha");
    assert_eq!(found.tools[0].title.as_deref(), Some("Alpha"));
    assert_eq!(found.tools[0].description, "first");
    assert!(found.tools[0].input_schema.contains("\"object\""));
    assert!(found.tools[0]
        .output_schema
        .as_deref()
        .unwrap()
        .contains("object"));
    assert!(found.tools[0]
        .annotations
        .as_deref()
        .unwrap()
        .contains("readOnlyHint"));
    assert_eq!(found.ttl_ms, Some(60_000));
    assert_eq!(mock.received_method("tools/list").len(), 3);
    // Nothing keeps a session.
    assert!(mock
        .received()
        .iter()
        .all(|r| r.header_values("mcp-session-id").is_empty()));
}

#[tokio::test]
async fn the_auth_header_is_sent_once_per_request_and_never_logged() {
    let cap = capture();
    for (kind, header, name, value) in [
        ("bearer", None, "authorization", format!("Bearer {SECRET}")),
        ("header", Some("X-Api-Key"), "x-api-key", SECRET.to_string()),
    ] {
        let mock = MockMcp::builder()
            .tools(names(2))
            .require_header(name, &value)
            .start()
            .await;
        let found = discover_tools(
            &reqwest::Client::new(),
            &server(&mock.url, kind, header),
            Some(SECRET),
        )
        .await
        .unwrap();
        assert_eq!(found.tools.len(), 2);
        let all = mock.received();
        assert!(!all.is_empty());
        for r in &all {
            assert_eq!(r.header_values(name), vec![value.as_str()], "{kind}");
        }
        // A wrong secret is refused and the message holds neither secret nor URL.
        let err = discover_tools(
            &reqwest::Client::new(),
            &server(&mock.url, kind, header),
            Some("wrong"),
        )
        .await
        .unwrap_err();
        assert_eq!(err, UpstreamError::Auth);
        assert!(!err.to_string().contains(SECRET));
    }
    let logs = String::from_utf8(cap.0.lock().unwrap().clone()).unwrap();
    assert!(!logs.contains(SECRET), "the secret reached the log");
}

#[tokio::test]
async fn a_500_is_connect_or_protocol_and_hides_upstream_text() {
    let mock = MockMcp::builder().failure(Failure::Status500).start().await;
    let err = discover_tools(
        &reqwest::Client::new(),
        &server(&mock.url, "none", None),
        None,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, UpstreamError::Connect | UpstreamError::Protocol),
        "{err:?}"
    );
    assert!(!err.to_string().contains(&mock.url));
}

#[tokio::test]
async fn an_unreachable_server_is_connect() {
    let err = discover_tools(
        &reqwest::Client::new(),
        &server("http://127.0.0.1:1/mcp", "none", None),
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(err, UpstreamError::Connect);
}

#[tokio::test]
async fn malformed_json_is_protocol() {
    let mock = MockMcp::builder().failure(Failure::Malformed).start().await;
    let err = discover_tools(
        &reqwest::Client::new(),
        &server(&mock.url, "none", None),
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(err, UpstreamError::Protocol);
}

#[tokio::test]
async fn a_hanging_server_times_out() {
    let mock = MockMcp::builder().failure(Failure::Hang).start().await;
    let start = Instant::now();
    let err = discover_tools_with(
        &reqwest::Client::new(),
        &server(&mock.url, "none", None),
        None,
        Options {
            timeout: Duration::from_millis(300),
            ..Options::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err, UpstreamError::Timeout);
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn more_than_the_page_cap_is_too_many() {
    let mock = MockMcp::builder()
        .failure(Failure::EndlessPages)
        .start()
        .await;
    let err = discover_tools(
        &reqwest::Client::new(),
        &server(&mock.url, "none", None),
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(err, UpstreamError::TooMany);
    assert_eq!(mock.received_method("tools/list").len(), MAX_PAGES);
}

#[tokio::test]
async fn a_tool_with_a_200_byte_name_is_skipped_and_counted() {
    let mut tools = names(2);
    tools.push(tool(&"n".repeat(200), "long"));
    let mock = MockMcp::builder().tools(tools).start().await;
    let found = discover_tools(
        &reqwest::Client::new(),
        &server(&mock.url, "none", None),
        None,
    )
    .await
    .unwrap();
    assert_eq!(found.tools.len(), 2);
    assert_eq!(found.skipped, 1);
}

#[tokio::test]
async fn a_page_over_the_body_cap_is_too_large() {
    let mock = MockMcp::builder().tools(names(200)).start().await;
    let err = discover_tools_with(
        &reqwest::Client::new(),
        &server(&mock.url, "none", None),
        None,
        Options {
            max_body_bytes: 2000,
            ..Options::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err, UpstreamError::TooLarge);
}

#[tokio::test]
async fn a_huge_schema_skips_the_tool() {
    let mut big = tool("big", "d");
    big["inputSchema"] =
        serde_json::json!({"type": "object", "description": "x".repeat(300 * 1024)});
    let mut tools = names(1);
    tools.push(big);
    let mock = MockMcp::builder().tools(tools).start().await;
    let found = discover_tools(
        &reqwest::Client::new(),
        &server(&mock.url, "none", None),
        None,
    )
    .await
    .unwrap();
    assert_eq!(found.tools.len(), 1);
    assert_eq!(found.skipped, 1);
}

#[tokio::test]
async fn upstream_text_does_not_choose_the_error_kind() {
    // A 500 whose body says "unauthorized" and "timeout" stays a protocol error.
    let mock = MockMcp::builder()
        .failure(Failure::Status500Text("401 unauthorized timeout"))
        .start()
        .await;
    let err = discover_tools(
        &reqwest::Client::new(),
        &server(&mock.url, "none", None),
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(err, UpstreamError::Protocol);
}

#[tokio::test]
async fn a_redirect_is_not_followed_and_not_sent_the_secret() {
    let target = MockMcp::builder().tools(names(1)).start().await;
    let mock = MockMcp::builder()
        .failure(Failure::Redirect(target.url.clone()))
        .start()
        .await;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let err = discover_tools(&http, &server(&mock.url, "bearer", None), Some(SECRET))
        .await
        .unwrap_err();
    assert_eq!(err, UpstreamError::Protocol);
    assert!(target.received().is_empty());
}
