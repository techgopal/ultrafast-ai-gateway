//! `POST /mcp/{server}`: the gateway as an MCP server toward agents.
//!
//! Stateless, as the 2026-07-28 spec allows: no `Mcp-Session-Id` is issued or
//! read, no handshake is needed, and every request stands alone. The
//! JSON-RPC dispatcher is written by hand (as the client toward upstreams
//! is): `initialize`, `ping`, `tools/list`, `tools/call`, and the four
//! methods in `call::PASSED_METHODS` (`resources/list`, `resources/read`,
//! `prompts/list`, `prompts/get`), which are passed to the upstream with
//! the same authentication and caps. Every reply is one JSON body. Anything
//! else is answered "method not found".

use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::header::{ALLOW, CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use http_body_util::LengthLimitError;
use serde_json::{json, Value};

use super::call::{self, ToolCall};
use crate::access;
use crate::app::AppState;
use crate::auth::authenticate;
use crate::errors::Shape;
use crate::snapshot::{SnapKey, Snapshot};

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
    entry(state, Some(name), request).await
}

/// `POST /mcp`: every tool the key may call, as `server__tool`.
pub async fn serve_combined(State(state): State<Arc<AppState>>, request: Request) -> Response {
    entry(state, None, request).await
}

async fn entry(state: Arc<AppState>, name: Option<String>, request: Request) -> Response {
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
    dispatch(
        &state,
        &snapshot,
        &key,
        name.as_deref(),
        &parts.headers,
        body,
    )
    .await
}

/// The part of a request after the caller is known: the server (`None`: all
/// of them, namespaced), the tools the caller may use, the body, and the
/// method. The console's playground comes here with the signed-in user in
/// place of a key.
pub(crate) async fn dispatch(
    state: &Arc<AppState>,
    snapshot: &Arc<Snapshot>,
    key: &SnapKey,
    name: Option<&str>,
    headers: &HeaderMap,
    body: Body,
) -> Response {
    let null = Value::Null;
    // One server, with the tools of it the key may call; or none, for the
    // combined endpoint.
    let single = match name {
        Some(name) => {
            let Some(server) = snapshot.mcp_server(name).filter(|s| s.enabled && s.usable) else {
                return not_found().await;
            };
            let tools: Vec<_> = snapshot
                .mcp_tools_of(server.id)
                .filter(|t| access::key_may_call_tool(snapshot, key, server, t))
                .collect();
            if tools.is_empty() {
                // The same answer as an unknown server: a key learns nothing
                // about servers it may not use.
                return not_found().await;
            }
            Some((server, tools))
        }
        None => None,
    };
    let bytes = match axum::body::to_bytes(body, state.mcp.limits.max_args_bytes).await {
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
                    "capabilities": {
                        "tools": {"listChanged": false},
                        "resources": {},
                        "prompts": {},
                    },
                    "serverInfo": {"name": "ultrafast-gateway", "version": env!("CARGO_PKG_VERSION")},
                }),
            )
        }
        "ping" => reply(ok, id, json!({})),
        "tools/list" => {
            // Listing counts as a request; nothing is upstream, so a permit
            // is never given back.
            let subjects = snapshot.subjects_of(Some(key.id), key.user_id, key.team_id);
            let _permit = match state.rate.acquire(&subjects, 0, Instant::now()) {
                Ok(permit) => permit,
                Err(refusal) => {
                    state.metrics.rate_limited(refusal.limit_name);
                    let mut response = reply_error(
                        StatusCode::TOO_MANY_REQUESTS,
                        id,
                        CODE_REFUSED,
                        &refusal.message(),
                    );
                    response
                        .headers_mut()
                        .insert(RETRY_AFTER, refusal.retry_after_seconds().into());
                    return response;
                }
            };
            let listed: Vec<Value> = match &single {
                Some((server, tools)) => tools
                    .iter()
                    .map(|t| tool_json(&server.name, t, t.name.clone()))
                    .collect(),
                None => super::combined::list(snapshot, key),
            };
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
            let (server, tool) = match &single {
                Some((server, _)) => (*server, tool),
                None => match super::combined::resolve(snapshot, tool) {
                    Some(found) => found,
                    None => return reply_error(ok, id, CODE_INVALID_PARAMS, "Unknown tool"),
                },
            };
            call::run(ToolCall {
                state,
                snapshot,
                key,
                server,
                tool,
                arguments,
                id: id.clone(),
                headers,
            })
            .await
        }
        m if single.is_some() && call::PASSED_METHODS.contains(&m) => {
            let Some((server, _)) = &single else {
                return reply_error(ok, id, CODE_METHOD_NOT_FOUND, "Method not found.");
            };
            let method = call::PASSED_METHODS.iter().find(|p| **p == m).unwrap();
            let params = match message.get("params") {
                None | Some(Value::Null) => json!({}),
                Some(p) if p.is_object() => p.clone(),
                Some(_) => {
                    return reply_error(ok, id, CODE_INVALID_PARAMS, "params must be an object.");
                }
            };
            call::proxy(state, snapshot, key, server, (method, &params), id, headers).await
        }
        _ => reply_error(ok, id, CODE_METHOD_NOT_FOUND, "Method not found."),
    }
}

/// A tool as `tools/list` shows it, under `exposed` as its name.
pub(crate) fn tool_json(server: &str, t: &crate::snapshot::SnapMcpTool, exposed: String) -> Value {
    let mut out = json!({
        "name": exposed,
        "description": t.description,
        "inputSchema": t.input_schema,
        "_meta": {"ultrafast": {"server": server}},
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
}

/// A call of the console's playground for a signed-in user: the same
/// pipeline as `POST /mcp/{server}`, for the user as a key they own would
/// make it (no team, no allowlist), recorded under no key.
pub(crate) async fn playground(
    state: &Arc<AppState>,
    user_id: i64,
    name: &str,
    body: Body,
) -> Response {
    let key = SnapKey {
        id: 0,
        name: "playground".to_string(),
        user_id: Some(user_id),
        team_id: None,
        expires_at: None,
        allowed: None,
        tags: crate::tags::Tags::new(),
        team_only: false,
        guardrails: Vec::new(),
    };
    let snapshot = state.snapshot.load_full();
    dispatch(state, &snapshot, &key, Some(name), &HeaderMap::new(), body).await
}
