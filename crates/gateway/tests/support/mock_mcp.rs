//! A mock MCP server: JSON-RPC over streamable HTTP (stateless, no session id)
//! on 127.0.0.1 with an OS-assigned port. Used by the MCP tests.
//!
//! Include with `#[path = "support/mock_mcp.rs"] mod mock_mcp;`.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    None,
    /// Every request answers 500.
    Status500,
    /// Every request answers 500 with this body.
    Status500Text(&'static str),
    /// Every request answers 302 to this URL.
    Redirect(String),
    /// Every request answers 200 with a body that is not JSON.
    Malformed,
    /// Every request waits far longer than any test timeout.
    Hang,
    /// `tools/list` always returns a next cursor.
    EndlessPages,
}

/// How `tools/call` goes wrong (listing is unaffected).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallMode {
    /// Echo the arguments as text content.
    Echo,
    /// A JSON-RPC error.
    RpcError,
    /// A result with `isError: true`.
    IsError,
    /// HTTP 401 on calls only.
    Http401,
    /// HTTP 500 on calls only.
    Http500,
    /// A text result of this many bytes.
    Oversize(usize),
    /// Text, image and resource content plus `structuredContent`.
    Rich,
}

/// One received request: its headers (name lowercased) and JSON body.
#[derive(Clone, Debug)]
pub struct Received {
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl Received {
    pub fn header_values(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

pub struct MockMcpBuilder {
    tools: Vec<Value>,
    page_size: usize,
    required: Option<(String, String)>,
    failure: Failure,
    ttl_ms: Option<u64>,
    call_mode: CallMode,
    call_delay: Duration,
}

struct Inner {
    tools: Mutex<Vec<Value>>,
    call_mode: CallMode,
    call_delay: Duration,
    page_size: usize,
    required: Option<(String, String)>,
    failure: Failure,
    ttl_ms: Option<u64>,
    log: Mutex<Vec<Received>>,
}

pub struct MockMcp {
    pub url: String,
    inner: Arc<Inner>,
    task: JoinHandle<()>,
}

/// A tool object as `tools/list` returns it.
pub fn tool(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}},
    })
}

impl MockMcp {
    pub fn builder() -> MockMcpBuilder {
        MockMcpBuilder {
            tools: Vec::new(),
            page_size: 100,
            required: None,
            failure: Failure::None,
            ttl_ms: None,
            call_mode: CallMode::Echo,
            call_delay: Duration::ZERO,
        }
    }

    /// Replaces the tool set; the next `tools/list` shows it (a changed upstream).
    pub fn set_tools(&self, tools: Vec<Value>) {
        *self.inner.tools.lock().unwrap() = tools;
    }

    /// Every request received so far, in order.
    pub fn received(&self) -> Vec<Received> {
        self.inner.log.lock().unwrap().clone()
    }

    pub fn received_method(&self, method: &str) -> Vec<Received> {
        self.received()
            .into_iter()
            .filter(|r| r.body["method"] == method)
            .collect()
    }
}

impl Drop for MockMcp {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MockMcpBuilder {
    pub fn tools(mut self, tools: Vec<Value>) -> Self {
        self.tools = tools;
        self
    }
    pub fn page_size(mut self, n: usize) -> Self {
        self.page_size = n.max(1);
        self
    }
    /// Requests without exactly this header value answer 401.
    pub fn require_header(mut self, name: &str, value: &str) -> Self {
        self.required = Some((name.to_ascii_lowercase(), value.to_string()));
        self
    }
    pub fn failure(mut self, f: Failure) -> Self {
        self.failure = f;
        self
    }
    pub fn call_mode(mut self, m: CallMode) -> Self {
        self.call_mode = m;
        self
    }
    /// Each `tools/call` waits this long before answering.
    pub fn call_delay(mut self, d: Duration) -> Self {
        self.call_delay = d;
        self
    }
    pub fn ttl_ms(mut self, ttl: u64) -> Self {
        self.ttl_ms = Some(ttl);
        self
    }

    pub async fn start(self) -> MockMcp {
        let inner = Arc::new(Inner {
            tools: Mutex::new(self.tools),
            call_mode: self.call_mode,
            call_delay: self.call_delay,
            page_size: self.page_size,
            required: self.required,
            failure: self.failure,
            ttl_ms: self.ttl_ms,
            log: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/mcp", post(handle).fallback(record_any))
            .fallback(record_any)
            .with_state(inner.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        MockMcp { url, inner, task }
    }
}

/// Any request that is not `POST /mcp` (a followed redirect, say) is recorded and refused.
async fn record_any(State(s): State<Arc<Inner>>, headers: HeaderMap) -> StatusCode {
    s.log.lock().unwrap().push(Received {
        headers: headers
            .iter()
            .map(|(n, v)| (n.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
            .collect(),
        body: Value::Null,
    });
    StatusCode::NOT_FOUND
}

async fn handle(State(s): State<Arc<Inner>>, headers: HeaderMap, body: String) -> Response {
    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    s.log.lock().unwrap().push(Received {
        headers: headers
            .iter()
            .map(|(n, v)| (n.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
            .collect(),
        body: parsed.clone(),
    });
    match &s.failure {
        Failure::Status500 => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Failure::Status500Text(t) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, *t).into_response()
        }
        Failure::Redirect(to) => {
            return (StatusCode::FOUND, [("location", to.as_str())]).into_response()
        }
        Failure::Malformed => return (StatusCode::OK, "<<not json").into_response(),
        Failure::Hang => {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            return StatusCode::OK.into_response();
        }
        _ => {}
    }
    if let Some((name, value)) = &s.required {
        if headers.get(name).and_then(|v| v.to_str().ok()) != Some(value.as_str()) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    let Some(id) = parsed.get("id").cloned() else {
        // A notification.
        return StatusCode::ACCEPTED.into_response();
    };
    let result = match parsed["method"].as_str().unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": parsed["params"]["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "mock-mcp", "version": "0"},
        }),
        "tools/list" => list(&s, &parsed),
        "tools/call" => {
            if !s.call_delay.is_zero() {
                tokio::time::sleep(s.call_delay).await;
            }
            match &s.call_mode {
                CallMode::Http401 => return StatusCode::UNAUTHORIZED.into_response(),
                CallMode::Http500 => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                CallMode::RpcError => {
                    return Json(json!({"jsonrpc": "2.0", "id": id,
                        "error": {"code": -32000, "message": "tool exploded"}}))
                    .into_response()
                }
                m => call(&parsed, m),
            }
        }
        "ping" => json!({}),
        _ => {
            return Json(json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": "method not found"}}))
            .into_response()
        }
    };
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

fn list(s: &Inner, req: &Value) -> Value {
    let start: usize = req["params"]["cursor"]
        .as_str()
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let mut out = json!({});
    if s.failure == Failure::EndlessPages {
        out["tools"] = json!([tool(&format!("t{start}"), "page tool")]);
        out["nextCursor"] = json!((start + 1).to_string());
    } else {
        let tools = s.tools.lock().unwrap().clone();
        let end = (start + s.page_size).min(tools.len());
        out["tools"] = Value::Array(tools[start.min(end)..end].to_vec());
        if end < tools.len() {
            out["nextCursor"] = json!(end.to_string());
        }
    }
    if let Some(t) = s.ttl_ms {
        out["ttlMs"] = json!(t);
    }
    out
}

fn call(req: &Value, mode: &CallMode) -> Value {
    let echo = req["params"]["arguments"].to_string();
    match mode {
        CallMode::IsError => {
            json!({"content": [{"type": "text", "text": "tool failed"}], "isError": true})
        }
        CallMode::Oversize(n) => {
            json!({"content": [{"type": "text", "text": "x".repeat(*n)}], "isError": false})
        }
        CallMode::Rich => json!({
            "content": [
                {"type": "text", "text": echo},
                {"type": "image", "data": "aGVsbG8=", "mimeType": "image/png"},
                {"type": "resource", "resource": {"uri": "file:///x", "text": "r"}},
            ],
            "structuredContent": {"echo": req["params"]["arguments"]},
            "isError": false,
        }),
        _ => json!({"content": [{"type": "text", "text": echo}], "isError": false}),
    }
}
