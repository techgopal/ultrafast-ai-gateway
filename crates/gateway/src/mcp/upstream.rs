//! Talking to an upstream MCP server: tool discovery.
//!
//! The `reqwest::Client` passed in MUST NOT follow redirects (the secret would
//! go to the redirect target); the gateway's provider client does not. A
//! redirect answer is treated as a protocol error here.
//!
//! Discovery speaks JSON-RPC over streamable HTTP itself rather than through a
//! library client so every response body is read with a size limit. It is
//! stateless: a best-effort `initialize` is sent, no session id is kept, and
//! each `tools/list` page is a request of its own.

use std::fmt;
use std::time::Duration;

use futures::StreamExt;
use reqwest::header::{HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::store::{DiscoveredTool, McpServerRow};

pub const MAX_TOOLS: usize = 2000;
pub const MAX_PAGES: usize = 20;
pub const MAX_NAME_BYTES: usize = 128;
/// A tool whose input or output schema serialises larger than this is skipped.
pub const MAX_SCHEMA_BYTES: usize = 256 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
pub const DEFAULT_MAX_DISCOVERY_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamError {
    Connect,
    Auth,
    Protocol,
    TooMany,
    Timeout,
    TooLarge,
    /// The upstream answered JSON-RPC -32602 to a tool call.
    InvalidParams,
    /// The upstream answered JSON-RPC -32601: it does not know the method.
    NotSupported,
    /// A local server that is not running (starting, waiting to be
    /// restarted, switched off).
    Unavailable,
}

impl fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Connect => "could not connect to the MCP server",
            Self::Auth => "the MCP server refused the credentials",
            Self::Protocol => "the MCP server sent an unexpected response",
            Self::TooMany => "the MCP server lists too many tools",
            Self::Timeout => "the MCP server did not answer in time",
            Self::TooLarge => "the MCP server sent a response that is too large",
            Self::InvalidParams => "the MCP server rejected the arguments",
            Self::NotSupported => "the MCP server does not support this method",
            Self::Unavailable => "the MCP server is not running",
        })
    }
}

impl std::error::Error for UpstreamError {}

/// What one discovery found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discovered {
    pub tools: Vec<DiscoveredTool>,
    /// Tools skipped: a name over `MAX_NAME_BYTES` or a schema over `MAX_SCHEMA_BYTES`.
    pub skipped: usize,
    /// `ttlMs` of the first list page, when the server sent one.
    pub ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Per request, body included.
    pub timeout: Duration,
    /// Per response body.
    pub max_body_bytes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            timeout: REQUEST_TIMEOUT,
            max_body_bytes: max_discovery_bytes(),
        }
    }
}

/// `UF_MCP_MAX_DISCOVERY_BYTES`, default 4 MiB; an unusable value reads as the default.
pub fn max_discovery_bytes() -> usize {
    std::env::var("UF_MCP_MAX_DISCOVERY_BYTES")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_DISCOVERY_BYTES)
}

pub async fn discover_tools(
    http: &reqwest::Client,
    server: &McpServerRow,
    secret: Option<&str>,
) -> Result<Discovered, UpstreamError> {
    discover_tools_with(http, server, secret, Options::default()).await
}

pub async fn discover_tools_with(
    http: &reqwest::Client,
    server: &McpServerRow,
    secret: Option<&str>,
    opts: Options,
) -> Result<Discovered, UpstreamError> {
    let auth = auth_header(&server.auth_kind, server.auth_header.as_deref(), secret)?;
    let mut conn = Conn {
        http,
        url: &server.url,
        auth: auth.as_ref(),
        trace: None,
        session: None,
        opts,
    };

    // Best effort: a stateless server may not know `initialize`. A stateful
    // one answers with a session id, which this discovery (and nothing
    // after it) sends back.
    match conn.handshake().await {
        Ok(()) | Err(UpstreamError::Protocol | UpstreamError::NotSupported) => {}
        Err(e) => return Err(e),
    }

    let conn = &conn;
    paginate(|cursor, page| {
        let params = match &cursor {
            Some(c) => json!({"cursor": c}),
            None => json!({}),
        };
        let body =
            json!({"jsonrpc": "2.0", "id": page + 1, "method": "tools/list", "params": params});
        async move {
            conn.send(body, Some(page + 1))
                .await
                .map(|r| r.result)
                .map_err(|f| f.error)
        }
    })
    .await
}

/// Reads every page of `tools/list`, as `fetch(cursor, page_number)` gets
/// them, and maps the tools the way the registry stores them. Used by the
/// HTTP and the stdio client alike.
pub async fn paginate<F, Fut>(mut fetch: F) -> Result<Discovered, UpstreamError>
where
    F: FnMut(Option<String>, u64) -> Fut,
    Fut: std::future::Future<Output = Result<Value, UpstreamError>>,
{
    let mut out = Discovered::default();
    let mut cursor: Option<String> = None;
    for page in 0..=MAX_PAGES {
        if page == MAX_PAGES {
            return Err(UpstreamError::TooMany);
        }
        let result = fetch(cursor.clone(), page as u64).await?;
        if page == 0 {
            out.ttl_ms = result["ttlMs"].as_i64().map(|t| t.max(0) as u64);
        }
        for t in result["tools"].as_array().ok_or(UpstreamError::Protocol)? {
            let name = t["name"].as_str().ok_or(UpstreamError::Protocol)?;
            let input = &t["inputSchema"];
            let output = t.get("outputSchema").filter(|v| !v.is_null());
            if !input.is_object() {
                return Err(UpstreamError::Protocol);
            }
            let (input, output) = (input.to_string(), output.map(Value::to_string));
            if name.len() > MAX_NAME_BYTES
                || input.len() > MAX_SCHEMA_BYTES
                || output.as_ref().is_some_and(|o| o.len() > MAX_SCHEMA_BYTES)
            {
                out.skipped += 1;
                continue;
            }
            if out.tools.len() >= MAX_TOOLS {
                return Err(UpstreamError::TooMany);
            }
            out.tools.push(DiscoveredTool {
                name: name.to_string(),
                title: t["title"].as_str().map(String::from),
                description: t["description"].as_str().unwrap_or_default().to_string(),
                input_schema: input,
                output_schema: output,
                annotations: t
                    .get("annotations")
                    .filter(|v| !v.is_null())
                    .map(Value::to_string),
            });
        }
        cursor = result["nextCursor"].as_str().map(String::from);
        if cursor.is_none() {
            break;
        }
    }
    Ok(out)
}

fn auth_header(
    kind: &str,
    header: Option<&str>,
    secret: Option<&str>,
) -> Result<Option<(HeaderName, HeaderValue)>, UpstreamError> {
    let Some(secret) = secret else {
        return Ok(None);
    };
    let (name, value) = match kind {
        "bearer" => (AUTHORIZATION, Zeroizing::new(format!("Bearer {secret}"))),
        "header" => {
            let name = header
                .and_then(|n| HeaderName::from_bytes(n.as_bytes()).ok())
                .ok_or(UpstreamError::Protocol)?;
            (name, Zeroizing::new(secret.to_string()))
        }
        _ => return Ok(None),
    };
    // Note: the HeaderValue and reqwest's copies of the secret are not zeroized.
    let mut value = HeaderValue::from_str(&value).map_err(|_| UpstreamError::Auth)?;
    value.set_sensitive(true);
    Ok(Some((name, value)))
}

/// Where a call goes and how it signs in.
pub struct Target<'a> {
    pub url: &'a str,
    pub auth_kind: &'a str,
    pub auth_header: Option<&'a str>,
    pub secret: Option<&'a str>,
}

/// `tools/call` on an upstream: see `call_method`.
pub async fn call_tool(
    http: &reqwest::Client,
    target: &Target<'_>,
    tool: &str,
    arguments: &Value,
    traceparent: Option<&str>,
    opts: Options,
) -> Result<Value, UpstreamError> {
    let params = json!({"name": tool, "arguments": arguments});
    call_method(http, target, "tools/call", &params, traceparent, opts).await
}

/// One method on an upstream: a request with no handshake and no session,
/// its answer read up to `opts.max_body_bytes` within `opts.timeout`.
/// Returns the `result` object as the upstream sent it. `traceparent` is
/// forwarded as it is. A JSON-RPC error -32602 is `InvalidParams`, -32601
/// `NotSupported`, any other `Protocol`: the upstream's text is not the
/// caller's to read.
///
/// An upstream that insists on a session answers 400 or 404 to a request
/// without one. Then, once, the gateway does the handshake itself, keeps
/// the `Mcp-Session-Id` it is given for this one call, resends the request
/// with it and forgets it. Nothing is stored between calls.
pub async fn call_method(
    http: &reqwest::Client,
    target: &Target<'_>,
    method: &str,
    params: &Value,
    traceparent: Option<&str>,
    opts: Options,
) -> Result<Value, UpstreamError> {
    let auth = auth_header(target.auth_kind, target.auth_header, target.secret)?;
    let trace = traceparent.and_then(|t| HeaderValue::from_str(t).ok());
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let mut conn = Conn {
        http,
        url: target.url,
        auth: auth.as_ref(),
        trace: trace.as_ref(),
        session: None,
        opts,
    };
    match conn.send(body.clone(), Some(1)).await {
        Ok(reply) => Ok(reply.result),
        Err(Failed {
            status: Some(400 | 404),
            ..
        }) => {
            // The time already used is not given back: the whole sequence
            // stays within one timeout.
            conn.handshake().await?;
            if conn.session.is_none() {
                return Err(UpstreamError::Protocol);
            }
            conn.send(body, Some(1))
                .await
                .map(|r| r.result)
                .map_err(|f| f.error)
        }
        Err(f) => Err(f.error),
    }
}

/// Why a request failed, and the HTTP status when the upstream sent one.
struct Failed {
    error: UpstreamError,
    status: Option<u16>,
}

impl From<UpstreamError> for Failed {
    fn from(error: UpstreamError) -> Self {
        Self {
            error,
            status: None,
        }
    }
}

struct Reply {
    result: Value,
    /// `Mcp-Session-Id` of the response, when it had one.
    session: Option<HeaderValue>,
}

/// The connection of one call or one discovery: where it goes, how it
/// signs in, and the session an upstream gave it (never kept longer).
struct Conn<'a> {
    http: &'a reqwest::Client,
    url: &'a str,
    auth: Option<&'a (HeaderName, HeaderValue)>,
    trace: Option<&'a HeaderValue>,
    session: Option<HeaderValue>,
    opts: Options,
}

impl Conn<'_> {
    /// `initialize` and `notifications/initialized`; keeps a session id the
    /// upstream answered with.
    async fn handshake(&mut self) -> Result<(), UpstreamError> {
        let init = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                       "clientInfo": {"name": "ultrafast-gateway", "version": env!("CARGO_PKG_VERSION")}}});
        let reply = self.send(init, Some(0)).await;
        if let Ok(r) = &reply {
            self.session = r.session.clone().map(|mut v| {
                v.set_sensitive(true);
                v
            });
        }
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let _ = self.send(note, None).await;
        reply.map(|_| ()).map_err(|f| f.error)
    }

    /// One JSON-RPC POST. With `want_id` the reply must carry that id and a
    /// `result`, which is returned; without it (a notification) nothing is
    /// read. `Mcp-Method` and `Mcp-Name` name the request in its headers.
    async fn send(&self, body: Value, want_id: Option<u64>) -> Result<Reply, Failed> {
        let opts = self.opts;
        let mut req = self
            .http
            .post(self.url)
            .timeout(opts.timeout)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream");
        if let Some(m) = body["method"].as_str() {
            req = req.header("mcp-method", m);
            if m == "tools/call" {
                if let Some(n) = body["params"]["name"]
                    .as_str()
                    .and_then(|n| HeaderValue::from_str(n).ok())
                {
                    req = req.header("mcp-name", n);
                }
            }
        }
        if let Some(t) = self.trace {
            req = req.header("traceparent", t.clone());
        }
        if let Some(s) = &self.session {
            req = req.header("mcp-session-id", s.clone());
        }
        let mut req = req.json(&body);
        if let Some((n, v)) = self.auth {
            req = req.header(n.clone(), v.clone());
        }
        let resp = req.send().await.map_err(map_reqwest)?;
        let status = resp.status();
        let failed = |error| Failed {
            error,
            status: Some(status.as_u16()),
        };
        if status == 401 || status == 403 {
            return Err(failed(UpstreamError::Auth));
        }
        if !status.is_success() {
            return Err(failed(UpstreamError::Protocol));
        }
        let session = resp.headers().get("mcp-session-id").cloned();
        let Some(id) = want_id else {
            return Ok(Reply {
                result: Value::Null,
                session,
            });
        };
        let result = read_reply(resp, id, opts).await?;
        Ok(Reply { result, session })
    }
}

/// The reply with `id` in a response, read within the body cap.
async fn read_reply(resp: reqwest::Response, id: u64, opts: Options) -> Result<Value, Failed> {
    if resp
        .content_length()
        .is_some_and(|n| n > opts.max_body_bytes as u64)
    {
        return Err(UpstreamError::TooLarge.into());
    }
    let sse = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    let mut seen = 0usize;
    let mut stream = resp.bytes_stream();
    let reply = if sse {
        // Events are parsed as they arrive; the matching reply ends the read.
        let mut pending: Vec<u8> = Vec::new();
        let mut data = String::new();
        let mut found = None;
        'read: while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_reqwest)?;
            seen += chunk.len();
            if seen > opts.max_body_bytes {
                return Err(UpstreamError::TooLarge.into());
            }
            pending.extend_from_slice(&chunk);
            while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                let raw: Vec<u8> = pending.drain(..=end).collect();
                let line = std::str::from_utf8(&raw).map_err(|_| UpstreamError::Protocol)?;
                let line = line.trim_end_matches(['\n', '\r']);
                if let Some(d) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(d.strip_prefix(' ').unwrap_or(d));
                } else if line.is_empty() && !data.is_empty() {
                    let event = serde_json::from_str::<Value>(&data).ok();
                    data.clear();
                    if let Some(m) = event.filter(|m| m["id"] == id && m.get("method").is_none()) {
                        found = Some(m);
                        break 'read;
                    }
                }
            }
        }
        if found.is_none() && !data.is_empty() {
            found = serde_json::from_str::<Value>(&data)
                .ok()
                .filter(|m| m["id"] == id && m.get("method").is_none());
        }
        found.ok_or(UpstreamError::Protocol)?
    } else {
        let mut buf = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_reqwest)?;
            if buf.len() + chunk.len() > opts.max_body_bytes {
                return Err(UpstreamError::TooLarge.into());
            }
            buf.extend_from_slice(&chunk);
        }
        let message: Value = serde_json::from_slice(&buf).map_err(|_| UpstreamError::Protocol)?;
        if message["id"] != id || message.get("method").is_some() {
            return Err(UpstreamError::Protocol.into());
        }
        message
    };
    reply_result(reply).map_err(Failed::from)
}

/// The `result` of a JSON-RPC reply, or what its `error` means.
pub(crate) fn reply_result(mut reply: Value) -> Result<Value, UpstreamError> {
    if let Some(error) = reply.get("error") {
        return Err(match error["code"].as_i64() {
            Some(-32602) => UpstreamError::InvalidParams,
            Some(-32601) => UpstreamError::NotSupported,
            _ => UpstreamError::Protocol,
        });
    }
    match reply.get_mut("result") {
        Some(r) if r.is_object() => Ok(r.take()),
        _ => Err(UpstreamError::Protocol),
    }
}

/// Sorted by the error's kind, never by its text.
fn map_reqwest(e: reqwest::Error) -> Failed {
    let error = classify(&e);
    Failed {
        error,
        status: None,
    }
}

fn classify(e: &reqwest::Error) -> UpstreamError {
    if e.is_timeout() {
        UpstreamError::Timeout
    } else if e.is_connect() || e.is_request() {
        UpstreamError::Connect
    } else {
        UpstreamError::Protocol
    }
}
