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
    // reqwest strips Authorization across origins but not custom headers.
    let srv = server(&mock.url, "header", Some("X-Api-Key"));
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let err = discover_tools(&http, &srv, Some(SECRET)).await.unwrap_err();
    assert_eq!(err, UpstreamError::Protocol);
    assert!(target.received().is_empty());
    // Control: with redirects on, the target does see the request.
    let _ = discover_tools(&reqwest::Client::new(), &srv, Some(SECRET)).await;
    assert!(
        !target.received().is_empty(),
        "the test cannot see a followed redirect"
    );
}

// ------------------------------------------------------- a raw HTTP server

struct Reply {
    head: String,
    /// Body pieces, each sent after its delay.
    parts: Vec<(Duration, Vec<u8>)>,
}

fn json_reply(v: serde_json::Value) -> Reply {
    let b = v.to_string().into_bytes();
    Reply {
        head: format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n",
            b.len()
        ),
        parts: vec![(Duration::ZERO, b)],
    }
}

fn status_reply(line: &str) -> Reply {
    Reply {
        head: format!("HTTP/1.1 {line}\r\ncontent-length: 0\r\n"),
        parts: vec![],
    }
}

fn chunked(content_type: &str, parts: Vec<(Duration, Vec<u8>)>) -> Reply {
    Reply {
        head: format!(
            "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ntransfer-encoding: chunked\r\n"
        ),
        parts,
    }
}

/// Serves `script(request body)` on 127.0.0.1:0, one request per connection.
async fn raw_server(script: fn(&serde_json::Value) -> Reply) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let (head_end, len) = loop {
                    let n = sock.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).to_ascii_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length: "))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (p + 4, len);
                    }
                };
                while buf.len() < head_end + len {
                    let n = sock.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let req: serde_json::Value =
                    serde_json::from_slice(&buf[head_end..head_end + len]).unwrap_or_default();
                let reply = script(&req);
                let chunked = reply.head.contains("chunked");
                let _ = sock
                    .write_all(format!("{}connection: close\r\n\r\n", reply.head).as_bytes())
                    .await;
                for (delay, part) in reply.parts {
                    tokio::time::sleep(delay).await;
                    let out = if chunked {
                        let mut c = format!("{:x}\r\n", part.len()).into_bytes();
                        c.extend_from_slice(&part);
                        c.extend_from_slice(b"\r\n");
                        c
                    } else {
                        part
                    };
                    if sock.write_all(&out).await.is_err() {
                        return;
                    }
                }
                if chunked {
                    let _ = sock.write_all(b"0\r\n\r\n").await;
                }
                let _ = sock.shutdown().await;
            });
        }
    });
    url
}

fn init_ok(req: &serde_json::Value) -> Option<Reply> {
    match req["method"].as_str() {
        Some("initialize") => Some(json_reply(
            serde_json::json!({"jsonrpc":"2.0","id":req["id"],
            "result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"r","version":"0"}}}),
        )),
        None | Some("notifications/initialized") => Some(status_reply("202 Accepted")),
        _ => None,
    }
}

fn one_tool_page(id: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({"jsonrpc":"2.0","id":id,"result":{"tools":[tool("only","d")]}})
}

async fn discover_raw(url: &str, opts: Options) -> Result<usize, UpstreamError> {
    discover_tools_with(
        &reqwest::Client::new(),
        &server(url, "none", None),
        None,
        opts,
    )
    .await
    .map(|d| d.tools.len())
}

#[tokio::test]
async fn a_chunked_page_without_content_length_over_the_cap_is_too_large() {
    let url = raw_server(|req| {
        init_ok(req).unwrap_or_else(|| {
            chunked(
                "application/json",
                (0..5).map(|_| (Duration::ZERO, vec![b' '; 1000])).collect(),
            )
        })
    })
    .await;
    let opts = Options {
        max_body_bytes: 2000,
        ..Options::default()
    };
    assert_eq!(discover_raw(&url, opts).await, Err(UpstreamError::TooLarge));
}

#[tokio::test]
async fn a_slow_drip_body_times_out() {
    let url = raw_server(|req| {
        init_ok(req).unwrap_or_else(|| {
            chunked(
                "application/json",
                (0..20)
                    .map(|_| (Duration::from_millis(200), vec![b' ']))
                    .collect(),
            )
        })
    })
    .await;
    let opts = Options {
        timeout: Duration::from_millis(500),
        ..Options::default()
    };
    assert_eq!(discover_raw(&url, opts).await, Err(UpstreamError::Timeout));
}

#[tokio::test]
async fn an_sse_framed_page_is_parsed_with_multiline_data_and_other_ids_skipped() {
    let url = raw_server(|req| {
        init_ok(req).unwrap_or_else(|| {
            // A server-to-client request with our id comes first and is skipped;
            // the reply's data is two lines, joined with a newline (JSON whitespace).
            let good = one_tool_page(&req["id"]).to_string().replacen("{\"tools\"", "{\n\"tools\"", 1);
            let (x, y) = good.split_at(good.find('\n').unwrap());
            let body = format!(
                "data: {{\"jsonrpc\":\"2.0\",\"id\":{},\"method\":\"ping\"}}\n\nevent: message\ndata: {x}\ndata: {}\n\n",
                req["id"],
                y.trim_start_matches('\n')
            );
            chunked("text/event-stream", vec![(Duration::ZERO, body.into_bytes())])
        })
    })
    .await;
    assert_eq!(discover_raw(&url, Options::default()).await, Ok(1));
}

#[tokio::test]
async fn discovery_continues_when_initialize_is_405_or_method_not_found() {
    let a = raw_server(|req| match req["method"].as_str() {
        Some("initialize") => status_reply("405 Method Not Allowed"),
        None | Some("notifications/initialized") => status_reply("202 Accepted"),
        _ => json_reply(one_tool_page(&req["id"])),
    })
    .await;
    assert_eq!(discover_raw(&a, Options::default()).await, Ok(1));
    let b = raw_server(|req| match req["method"].as_str() {
        Some("initialize") => json_reply(serde_json::json!({"jsonrpc":"2.0","id":req["id"],
            "error":{"code":-32601,"message":"method not found"}})),
        None | Some("notifications/initialized") => status_reply("202 Accepted"),
        _ => json_reply(one_tool_page(&req["id"])),
    })
    .await;
    assert_eq!(discover_raw(&b, Options::default()).await, Ok(1));
}
