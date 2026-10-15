//! A tool call through the gateway: policy, limits, budgets, guardrails,
//! the upstream call and the record, in the order of `/v1`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::header::RETRY_AFTER;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::{json, Value};
use time::OffsetDateTime;
use tokio::sync::Semaphore;

use super::server::{reply, reply_error, CODE_BUSY, CODE_INVALID_PARAMS, CODE_REFUSED};
use super::upstream::{self, Options, Target, UpstreamError};
use crate::access;
use crate::app::AppState;
use crate::guardrails::external::CallMeta;
use crate::guardrails::log::SideLog;
use crate::guardrails::run::{Active, Hooks, ScanFailed};
use crate::guardrails::Direction;
use crate::otel::TraceParent;
use crate::snapshot::{SnapKey, SnapMcpServer, Snapshot};
use crate::tags;
use crate::telemetry::{elapsed_ms, AttemptOutcome, Scope};

/// What calls are recorded as.
pub const ENDPOINT: &str = "mcp";

pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_MAX_RESULT_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_MAX_ARGS_BYTES: usize = 256 * 1024;
pub const DEFAULT_CALLS_PER_SERVER: usize = 32;

/// The caps of tool calls.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The longest an upstream call may take, answer included.
    pub call_timeout: Duration,
    /// The largest upstream answer that is read.
    pub max_result_bytes: usize,
    /// The largest request body `/mcp/{server}` takes.
    pub max_args_bytes: usize,
    /// Calls to one server at once, per process.
    pub calls_per_server: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            call_timeout: DEFAULT_CALL_TIMEOUT,
            max_result_bytes: DEFAULT_MAX_RESULT_BYTES,
            max_args_bytes: DEFAULT_MAX_ARGS_BYTES,
            calls_per_server: DEFAULT_CALLS_PER_SERVER,
        }
    }
}

fn env_number(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
}

impl Limits {
    /// `UF_MCP_CALL_TIMEOUT` (seconds), `UF_MCP_MAX_RESULT_BYTES` and
    /// `UF_MCP_MAX_ARGS_BYTES`; an unusable value reads as the default.
    pub fn from_env() -> Self {
        let bytes = |name, default| {
            env_number(name)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(default)
        };
        Self {
            call_timeout: env_number("UF_MCP_CALL_TIMEOUT")
                .map_or(DEFAULT_CALL_TIMEOUT, Duration::from_secs),
            max_result_bytes: bytes("UF_MCP_MAX_RESULT_BYTES", DEFAULT_MAX_RESULT_BYTES),
            max_args_bytes: bytes("UF_MCP_MAX_ARGS_BYTES", DEFAULT_MAX_ARGS_BYTES),
            calls_per_server: DEFAULT_CALLS_PER_SERVER,
        }
    }
}

/// What the MCP endpoint keeps in the state: its caps and the in-flight
/// gate of each server.
#[derive(Default)]
pub struct Runtime {
    pub limits: Limits,
    /// `UF_MCP_ALLOW_PRIVATE_UPSTREAMS=on`: servers may be registered at
    /// loopback, private and link-local addresses.
    pub allow_private_upstreams: bool,
    gates: Mutex<HashMap<i64, Arc<Semaphore>>>,
}

impl Runtime {
    pub fn from_env() -> Self {
        Self {
            limits: Limits::from_env(),
            allow_private_upstreams: std::env::var("UF_MCP_ALLOW_PRIVATE_UPSTREAMS")
                .is_ok_and(|v| v.trim().eq_ignore_ascii_case("on")),
            gates: Mutex::default(),
        }
    }

    /// The gate of a server, made on first use.
    pub(crate) fn gate(&self, server_id: i64) -> Arc<Semaphore> {
        self.gates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(server_id)
            .or_insert_with(|| Arc::new(Semaphore::new(self.limits.calls_per_server)))
            .clone()
    }
}

const SCAN_FAILED: &str = "The guardrails could not check this request.";
const SCAN_BUSY: &str = "The guardrails are busy checking other requests. Try again shortly.";
const UNKNOWN_TOOL: &str = "Unknown tool";

/// A tool call, as the endpoint received it.
pub struct ToolCall<'a> {
    pub state: &'a AppState,
    pub snapshot: &'a Snapshot,
    pub key: &'a SnapKey,
    pub server: &'a SnapMcpServer,
    /// The tool the caller named.
    pub tool: &'a str,
    pub arguments: Value,
    /// The id of the JSON-RPC request.
    pub id: Value,
    pub headers: &'a HeaderMap,
}

/// The result of a call the tool did not do: `isError`, with a message the
/// agent may read.
fn tool_error(id: &Value, message: &str) -> Response {
    reply(
        StatusCode::OK,
        id,
        json!({"content": [{"type": "text", "text": message}], "isError": true}),
    )
}

/// How the call ended, for the metric and the record.
struct Ending {
    status: u16,
    label: &'static str,
    /// Time at the server, when the call reached it.
    upstream_ms: Option<u64>,
}

fn upstream_failure(e: UpstreamError) -> (Ending, &'static str) {
    let (status, label, message) = match e {
        UpstreamError::Timeout => (504, "timeout", "The MCP server did not answer in time."),
        UpstreamError::TooLarge => (502, "too_large", "The MCP server's result was too large."),
        UpstreamError::Auth => (
            502,
            "upstream_error",
            "The MCP server did not accept the gateway's credentials.",
        ),
        UpstreamError::Connect => (
            502,
            "upstream_error",
            "Could not connect to the MCP server.",
        ),
        UpstreamError::InvalidParams => (400, "tool_error", "The server rejected the arguments."),
        UpstreamError::Unavailable => (503, "unavailable", "The MCP server is not running."),
        UpstreamError::Protocol | UpstreamError::TooMany | UpstreamError::NotSupported => (
            502,
            "upstream_error",
            "The MCP server sent an unexpected response.",
        ),
    };
    (
        Ending {
            status,
            label,
            upstream_ms: None,
        },
        message,
    )
}

/// The text of a block of `content` that guardrails read.
fn text_of(block: &mut Value) -> Option<&mut String> {
    if block["type"] != "text" {
        return None;
    }
    match block.get_mut("text")? {
        Value::String(s) => Some(s),
        _ => None,
    }
}

/// The guardrails' pass over a result: its text blocks and its structured
/// content (as text) are checked and rewritten in place. Returns the name of
/// the guardrail that blocked, if one did.
async fn check_result(
    guard: &Active,
    result: &mut Value,
    scope: &mut Scope,
    deadline: tokio::time::Instant,
) -> Result<Option<String>, ScanFailed> {
    if !guard.covers(Direction::Output) {
        return Ok(None);
    }
    let mut texts: Vec<String> = Vec::new();
    if let Some(blocks) = result["content"].as_array_mut() {
        texts.extend(blocks.iter_mut().filter_map(text_of).map(std::mem::take));
    }
    let mut structured = result
        .get("structuredContent")
        .filter(|v| !v.is_null())
        .map(Value::to_string);
    let originals = (texts.clone(), structured.clone());
    let outcome = {
        let slots: Vec<&mut String> = texts.iter_mut().chain(structured.iter_mut()).collect();
        guard.check(Direction::Output, slots, Some(deadline)).await
    };
    // The text goes back where it came from, rewritten or not.
    let outcome = match outcome {
        Ok(o) => o,
        Err(e) => {
            put_back(result, originals.0, originals.1);
            return Err(e);
        }
    };
    scope.guardrails_found(Direction::Output, SideLog::of(guard.refs(), &outcome));
    if let Some((_, name)) = outcome.blocked_by {
        return Ok(Some(name));
    }
    put_back(result, texts, structured);
    Ok(None)
}

fn put_back(result: &mut Value, texts: Vec<String>, structured: Option<String>) {
    let mut texts = texts.into_iter();
    if let Some(blocks) = result["content"].as_array_mut() {
        for slot in blocks.iter_mut().filter_map(text_of) {
            if let Some(t) = texts.next() {
                *slot = t;
            }
        }
    }
    if let Some(text) = structured {
        // A rewrite that is no longer JSON cannot be given as structured
        // content; the text blocks carry the answer.
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => result["structuredContent"] = v,
            Err(_) => {
                if let Some(o) = result.as_object_mut() {
                    o.remove("structuredContent");
                }
            }
        }
    }
}

/// The part of an upstream result the agent is given: content, structured
/// content and the error flag; nothing else the upstream chose to add.
fn shaped(result: &Value) -> Option<Value> {
    let content = match result.get("content") {
        None | Some(Value::Null) => json!([]),
        Some(c) if c.is_array() => c.clone(),
        Some(_) => return None,
    };
    let mut out = json!({
        "content": content,
        "isError": result["isError"].as_bool().unwrap_or(false),
    });
    if let Some(s) = result.get("structuredContent").filter(|v| !v.is_null()) {
        out["structuredContent"] = s.clone();
    }
    Some(out)
}

/// Runs one tool call through the pipeline of `/v1`: visibility, rate
/// limits, budgets, guardrails over the arguments, the upstream call,
/// guardrails over the result, and the record.
pub async fn run(call: ToolCall<'_>) -> Response {
    let ToolCall {
        state,
        snapshot,
        key,
        server,
        tool: tool_name,
        arguments,
        id,
        headers,
    } = call;
    // The console's playground makes calls as a user with no key (id 0): they
    // are recorded and counted for the user, under no key.
    let mut scope = Scope::begin(
        state.sink.clone(),
        (key.id != 0).then_some(key.id),
        key.user_id,
        key.team_id,
        ENDPOINT,
    );
    scope.metered(state.metrics.clone());
    scope.traced(state.otel.clone());
    scope.watched(state.alert_engine.clone());
    let traceparent = headers
        .get("traceparent")
        .and_then(|v| v.to_str().ok())
        .filter(|t| TraceParent::parse(t).is_some());
    if let Some(parent) = traceparent.and_then(TraceParent::parse) {
        scope.parented(parent);
    }
    let requested = format!("mcp:{}/{}", server.name, tool_name);
    scope.requested(&requested, false);
    match tags::from_headers(headers) {
        Ok(t) => scope.tagged(tags::effective(t, &key.tags)),
        Err(message) => {
            scope.tagged(key.tags.clone());
            scope.finish(400);
            return reply_error(StatusCode::BAD_REQUEST, &id, CODE_INVALID_PARAMS, &message);
        }
    }

    // 1. The tool must be one this key may call. A tool that is off, not
    // granted, not on the key's allowlist or not there is the same answer.
    let Some(tool) = snapshot
        .mcp_tool(&server.name, tool_name)
        .filter(|t| access::key_may_call_tool(snapshot, key, server, t))
    else {
        scope.finish(400);
        return reply_error(StatusCode::OK, &id, CODE_INVALID_PARAMS, UNKNOWN_TOOL);
    };
    let tool_name = tool.name.as_str();
    scope.resolved(None);
    let end = |scope: Scope, e: Ending| {
        state
            .metrics
            .mcp_call(&server.name, tool_name, e.label, e.upstream_ms);
        scope.finish(e.status);
    };
    let refused = |status: u16, label: &'static str| Ending {
        status,
        label,
        upstream_ms: None,
    };

    // 2. The rate limits of the key, its owner, their teams and the gateway.
    // A call refused after this gives its request back.
    let subjects = snapshot.subjects_of(Some(key.id), key.user_id, key.team_id);
    match state.rate.acquire(&subjects, 0, Instant::now()) {
        Ok(permit) => scope.hold(permit),
        Err(refusal) => {
            state.metrics.rate_limited(refusal.limit_name);
            end(scope, refused(429, "rate_limited"));
            let mut response = reply_error(
                StatusCode::TOO_MANY_REQUESTS,
                &id,
                CODE_REFUSED,
                &refusal.message(),
            );
            response
                .headers_mut()
                .insert(RETRY_AFTER, refusal.retry_after_seconds().into());
            return response;
        }
    }

    // 3. The budgets. A tool call costs nothing (no tool is priced), so this
    // only stops a subject whose budget is already spent.
    let budgets = snapshot.budgets_of(Some(key.id), key.user_id, key.team_id);
    if !budgets.is_empty() {
        if let Err(refusal) = state.budgets.check(&budgets, OffsetDateTime::now_utc()) {
            state.metrics.budget_blocked();
            scope.refund_permit();
            end(scope, refused(402, "budget_exceeded"));
            let mut response = reply_error(
                StatusCode::PAYMENT_REQUIRED,
                &id,
                CODE_REFUSED,
                &refusal.message(),
            );
            response
                .headers_mut()
                .insert(RETRY_AFTER, refusal.retry_after_seconds().into());
            return response;
        }
    }

    // 4. The guardrails over the arguments, as the JSON text they will be
    // sent in. Rules first, then external hooks (so a call that a rule
    // blocks never reaches a third party).
    let effective = snapshot.effective_guardrails(None, Some(key));
    let hooks = effective
        .iter()
        .any(|g| g.external.is_some())
        .then(|| Hooks {
            http: state.http.clone(),
            gates: state.hook_gates.clone(),
            meta: Arc::new(CallMeta {
                endpoint: ENDPOINT,
                model: requested.clone(),
                route: None,
                key_id: Some(key.id),
                team_id: key.team_id,
                user_id: key.user_id,
            }),
        });
    let guard = Active::of(&effective, hooks);
    let limits = state.mcp.limits;
    let deadline = tokio::time::Instant::now() + limits.call_timeout;
    let mut arguments = arguments;
    if guard.covers(Direction::Input) {
        let mut text = arguments.to_string();
        let sent = text.clone();
        let mut outcome = match guard
            .check_rules(Direction::Input, &mut [&mut text], Some(deadline))
            .await
        {
            Ok(o) => o,
            Err(failed) => {
                scope.refund_permit();
                let (status, message) = scan_message(failed);
                end(scope, refused(status, "scan_failed"));
                return tool_error(&id, message);
            }
        };
        scope.guardrails_found(Direction::Input, SideLog::of(guard.refs(), &outcome));
        if outcome.blocked_by.is_none() && guard.has_hooks(Direction::Input) {
            let asked = guard
                .check_hooks(Direction::Input, &mut [&mut text], Some(deadline))
                .await;
            outcome.merge(&asked);
            scope.guardrails_found(Direction::Input, SideLog::of(guard.refs(), &outcome));
        }
        if let Some((_, name)) = outcome.blocked_by {
            scope.refund_permit();
            end(scope, refused(400, "blocked"));
            return tool_error(&id, &format!("Blocked by guardrail '{name}'."));
        }
        if text != sent {
            match serde_json::from_str::<Value>(&text) {
                Ok(v) if v.is_object() => arguments = v,
                _ => {
                    scope.refund_permit();
                    end(scope, refused(400, "blocked"));
                    return tool_error(
                        &id,
                        "A guardrail changed the arguments of this call so they could not be sent.",
                    );
                }
            }
        }
    }

    // 5. The server's gate: at most so many calls to it at once.
    let Ok(_slot) = state.mcp.gate(server.id).try_acquire_owned() else {
        scope.refund_permit();
        end(scope, refused(503, "busy"));
        let mut response = reply_error(
            StatusCode::SERVICE_UNAVAILABLE,
            &id,
            CODE_BUSY,
            "The MCP server is busy. Try again shortly.",
        );
        response.headers_mut().insert(RETRY_AFTER, 1.into());
        return response;
    };

    // 6. The upstream call.
    scope.begin_attempt(&format!("mcp:{}", server.name), tool_name);
    let started = Instant::now();
    let target = Target {
        url: &server.url,
        auth_kind: &server.auth_kind,
        auth_header: server.auth_header.as_deref(),
        secret: server.secret.as_deref(),
    };
    let opts = Options {
        timeout: limits.call_timeout,
        max_body_bytes: limits.max_result_bytes,
    };
    let answered = if server.local.is_some() {
        let params = json!({"name": tool_name, "arguments": arguments});
        state
            .local
            .call(server.id, "tools/call", &params, opts)
            .await
    } else {
        upstream::call_tool(
            &state.http,
            &target,
            tool_name,
            &arguments,
            traceparent,
            opts,
        )
        .await
    };
    let upstream_ms = elapsed_ms(started);
    let raw = match answered {
        Ok(raw) => raw,
        Err(e) => {
            tracing::warn!(server = %server.name, tool = %tool_name, error = %e, "an MCP tool call failed");
            let (mut ending, message) = upstream_failure(e);
            ending.upstream_ms = Some(upstream_ms);
            scope.settle_attempt(AttemptOutcome::Fatal, None, started);
            end(scope, ending);
            return tool_error(&id, message);
        }
    };
    let Some(mut result) = shaped(&raw) else {
        scope.settle_attempt(AttemptOutcome::Fatal, None, started);
        end(
            scope,
            Ending {
                status: 502,
                label: "upstream_error",
                upstream_ms: Some(upstream_ms),
            },
        );
        return tool_error(&id, "The MCP server sent an unexpected response.");
    };
    scope.settle_attempt(AttemptOutcome::Ok, Some(200), started);
    let ended = |status: u16, label: &'static str| Ending {
        status,
        label,
        upstream_ms: Some(upstream_ms),
    };

    // 7. The guardrails over what came back. Images and embedded resources
    // are passed on unread.
    let deadline = tokio::time::Instant::now() + limits.call_timeout;
    match check_result(&guard, &mut result, &mut scope, deadline).await {
        Ok(None) => {}
        Ok(Some(name)) => {
            end(scope, ended(200, "blocked"));
            return tool_error(&id, &format!("Blocked by guardrail '{name}'."));
        }
        Err(failed) => {
            let (status, message) = scan_message(failed);
            end(scope, ended(status, "scan_failed"));
            return tool_error(&id, message);
        }
    }
    let label = if result["isError"] == true {
        "tool_error"
    } else {
        "ok"
    };
    end(scope, ended(200, label));
    reply(StatusCode::OK, &id, result)
}

fn scan_message(failed: ScanFailed) -> (u16, &'static str) {
    if failed == ScanFailed::Busy {
        tracing::warn!("a guardrail scan found no free CPU slot");
        (503, SCAN_BUSY)
    } else {
        tracing::error!("a guardrail scan did not finish");
        (500, SCAN_FAILED)
    }
}

/// The methods besides tools that are passed to the upstream as they are:
/// what a server offers to read and to start from. They are not tools, so a
/// key's tool grants do not cut them; a key that may call at least one tool
/// of the server may use them, with its rate limits, the server's gate and
/// the same time and size caps. Nothing in the result is filtered.
pub const PASSED_METHODS: [&str; 4] = [
    "resources/list",
    "resources/read",
    "prompts/list",
    "prompts/get",
];

/// Passes `method` to the upstream and returns its result.
pub async fn proxy(
    state: &AppState,
    snapshot: &Snapshot,
    key: &SnapKey,
    server: &SnapMcpServer,
    (method, params): (&'static str, &Value),
    id: &Value,
    headers: &HeaderMap,
) -> Response {
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
    let Ok(_slot) = state.mcp.gate(server.id).try_acquire_owned() else {
        let mut response = reply_error(
            StatusCode::SERVICE_UNAVAILABLE,
            id,
            CODE_BUSY,
            "The MCP server is busy. Try again shortly.",
        );
        response.headers_mut().insert(RETRY_AFTER, 1.into());
        return response;
    };
    let limits = state.mcp.limits;
    let traceparent = headers
        .get("traceparent")
        .and_then(|v| v.to_str().ok())
        .filter(|t| TraceParent::parse(t).is_some());
    let target = Target {
        url: &server.url,
        auth_kind: &server.auth_kind,
        auth_header: server.auth_header.as_deref(),
        secret: server.secret.as_deref(),
    };
    let opts = Options {
        timeout: limits.call_timeout,
        max_body_bytes: limits.max_result_bytes,
    };
    let started = Instant::now();
    let answered = if server.local.is_some() {
        state.local.call(server.id, method, params, opts).await
    } else {
        upstream::call_method(&state.http, &target, method, params, traceparent, opts).await
    };
    let ms = elapsed_ms(started);
    match answered {
        Ok(result) => {
            state.metrics.mcp_call(&server.name, method, "ok", Some(ms));
            reply(StatusCode::OK, id, result)
        }
        Err(e) => {
            tracing::warn!(server = %server.name, method, error = %e, "an MCP request failed");
            let (code, status, label, message) = match e {
                UpstreamError::NotSupported => (
                    super::server::CODE_METHOD_NOT_FOUND,
                    StatusCode::OK,
                    "unsupported",
                    "Method not found.",
                ),
                UpstreamError::InvalidParams => (
                    CODE_INVALID_PARAMS,
                    StatusCode::OK,
                    "tool_error",
                    "The server rejected the parameters.",
                ),
                UpstreamError::Timeout => (
                    CODE_REFUSED,
                    StatusCode::GATEWAY_TIMEOUT,
                    "timeout",
                    "The MCP server did not answer in time.",
                ),
                UpstreamError::TooLarge => (
                    CODE_REFUSED,
                    StatusCode::BAD_GATEWAY,
                    "too_large",
                    "The MCP server's result was too large.",
                ),
                UpstreamError::Unavailable => (
                    CODE_BUSY,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "unavailable",
                    "The MCP server is not running.",
                ),
                _ => (
                    CODE_REFUSED,
                    StatusCode::BAD_GATEWAY,
                    "upstream_error",
                    "The MCP server sent an unexpected response.",
                ),
            };
            state
                .metrics
                .mcp_call(&server.name, method, label, Some(ms));
            reply_error(status, id, code, message)
        }
    }
}
