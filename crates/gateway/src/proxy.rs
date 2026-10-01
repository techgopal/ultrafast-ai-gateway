//! The `/v1/chat/completions` handler.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::time::timeout_at;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use futures::stream::BoxStream;
use futures::StreamExt;
use http_body_util::LengthLimitError;
use rand::rngs::StdRng;
use ultrafast_translate::error::TranslateError;
use ultrafast_translate::ingress::openai::{
    parse_request, render_response, render_stream_error, render_stream_event,
};
use ultrafast_translate::provider::{
    build_request, parse_response, HttpRequest, StreamDecoder, Target,
};
use ultrafast_translate::types::{ChatRequest, ChatResponse, StreamEvent, Usage};

use crate::access::{self, Denied, Resolved};
use crate::app::AppState;
use crate::auth::authenticate;
use crate::errors::{caller_message, error_response, translate_error_response};
use crate::routing::{
    self, Candidate, Exhausted, Failure, HealthStore, Limits, Settings, Stop, Success, TargetRef,
};
use crate::snapshot::{SnapKey, SnapProvider, Snapshot};
use crate::telemetry::{AttemptOutcome, Scope};

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn not_found(model: &str) -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "not_found_error",
        &format!("Unknown model '{model}'."),
    )
}

fn forbidden(model: &str) -> Response {
    error_response(
        StatusCode::FORBIDDEN,
        "permission_error",
        &format!("You do not have access to model '{model}'."),
    )
}

/// `GET /v1/models`: what the key can call, models and routes.
pub async fn list_models(State(state): State<Arc<AppState>>, request: Request) -> Response {
    let snapshot = state.snapshot.load_full();
    let key = match authenticate(&snapshot, request.headers()) {
        Ok(key) => key,
        Err(resp) => return resp,
    };
    let data: Vec<_> = access::callable_names(&snapshot, &key)
        .into_iter()
        .map(|(id, owned_by)| {
            serde_json::json!({
                "id": id, "object": "model", "created": 0, "owned_by": owned_by
            })
        })
        .collect();
    Json(serde_json::json!({ "object": "list", "data": data })).into_response()
}

const NO_PROVIDER: &str = "No provider could serve this request.";
const NO_MODEL: &str = "No model is available for this request.";
const TIMED_OUT: &str = "The request timed out.";

pub async fn chat_completions(State(state): State<Arc<AppState>>, request: Request) -> Response {
    // 1. Authenticate on the headers alone. The body has not been read yet.
    let (parts, body) = request.into_parts();
    // One snapshot serves the whole request.
    let snapshot = state.snapshot.load_full();
    let key = match authenticate(&snapshot, &parts.headers) {
        Ok(key) => key,
        Err(resp) => return resp,
    };
    // From here on the call is recorded, once: when it is answered, when the
    // stream ends, or when the caller goes away (the scope is dropped).
    let mut scope = Some(Scope::begin(
        state.sink.clone(),
        key.id,
        key.user_id,
        key.team_id,
        "chat",
    ));
    let response = dispatch(&state, &snapshot, &key, body, &mut scope).await;
    // A stream took the scope with it and records itself.
    if let Some(scope) = scope {
        scope.finish(response.status().as_u16());
    }
    response
}

/// The targets of a call in the order they are tried, and how they are tried.
fn plan_of(
    snapshot: &Snapshot,
    key: &SnapKey,
    resolved: &Resolved<'_>,
    rng: &mut StdRng,
) -> (Vec<Candidate>, Settings) {
    let (order, settings) = match resolved {
        Resolved::Model(model) => (
            vec![TargetRef {
                provider: model.provider.clone(),
                model: model.name.clone(),
                model_id: model.id,
            }],
            Settings::DIRECT,
        ),
        Resolved::Route(route) => (
            routing::plan(route, rng),
            Settings {
                retries: route.retries,
                first_token_timeout: route.first_token_timeout,
                total_timeout: route.total_timeout,
                breaker: route.breaker,
            },
        ),
    };
    let candidates = order
        .into_iter()
        .map(|target| {
            let callable = snapshot.provider(&target.provider).is_some()
                && snapshot
                    .model(&target.provider, &target.model)
                    .is_some_and(|m| access::may_call_model(snapshot, key, m));
            Candidate { target, callable }
        })
        .collect();
    (candidates, settings)
}

async fn dispatch(
    state: &AppState,
    snapshot: &Snapshot,
    key: &SnapKey,
    body: Body,
    scope: &mut Option<Scope>,
) -> Response {
    let record = scope.as_mut().expect("the scope is taken only by a stream");

    // 2. Read and parse the body.
    let body = match axum::body::to_bytes(body, state.max_body_bytes).await {
        Ok(b) => b,
        Err(e) => {
            let too_large = e.into_inner().is::<LengthLimitError>();
            return if too_large {
                error_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "invalid_request_error",
                    "Request body is too large.",
                )
            } else {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    "Request body could not be read.",
                )
            };
        }
    };
    let req = match parse_request(&body) {
        Ok(r) => r,
        Err(e) => return translate_error_response(&e),
    };
    record.requested(&req.model, req.stream);

    // 3. Resolve the name to something this key may call.
    let resolved = match access::resolve(snapshot, key, &req.model) {
        Ok(r) => r,
        Err(Denied::Unknown) => return not_found(&req.model),
        Err(Denied::Forbidden) => return forbidden(&req.model),
    };
    let mut rng: StdRng = rand::make_rng();
    let (candidates, settings) = plan_of(snapshot, key, &resolved, &mut rng);
    record.targets(
        candidates
            .iter()
            .map(|c| (c.target.provider.clone(), c.target.model.clone()))
            .collect(),
    );
    if candidates.is_empty() {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "upstream_error", NO_MODEL);
    }

    // 4. Try the targets in order.
    let served = routing::run(
        &*state.health,
        record,
        &settings,
        &candidates,
        &mut rng,
        |target, limits| {
            // A candidate is called only when its provider is in the snapshot.
            let provider = snapshot.provider(&target.provider);
            try_target(
                &state.http,
                &req,
                provider,
                target,
                limits,
                state.max_provider_response_bytes,
            )
        },
    )
    .await;
    match served {
        Ok(Served::Whole(response)) => {
            scope
                .as_mut()
                .expect("a whole answer keeps the scope")
                .usage(response.usage);
            Json(render_response(&response, now_secs())).into_response()
        }
        Ok(Served::Stream(committed)) => {
            let guard = StreamRecord {
                scope: scope.take(),
                started: committed.started,
                health: state.health.clone(),
                target: committed.target.clone(),
                breaker: settings.breaker,
            };
            stream_to_caller(*committed, guard)
        }
        Err(Stop::Fatal(e)) => e.into_response(),
        Err(Stop::Exhausted(ex)) => exhausted_response(ex),
    }
}

/// The longest wait a caller is told to keep.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(60);

/// What the caller is told when no target served the call: the masked
/// credential error when every try was refused, 429 when every try was a 429,
/// otherwise 503.
fn exhausted_response(ex: Exhausted<CallError>) -> Response {
    if let Some(refused) = ex.refused {
        return refused.into_response();
    }
    let seconds = |d: Duration| d.min(RETRY_AFTER_CAP).as_secs_f64().ceil() as u64;
    let response = if ex.attempts > 0 && ex.rate_limited == ex.attempts {
        let mut r = error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_error",
            "The provider is rate limiting this request. Try again later.",
        );
        // Always a wait: one second when no provider said how long.
        let wait = seconds(ex.retry_after.unwrap_or_default()).max(1);
        r.headers_mut().insert(RETRY_AFTER, wait.into());
        r
    } else {
        let mut r = error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "upstream_error",
            NO_PROVIDER,
        );
        if let Some(wait) = ex.retry_after {
            r.headers_mut()
                .insert(RETRY_AFTER, seconds(wait).max(1).into());
        }
        r
    };
    response
}

/// How long a provider asked to be left alone: `Retry-After` as seconds or
/// as an HTTP date.
fn retry_after_of(headers: &axum::http::HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at =
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc2822).ok()?;
    let wait = (at - time::OffsetDateTime::now_utc()).whole_seconds();
    Some(Duration::from_secs(u64::try_from(wait).unwrap_or(0)))
}

/// Why a try of a target failed. Only a failure that is not retryable is
/// ever shown to the caller.
enum CallError {
    Translate(TranslateError),
    Redirect(String),
    TooLarge,
    /// Retryable: the caller sees only that no provider could serve it.
    Lost,
}

impl CallError {
    fn into_response(self) -> Response {
        match self {
            CallError::Translate(e) => translate_error_response(&e),
            CallError::Redirect(provider) => error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                &format!("Provider '{provider}' answered with a redirect."),
            ),
            CallError::TooLarge => error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "The provider response was too large.",
            ),
            CallError::Lost => error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "upstream_error",
                NO_PROVIDER,
            ),
        }
    }
}

/// What a target gave.
enum Served {
    Whole(ChatResponse),
    Stream(Box<Committed>),
}

/// A stream whose first event has arrived: the target is the caller's now.
struct Committed {
    target: TargetRef,
    /// When the try began.
    started: Instant,
    deadline: tokio::time::Instant,
    chunks: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    decoder: StreamDecoder,
    /// Decoded from the chunks read to find the first event.
    events: Vec<StreamEvent>,
    error: Option<TranslateError>,
}

fn retryable<E>(error: E, status: Option<u16>) -> Failure<E> {
    Failure::Retryable {
        error,
        status,
        retry_after: None,
    }
}

/// Whether another try could do better after this error of the provider.
/// A rejected credential is the gateway's problem with this target, not the
/// caller's: the next target is tried.
fn failure_of(
    e: TranslateError,
    status: Option<u16>,
    retry_after: Option<Duration>,
) -> Failure<CallError> {
    match e {
        TranslateError::Provider {
            retryable: true, ..
        }
        | TranslateError::Malformed(_) => Failure::Retryable {
            error: CallError::Lost,
            status,
            retry_after,
        },
        e @ TranslateError::Provider {
            status: 401 | 403, ..
        } => Failure::Failover {
            error: CallError::Translate(e),
            status,
        },
        e => Failure::Fatal {
            error: CallError::Translate(e),
            status,
        },
    }
}

/// Whether an error the provider sent inside a stream is the provider's own
/// trouble, which another target may not share. A rejected credential is not:
/// it is answered as it is outside a stream.
fn stream_error_is_retryable(e: &TranslateError) -> bool {
    match e {
        TranslateError::Provider {
            status: 401 | 403, ..
        } => false,
        TranslateError::Provider {
            status, retryable, ..
        } => *retryable || *status >= 500,
        TranslateError::Malformed(_) => true,
        _ => false,
    }
}

/// How an error inside a stream counts for the record and the breaker.
fn outcome_of_error(e: &TranslateError) -> AttemptOutcome {
    if stream_error_is_retryable(e) {
        AttemptOutcome::Retryable
    } else {
        AttemptOutcome::Fatal
    }
}

/// One try of one target: the request, and the answer up to the point where
/// it is the caller's: the whole answer, or the first event of a stream.
/// Until the first byte `limits.first_token` holds; the whole try is held to
/// the deadline of the request.
async fn try_target(
    http: &reqwest::Client,
    req: &ChatRequest,
    provider: Option<&SnapProvider>,
    target: TargetRef,
    limits: Limits,
    max_response: usize,
) -> Result<Success<Served>, Failure<CallError>> {
    let Some(provider) = provider else {
        return Err(retryable(CallError::Lost, None));
    };
    let started = Instant::now();
    let first_by = tokio::time::Instant::now() + limits.first_token;
    let wire = Target {
        kind: provider.kind,
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        model: target.model.clone(),
        api_version: provider.api_version.clone(),
    };
    let out = build_request(&wire, req).map_err(|e| Failure::Fatal {
        error: CallError::Translate(e),
        status: None,
    })?;
    let upstream = match timeout_at(first_by, send(http, out)).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            // `without_url` keeps credentials in query strings out of logs.
            let e = e.without_url();
            tracing::warn!(provider = %provider.name, error = %e, "provider unreachable");
            return Err(retryable(CallError::Lost, None));
        }
        Err(_) => {
            tracing::warn!(provider = %provider.name, "provider gave no answer in time");
            return Err(retryable(CallError::Lost, None));
        }
    };
    let status = upstream.status().as_u16();
    let retry_after = retry_after_of(upstream.headers());
    // Redirects are not followed, and a redirect is never a usable answer.
    if (300..400).contains(&status) {
        tracing::warn!(provider = %provider.name, status, "provider answered with a redirect");
        return Err(Failure::Fatal {
            error: CallError::Redirect(provider.name.clone()),
            status: Some(status),
        });
    }
    if req.stream && status < 400 {
        return first_event(
            upstream,
            provider,
            target,
            started,
            first_by,
            limits.deadline,
            status,
        )
        .await;
    }

    let bytes = match timeout_at(
        limits.deadline,
        read_capped(upstream, max_response, limits.first_token),
    )
    .await
    {
        Ok(Ok(b)) => b,
        Ok(Err(ReadError::TooLarge)) => {
            tracing::warn!(provider = %provider.name, "provider response was too large");
            return Err(Failure::Fatal {
                error: CallError::TooLarge,
                status: Some(status),
            });
        }
        Ok(Err(ReadError::Failed)) | Err(_) => {
            return Err(retryable(CallError::Lost, Some(status)));
        }
    };
    match parse_response(provider.kind, status, &bytes) {
        Ok(r) => Ok(Success {
            value: Served::Whole(r),
            status: Some(status),
        }),
        Err(e) => Err(failure_of(e, Some(status), retry_after)),
    }
}

/// Reads a stream until its first event, which commits the target.
async fn first_event(
    upstream: reqwest::Response,
    provider: &SnapProvider,
    target: TargetRef,
    started: Instant,
    first_by: tokio::time::Instant,
    deadline: tokio::time::Instant,
    status: u16,
) -> Result<Success<Served>, Failure<CallError>> {
    let mut chunks = upstream.bytes_stream().boxed();
    let mut decoder = StreamDecoder::new(provider.kind);
    loop {
        let chunk = match timeout_at(first_by, chunks.next()).await {
            Err(_) => {
                tracing::warn!(provider = %provider.name, "provider sent no event in time");
                return Err(retryable(CallError::Lost, Some(status)));
            }
            Ok(None) => {
                tracing::warn!(provider = %provider.name, "provider stream ended before an event");
                return Err(retryable(CallError::Lost, Some(status)));
            }
            Ok(Some(Err(e))) => {
                let e = e.without_url();
                tracing::warn!(provider = %provider.name, error = %e, "provider stream was lost");
                return Err(retryable(CallError::Lost, Some(status)));
            }
            Ok(Some(Ok(bytes))) => bytes,
        };
        match decoder.feed(&chunk) {
            Err(e) => {
                log_stream_error(&provider.name, &e);
                return Err(if stream_error_is_retryable(&e) {
                    retryable(CallError::Lost, Some(status))
                } else {
                    failure_of(e, Some(status), None)
                });
            }
            Ok(events) if events.is_empty() => continue,
            Ok(events) => {
                let error = decoder.take_error();
                return Ok(Success {
                    value: Served::Stream(Box::new(Committed {
                        target,
                        started,
                        deadline,
                        chunks,
                        decoder,
                        events,
                        error,
                    })),
                    status: Some(status),
                });
            }
        }
    }
}

enum ReadError {
    TooLarge,
    Failed,
}

/// Reads a provider response, giving up once it is larger than `max` bytes
/// or when no byte comes for `idle`.
async fn read_capped(
    upstream: reqwest::Response,
    max: usize,
    idle: Duration,
) -> Result<Vec<u8>, ReadError> {
    let mut out = Vec::new();
    let mut chunks = upstream.bytes_stream();
    while let Some(chunk) = tokio::time::timeout(idle, chunks.next())
        .await
        .map_err(|_| ReadError::Failed)?
    {
        let chunk = chunk.map_err(|_| ReadError::Failed)?;
        if chunk.len() > max - out.len() {
            return Err(ReadError::TooLarge);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

async fn send(
    http: &reqwest::Client,
    out: HttpRequest,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut rb = http.post(&out.url);
    for (k, v) in &out.headers {
        rb = rb.header(k, v);
    }
    rb.body(out.body).send().await
}

fn stream_id() -> String {
    let mut bytes = [0u8; 12];
    crate::secrets::fill_random(&mut bytes);
    format!("chatcmpl-{}", hex::encode(bytes))
}

/// Logs a stream failure. The provider's text about a rejected credential
/// may quote the credential, so only the masked message is logged for it.
fn log_stream_error(provider: &str, e: &TranslateError) {
    let (_, _, message) = caller_message(e);
    let credential = matches!(
        e,
        TranslateError::Provider {
            status: 401 | 403,
            ..
        }
    );
    if credential {
        tracing::warn!(provider = %provider, error = %message, "provider stream failed");
    } else {
        tracing::warn!(provider = %provider, error = %e, "provider stream failed");
    }
}

/// Logs a stream failure and renders the error event the caller may see.
fn stream_failure(provider: &str, e: &TranslateError) -> String {
    log_stream_error(provider, e);
    let (_, _, message) = caller_message(e);
    render_stream_error(&message)
}

/// Holds a stream's record until the stream ends. Dropping it, which is what
/// happens when the caller goes away, emits the record as a gone caller.
struct StreamRecord {
    scope: Option<Scope>,
    started: Instant,
    health: Arc<dyn HealthStore>,
    target: TargetRef,
    breaker: crate::routing::BreakerSettings,
}

impl StreamRecord {
    /// Records the end of the stream: what the attempt came to, and the
    /// usage if it was reported. The caller was answered 200. The success of
    /// the first event is already with the breaker; a failure after it is
    /// reported now, and counts when another try could have done better.
    fn end(self, outcome: AttemptOutcome, usage: Option<Usage>) {
        self.finish(outcome, usage, true);
    }

    /// Like [`end`](Self::end) for a stream the request ran out of time on:
    /// a long answer is not the target's failure, so the breaker is left alone.
    fn end_out_of_time(self) {
        self.finish(AttemptOutcome::Retryable, None, false);
    }

    fn finish(mut self, outcome: AttemptOutcome, usage: Option<Usage>, report: bool) {
        if let Some(mut scope) = self.scope.take() {
            if report && outcome != AttemptOutcome::Ok {
                self.health.report(
                    &self.target,
                    false,
                    outcome == AttemptOutcome::Retryable,
                    Some(200),
                    tokio::time::Instant::now(),
                    &self.breaker,
                );
            }
            scope.set_last_outcome(outcome);
            scope.end_last_attempt(self.started);
            scope.usage(usage);
            scope.finish(200);
        }
    }
}

impl Drop for StreamRecord {
    fn drop(&mut self) {
        // The caller went away before the end: the attempt did not finish.
        if let Some(scope) = self.scope.as_mut() {
            scope.set_last_outcome(AttemptOutcome::Retryable);
            scope.end_last_attempt(self.started);
        }
    }
}

/// Forwards the provider's stream to the caller as OpenAI server-sent events.
///
/// The body owns the upstream response, so when the caller disconnects and the
/// body is dropped, the provider request is dropped with it.
fn stream_to_caller(committed: Committed, record: StreamRecord) -> Response {
    let created = now_secs();
    let id = stream_id();
    let body = async_stream::stream! {
        let record = record;
        let Committed {
            target,
            started: _,
            deadline,
            mut chunks,
            mut decoder,
            events,
            error,
        } = committed;
        let provider = target.provider.clone();
        let model = target.model.clone();
        let mut pending = Some((events, error));
        loop {
            let (events, error) = match pending.take() {
                Some(first) => first,
                None => {
                    let chunk = match timeout_at(deadline, chunks.next()).await {
                        Err(_) => {
                            tracing::warn!(provider = %provider, "request ran out of time during the stream");
                            record.end_out_of_time();
                            yield Ok::<String, Infallible>(render_stream_error(TIMED_OUT));
                            return;
                        }
                        Ok(None) => {
                            record.end(AttemptOutcome::Retryable, None);
                            tracing::warn!(provider = %provider, "provider stream ended before completion");
                            yield Ok(render_stream_error("The provider stream ended before completion."));
                            return;
                        }
                        Ok(Some(Err(e))) => {
                            let e = e.without_url();
                            tracing::warn!(provider = %provider, error = %e, "provider stream was lost");
                            record.end(AttemptOutcome::Retryable, None);
                            yield Ok(render_stream_error(
                                "The connection to the provider was lost.",
                            ));
                            return;
                        }
                        Ok(Some(Ok(bytes))) => bytes,
                    };
                    match decoder.feed(&chunk) {
                        Ok(events) => (events, decoder.take_error()),
                        Err(e) => (Vec::new(), Some(e)),
                    }
                }
            };
            for ev in events {
                let usage = match &ev {
                    StreamEvent::Done { usage, .. } => Some(*usage),
                    _ => None,
                };
                let rendered = render_stream_event(&ev, &id, &model, created);
                if let Some(usage) = usage {
                    // Recorded before the last event is handed over, so a
                    // caller that leaves right after it is not a lost call.
                    record.end(AttemptOutcome::Ok, usage);
                    yield Ok(rendered);
                    return;
                }
                yield Ok(rendered);
            }
            // An error that ended the stream, after the events before it.
            if let Some(e) = error {
                record.end(outcome_of_error(&e), None);
                yield Ok(stream_failure(&provider, &e));
                return;
            }
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .header(CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(body))
        .expect("static headers are valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    fn with(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, value.parse().unwrap());
        h
    }

    #[test]
    fn retry_after_reads_seconds_and_dates() {
        assert_eq!(retry_after_of(&with("7")), Some(Duration::from_secs(7)));
        assert_eq!(retry_after_of(&with(" 0 ")), Some(Duration::ZERO));
        assert_eq!(retry_after_of(&HeaderMap::new()), None);
        assert_eq!(retry_after_of(&with("soon")), None);
        assert_eq!(retry_after_of(&with("-3")), None);
        // A date in the past is no wait; one far ahead is a long one.
        assert_eq!(
            retry_after_of(&with("Mon, 01 Jan 1990 00:00:00 GMT")),
            Some(Duration::ZERO)
        );
        let far = retry_after_of(&with("Fri, 01 Jan 2100 00:00:00 GMT")).unwrap();
        assert!(far > Duration::from_secs(3600 * 24 * 365));
    }
}
