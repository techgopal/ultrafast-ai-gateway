//! `POST /mcp/{server}`: the gateway as an MCP server toward agents.
//!
//! Stateless, as the 2026-07-28 spec allows: no `Mcp-Session-Id` is issued or
//! read, no handshake is needed, and every request stands alone. The
//! JSON-RPC dispatcher is written by hand (as the client toward upstreams
//! is): the whole server side is `initialize`, `ping`, `tools/list` and
//! `tools/call`, and every reply is one JSON body. `resources/*`,
//! `prompts/*` and everything else are answered "method not found"; proxying
//! them is not part of this release.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::header::{ALLOW, CACHE_CONTROL, CONTENT_TYPE, WWW_AUTHENTICATE};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use http_body_util::LengthLimitError;
use serde_json::{json, Value};

use super::call::{self, ToolCall};
use crate::access;
use crate::app::AppState;
use crate::auth::authenticate;
use crate::errors::Shape;

/// JSON-RPC and MCP error codes.
pub const CODE_PARSE: i64 = -32700;
pub const CODE_INVALID_REQUEST: i64 = -32600;
pub const CODE_METHOD_NOT_FOUND: i64 = -32601;
pub const CODE_INVALID_PARAMS: i64 = -32602;
/// A request the gateway refuses for its own reasons (a missing or bad key,
/// a limit, a budget, an unknown server).
pub const CODE_REFUSED: i64 = -32000;
pub const CODE_BUSY: i64 = -32001;

/// The protocol versions this endpoint answers `initialize` with, newest
/// first. A client asking for one of them gets it back, any other client
/// the first.
const VERSIONS: [&str; 4] = ["2026-07-28", "2025-06-18", "2025-03-26", "2024-11-05"];

/// A JSON-RPC success.
pub fn reply(status: StatusCode, id: &Value, result: Value) -> Response {
    respond(
        status,
        json!({"jsonrpc": "2.0", "id": id, "result": result}),
    )
}

/// A JSON-RPC error with the HTTP status it travels with.
pub fn reply_error(status: StatusCode, id: &Value, code: i64, message: &str) -> Response {
    respond(
        status,
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    )
}

fn respond(status: StatusCode, body: Value) -> Response {
    let mut response = (status, body.to_string()).into_response();
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Any path under `/mcp` that is not `/mcp/{server}`.
pub async fn not_found() -> Response {
    reply_error(
        StatusCode::NOT_FOUND,
        &Value::Null,
        CODE_REFUSED,
        "Unknown MCP server.",
    )
}

pub async fn serve(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    request: Request,
) -> Response {
    if request.method() != Method::POST {
        let mut response = reply_error(
            StatusCode::METHOD_NOT_ALLOWED,
            &Value::Null,
            CODE_INVALID_REQUEST,
            "Use POST.",
        );
        response
            .headers_mut()
            .insert(ALLOW, HeaderValue::from_static("POST"));
        return response;
    }
    let (parts, body) = request.into_parts();
    let snapshot = state.snapshot.load_full();
    let null = Value::Null;
    let key = match authenticate(&snapshot, &parts.headers, Shape::OpenAi) {
        Ok(key) => key,
        Err(_) => {
            let mut response = reply_error(
                StatusCode::UNAUTHORIZED,
                &null,
                CODE_REFUSED,
                "Missing or invalid API key.",
            );
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            return response;
        }
    };
    let Some(server) = snapshot.mcp_server(&name).filter(|s| s.enabled && s.usable) else {
        return not_found().await;
    };
    let tools: Vec<_> = snapshot
        .mcp_tools_of(server.id)
        .filter(|t| access::key_may_call_tool(&snapshot, &key, server, t))
        .collect();
    if tools.is_empty() {
        return reply_error(
            StatusCode::FORBIDDEN,
            &null,
            CODE_REFUSED,
            "This key may not call any tool of this MCP server.",
        );
    }
    let bytes = match axum::body::to_bytes(Body::new(body), state.mcp.limits.max_args_bytes).await {
        Ok(b) => b,
        Err(e) => {
            return if e.into_inner().is::<LengthLimitError>() {
                reply_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    &null,
                    CODE_INVALID_REQUEST,
                    "Request body is too large.",
                )
            } else {
                reply_error(
                    StatusCode::BAD_REQUEST,
                    &null,
                    CODE_INVALID_REQUEST,
                    "The request body could not be read.",
                )
            };
        }
    };
    let message: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            return reply_error(StatusCode::BAD_REQUEST, &null, CODE_PARSE, "Parse error.");
        }
    };
    if !message.is_object() {
        return reply_error(
            StatusCode::BAD_REQUEST,
            &null,
            CODE_INVALID_REQUEST,
            "The request must be one JSON-RPC object.",
        );
    }
    let Some(method) = message["method"].as_str() else {
        // A response to a request the gateway never made, or noise.
        return StatusCode::ACCEPTED.into_response();
    };
    let Some(id) = message.get("id").filter(|v| v.is_string() || v.is_number()) else {
        // A notification.
        return StatusCode::ACCEPTED.into_response();
    };
    let ok = StatusCode::OK;
    match method {
        "initialize" => {
            let asked = message["params"]["protocolVersion"].as_str();
            let version = VERSIONS
                .iter()
                .find(|v| Some(**v) == asked)
                .unwrap_or(&VERSIONS[0]);
            reply(
                ok,
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "ultrafast-gateway", "version": env!("CARGO_PKG_VERSION")},
                }),
            )
        }
        "ping" => reply(ok, id, json!({})),
        "tools/list" => {
            let listed: Vec<Value> = tools
                .iter()
                .map(|t| {
                    let mut out = json!({
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": t.input_schema,
                        "_meta": {"ultrafast": {"server": server.name}},
                    });
                    if let Some(title) = &t.title {
                        out["title"] = json!(title);
                    }
                    if let Some(schema) = &t.output_schema {
                        out["outputSchema"] = schema.clone();
                    }
                    if let Some(annotations) = &t.annotations {
                        out["annotations"] = annotations.clone();
                    }
                    out
                })
                .collect();
            reply(ok, id, json!({"tools": listed}))
        }
        "tools/call" => {
            let params = &message["params"];
            let Some(tool) = params["name"].as_str() else {
                return reply_error(ok, id, CODE_INVALID_PARAMS, "name is required.");
            };
            let arguments = match params.get("arguments") {
                None | Some(Value::Null) => json!({}),
                Some(a) if a.is_object() => a.clone(),
                Some(_) => {
                    return reply_error(
                        ok,
                        id,
                        CODE_INVALID_PARAMS,
                        "arguments must be an object.",
                    );
                }
            };
            call::run(ToolCall {
                state: &state,
                snapshot: &snapshot,
                key: &key,
                server,
                tool,
                arguments,
                id: id.clone(),
                headers: &parts.headers,
            })
            .await
        }
        _ => reply_error(ok, id, CODE_METHOD_NOT_FOUND, "Method not found."),
    }
}
