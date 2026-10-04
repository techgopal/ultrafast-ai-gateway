//! The `/v1` call handlers: chat completions, Anthropic messages and
//! embeddings. They share authentication, body reading, resolution, the
//! routing engine and recording; only the caller's format differs.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::time::timeout_at;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use futures::stream::BoxStream;
use futures::StreamExt;
use http_body_util::LengthLimitError;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use time::OffsetDateTime;
use ultrafast_translate::embeddings::{
    self, EmbeddingsRequest, EmbeddingsResponse, NOT_SUPPORTED as EMBEDDINGS_NOT_SUPPORTED,
};
use ultrafast_translate::error::TranslateError;
use ultrafast_translate::ingress::{anthropic, openai};
use ultrafast_translate::provider::{
    build_request, parse_response, HttpRequest, StreamDecoder, Target,
};
use ultrafast_translate::types::{ChatRequest, ChatResponse, StreamEvent, Usage};

use crate::access::{self, Denied, Resolved};
use crate::app::AppState;
use crate::auth::authenticate;
use crate::cache::{Answer, CacheKey, CacheScope, Cached, KeyParts, ScopeId};
use crate::errors::{caller_message, Shape};
use crate::routing::{
    self, Candidate, Exhausted, Failure, HealthStore, Limits, Settings, Stop, Success, TargetRef,
};
use crate::snapshot::{SnapKey, SnapProvider, Snapshot};
use crate::tags::{self, Tags};
use crate::telemetry::{AttemptOutcome, Scope};

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn not_found(shape: Shape, model: &str) -> Response {
    shape.error(
        StatusCode::NOT_FOUND,
        "not_found_error",
        &format!("Unknown model '{model}'."),
    )
}

fn forbidden(shape: Shape, model: &str) -> Response {
    shape.error(
        StatusCode::FORBIDDEN,
        "permission_error",
        &format!("You do not have access to model '{model}'."),
    )
}

/// `GET /v1/models`: what the key can call, models and routes.
pub async fn list_models(State(state): State<Arc<AppState>>, request: Request) -> Response {
    let snapshot = state.snapshot.load_full();
    let key = match authenticate(&snapshot, request.headers(), Shape::OpenAi) {
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

/// What a chat call that names no `max_tokens` is expected to answer with.
const DEFAULT_MAX_TOKENS_ESTIMATE: u32 = 1_000;

/// The three calls of `/v1` that reach a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Chat,
    Messages,
    Embeddings,
    /// A chat call the console makes for a signed-in user: the answer of
    /// `/v1/chat/completions`, recorded as its own endpoint.
    Playground,
}

impl Endpoint {
    /// What records call it.
    fn name(self) -> &'static str {
        match self {
            Endpoint::Chat => "chat",
            Endpoint::Messages => "messages",
            Endpoint::Embeddings => "embeddings",
            Endpoint::Playground => "playground",
        }
    }

    fn shape(self) -> Shape {
        match self {
            Endpoint::Messages => Shape::Anthropic,
            Endpoint::Chat | Endpoint::Embeddings | Endpoint::Playground => Shape::OpenAi,
        }
    }

    fn parse(self, body: &[u8]) -> Result<Call, TranslateError> {
        match self {
            Endpoint::Chat | Endpoint::Playground => openai::parse_request(body).map(Call::Chat),
            Endpoint::Messages => anthropic::parse_request(body).map(Call::Chat),
            Endpoint::Embeddings => embeddings::parse_request(body).map(Call::Embed),
        }
    }
}

/// What the caller asked for, in the common form.
enum Call {
    Chat(ChatRequest),
    Embed(EmbeddingsRequest),
}

impl Call {
    fn model(&self) -> &str {
        match self {
            Call::Chat(r) => &r.model,
            Call::Embed(r) => &r.model,
        }
    }

    fn stream(&self) -> bool {
        matches!(self, Call::Chat(r) if r.stream)
    }

    /// What the call is expected to use, for the rate limit: the most it may
    /// answer (`max_tokens`, or 1 000 when it names none) and its input at
    /// four characters to a token. An embedding has no answer to count.
    fn estimated_tokens(&self) -> u64 {
        match self {
            Call::Chat(r) => {
                u64::from(r.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS_ESTIMATE))
                    + self.input_estimate()
            }
            Call::Embed(_) => self.input_estimate(),
        }
    }

    /// The input of the call in tokens, at four characters to a token: what
    /// a stream that ends without a usage report is charged for its input.
    fn input_estimate(&self) -> u64 {
        fn tokens(chars: usize) -> u64 {
            chars.div_ceil(4) as u64
        }
        match self {
            Call::Chat(r) => tokens(r.messages.iter().map(|m| m.content.chars().count()).sum()),
            Call::Embed(r) => tokens(r.input.iter().map(|s| s.chars().count()).sum()),
        }
    }

    /// Whether a provider of this kind can serve it.
    fn served_by(&self, kind: ultrafast_translate::provider::ProviderKind) -> bool {
        match self {
            Call::Chat(_) => true,
            Call::Embed(_) => kind.supports_embeddings(),
        }
    }
}

pub async fn chat_completions(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Chat).await
}

pub async fn messages(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Messages).await
}

pub async fn embeddings(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Embeddings).await
}

/// Who a call is made for: a virtual key, or a signed-in user who has none
/// (the console playground). Everything that decides what the call may do
/// (access, limits, budgets, cache, records) reads it from here.
pub(crate) struct Actor<'a> {
    /// The key as access sees it: the real key, or for a user without one
    /// a stand-in owned by the user, with no team and no allowlist, so
    /// the user's own grants and teams decide.
    access: Cow<'a, SnapKey>,
    /// The key the call is counted and recorded under; `None` for a user.
    key_id: Option<i64>,
}

impl<'a> Actor<'a> {
    fn of_key(key: &'a SnapKey) -> Self {
        Self {
            access: Cow::Borrowed(key),
            key_id: Some(key.id),
        }
    }

    /// A signed-in user, as a key they own with no team and no allowlist.
    pub(crate) fn of_user(user_id: i64) -> Self {
        Self {
            access: Cow::Owned(SnapKey {
                id: 0,
                name: "playground".to_string(),
                user_id: Some(user_id),
                team_id: None,
                expires_at: None,
                allowed: None,
                tags: Tags::new(),
            }),
            key_id: None,
        }
    }
}

/// A chat call of the console playground for a signed-in user: the same
/// pipeline as `/v1/chat/completions`, recorded without a key.
pub(crate) async fn playground(state: Arc<AppState>, user_id: i64, body: Body) -> Response {
    let snapshot = state.snapshot.load_full();
    run(
        &state,
        &snapshot,
        &Actor::of_user(user_id),
        None,
        body,
        Endpoint::Playground,
    )
    .await
}

async fn handle(state: Arc<AppState>, request: Request, endpoint: Endpoint) -> Response {
    // 1. Authenticate on the headers alone. The body has not been read yet.
    let (parts, body) = request.into_parts();
    // One snapshot serves the whole request.
    let snapshot = state.snapshot.load_full();
    let key = match authenticate(&snapshot, &parts.headers, endpoint.shape()) {
        Ok(key) => key,
        Err(resp) => return resp,
    };
    run(
        &state,
        &snapshot,
        &Actor::of_key(&key),
        Some(&parts.headers),
        body,
        endpoint,
    )
    .await
}

async fn run(
    state: &AppState,
    snapshot: &Snapshot,
    actor: &Actor<'_>,
    headers: Option<&HeaderMap>,
    body: Body,
    endpoint: Endpoint,
) -> Response {
    // From here on the call is recorded, once: when it is answered, when the
    // stream ends, or when the caller goes away (the scope is dropped).
    let mut begun = Scope::begin(
        state.sink.clone(),
        actor.key_id,
        actor.access.user_id,
        actor.access.team_id,
        endpoint.name(),
    );
    begun.metered(state.metrics.clone());
    // The caller's tags, under the key's. An invalid header is the caller's
    // mistake: refused here, and recorded as the call it was.
    let call_tags = headers.map_or(Ok(Tags::new()), tags::from_headers);
    match call_tags {
        Ok(tags) => begun.tagged(tags::effective(tags, &actor.access.tags)),
        Err(message) => {
            begun.tagged(actor.access.tags.clone());
            let response =
                endpoint
                    .shape()
                    .error(StatusCode::BAD_REQUEST, "invalid_request_error", &message);
            begun.finish(response.status().as_u16());
            return response;
        }
    }
    let mut scope = Some(begun);
    let response = dispatch(state, snapshot, actor, body, endpoint, &mut scope).await;
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
    call: &Call,
    resolved: &Resolved<'_>,
    rng: &mut impl Rng,
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
            let callable = snapshot
                .provider(&target.provider)
                .is_some_and(|p| call.served_by(p.kind))
                && snapshot
                    .model(&target.provider, &target.model)
                    .is_some_and(|m| access::may_call_model(snapshot, key, m));
            Candidate { target, callable }
        })
        .collect();
    (candidates, settings)
}

/// The warmest a call may be and still be kept: above it the answer is
/// meant to differ from one call to the next.
const MAX_CACHED_TEMPERATURE: f32 = 0.5;

/// What a call is kept and looked up under.
struct CachePlan {
    key: CacheKey,
    ttl: Duration,
}

/// The cache of a call, when it has one: it goes to a route with the cache
/// on, it is not a stream, and it is not random. The key names the targets
/// of the route this key may call, so an answer a target gave is never
/// given to a key that may not call that target.
fn cache_plan(
    snapshot: &Snapshot,
    actor: &Actor<'_>,
    call: &Call,
    resolved: &Resolved<'_>,
    candidates: &[Candidate],
) -> Option<CachePlan> {
    let key: &SnapKey = &actor.access;
    let Resolved::Route(route) = resolved else {
        return None;
    };
    if !route.cache.enabled || call.stream() {
        return None;
    }
    if let Call::Chat(r) = call {
        // A temperature that is not a number is not "at most" anything.
        let kept = |t: f32| {
            matches!(
                t.partial_cmp(&MAX_CACHED_TEMPERATURE),
                Some(Ordering::Less | Ordering::Equal)
            )
        };
        if r.temperature.is_some_and(|t| !kept(t)) {
            return None;
        }
    }
    let mut targets: Vec<(String, String)> = candidates
        .iter()
        .filter(|c| c.callable)
        .map(|c| (c.target.provider.clone(), c.target.model.clone()))
        .collect();
    targets.sort();
    targets.dedup();
    if targets.is_empty() {
        return None;
    }
    // A caller with no key is never cached "per key": every such caller
    // would share the same one. Its user is the nearest scope.
    let cache_scope = match (route.cache.scope, actor.key_id) {
        (CacheScope::Key, None) => CacheScope::User,
        (scope, _) => scope,
    };
    let parts = KeyParts {
        route: &route.name,
        targets: &targets,
        scope: ScopeId::of(cache_scope, key.team_id, key.user_id, key.id),
        config: snapshot.cache_fingerprint(),
    };
    let cache_key = match call {
        Call::Chat(r) => CacheKey::chat(&parts, r),
        Call::Embed(r) => CacheKey::embeddings(&parts, r),
    };
    let seconds = u64::try_from(route.cache.ttl_s).unwrap_or(0).max(1);
    Some(CachePlan {
        key: cache_key,
        ttl: Duration::from_secs(seconds),
    })
}

/// The kept answer in the shape the caller asked in, or `None` when it is
/// of the other kind of call (which a key never allows).
fn render_cached(endpoint: Endpoint, cached: &Cached) -> Option<Response> {
    match (&cached.answer, endpoint) {
        (Answer::Chat(r), Endpoint::Messages) => {
            Some(Json(anthropic::render_response(r)).into_response())
        }
        (Answer::Chat(r), Endpoint::Chat | Endpoint::Playground) => {
            Some(Json(openai::render_response(r, now_secs())).into_response())
        }
        (Answer::Embeddings(r), Endpoint::Embeddings) => {
            Some(Json(embeddings::render_response(r)).into_response())
        }
        _ => None,
    }
}

/// Whether the key may call some target of the plan, and a provider of that
/// target cannot serve this call: nothing is wrong with the key or the
/// providers, the model is of the wrong kind.
fn wrong_kind(snapshot: &Snapshot, key: &SnapKey, call: &Call, candidates: &[Candidate]) -> bool {
    let reachable = |c: &&Candidate| {
        snapshot.provider(&c.target.provider).is_some()
            && snapshot
                .model(&c.target.provider, &c.target.model)
                .is_some_and(|m| access::may_call_model(snapshot, key, m))
    };
    candidates.iter().any(|c| reachable(&c))
        && !candidates.iter().any(|c| c.callable)
        && !matches!(call, Call::Chat(_))
}

async fn dispatch(
    state: &AppState,
    snapshot: &Snapshot,
    actor: &Actor<'_>,
    body: Body,
    endpoint: Endpoint,
    scope: &mut Option<Scope>,
) -> Response {
    let key: &SnapKey = &actor.access;
    let shape = endpoint.shape();
    let record = scope.as_mut().expect("the scope is taken only by a stream");

    // 2. Read and parse the body.
    let body = match axum::body::to_bytes(body, state.max_body_bytes).await {
        Ok(b) => b,
        Err(e) => {
            let too_large = e.into_inner().is::<LengthLimitError>();
            return if too_large {
                shape.error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "invalid_request_error",
                    "Request body is too large.",
                )
            } else {
                shape.error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    "Request body could not be read.",
                )
            };
        }
    };
    let call = match endpoint.parse(&body) {
        Ok(c) => c,
        Err(e) => return shape.translate_error(&e),
    };
    record.requested(call.model(), call.stream());

    // 3. Resolve the name to something this key may call.
    let resolved = match access::resolve(snapshot, key, call.model()) {
        Ok(r) => r,
        Err(Denied::Unknown) => return not_found(shape, call.model()),
        Err(Denied::Forbidden) => return forbidden(shape, call.model()),
    };
    // 3b. The rate limits of the key, its owner, their teams and the gateway.
    // The permit goes with the scope, which a stream carries to its end. A
    // call that a limit refuses counts nowhere; one that is refused after
    // this (a spent budget, no usable model) gives back its request and its
    // token estimate at every scope.
    let subjects = snapshot.subjects_of(actor.key_id, key.user_id, key.team_id);
    match state
        .rate
        .acquire(&subjects, call.estimated_tokens(), Instant::now())
    {
        Ok(permit) => record.hold(permit),
        Err(refusal) => {
            state.metrics.rate_limited(refusal.limit_name);
            return shape.rate_limited(&refusal);
        }
    }
    // 3c. The budgets of the same subjects: a spent `block` budget refuses
    // the call. Spend is counted when the log writer prices a call, so what
    // was already running is not stopped.
    let budgets = snapshot.budgets_of(actor.key_id, key.user_id, key.team_id);
    if !budgets.is_empty() {
        if let Err(refusal) = state.budgets.check(&budgets, OffsetDateTime::now_utc()) {
            state.metrics.budget_blocked();
            record.refund_permit();
            return shape.budget_exceeded(&refusal);
        }
    }
    // Seeded from the thread's generator (itself seeded once per thread), not
    // from the operating system on every request. It is `Send`: it lives
    // across awaits.
    let mut rng = StdRng::from_rng(&mut rand::rng());
    let (candidates, settings) = plan_of(snapshot, key, &call, &resolved, &mut rng);
    record.targets(
        candidates
            .iter()
            .map(|c| (c.target.provider.clone(), c.target.model.clone()))
            .collect(),
    );
    if candidates.is_empty() {
        record.refund_permit();
        return shape.error(StatusCode::SERVICE_UNAVAILABLE, "upstream_error", NO_MODEL);
    }

    // 3d. The response cache of the route: after access, limits and budgets,
    // so a hit is refused as a call would be. A hit calls no provider.
    let cache = cache_plan(snapshot, actor, &call, &resolved, &candidates);
    if let Some(plan) = &cache {
        let now = tokio::time::Instant::now().into_std();
        if let Some(hit) = state.cache.get(&plan.key, now) {
            if let Some(response) = render_cached(endpoint, &hit) {
                state.metrics.cache_hit();
                record.cache_hit(&hit.provider, &hit.model, hit.usage());
                return response;
            }
        }
        state.metrics.cache_miss();
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
                &call,
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
            let record = scope.as_mut().expect("a whole answer keeps the scope");
            record.usage(response.usage);
            keep(state, &cache, record, Answer::Chat(response.clone()));
            match endpoint {
                Endpoint::Messages => Json(anthropic::render_response(&response)).into_response(),
                _ => Json(openai::render_response(&response, now_secs())).into_response(),
            }
        }
        Ok(Served::Embeddings(response)) => {
            let record = scope.as_mut().expect("a whole answer keeps the scope");
            record.usage(Some(Usage {
                input_tokens: response.prompt_tokens,
                output_tokens: 0,
            }));
            keep(state, &cache, record, Answer::Embeddings(response.clone()));
            Json(embeddings::render_response(&response)).into_response()
        }
        Ok(Served::Stream(committed)) => {
            if let Some(scope) = scope.as_mut() {
                scope.begin_stream(call.input_estimate());
            }
            let guard = StreamRecord {
                scope: scope.take(),
                started: committed.started,
                health: state.health.clone(),
                target: committed.target.clone(),
                breaker: settings.breaker,
            };
            stream_to_caller(*committed, guard, endpoint)
        }
        Err(Stop::Fatal(e)) => e.into_response(shape),
        Err(Stop::Exhausted(_)) if wrong_kind(snapshot, key, &call, &candidates) => shape.error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            EMBEDDINGS_NOT_SUPPORTED,
        ),
        Err(Stop::Exhausted(ex)) => exhausted_response(shape, ex),
    }
}

/// Keeps the answer of a call under the key of its cache plan, with the
/// target that gave it.
fn keep(state: &AppState, plan: &Option<CachePlan>, record: &Scope, answer: Answer) {
    let (Some(plan), Some((provider, model))) = (plan, record.answered_by()) else {
        return;
    };
    let now = tokio::time::Instant::now().into_std();
    let value = Cached {
        answer,
        provider,
        model,
    };
    state.cache.put(plan.key, value, plan.ttl, now);
}

/// The longest wait a caller is told to keep.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(60);

/// What the caller is told when no target served the call: the masked
/// credential error when every try was refused, 429 when every try was a 429,
/// otherwise 503.
fn exhausted_response(shape: Shape, ex: Exhausted<CallError>) -> Response {
    if let Some(refused) = ex.refused {
        return refused.into_response(shape);
    }
    let seconds = |d: Duration| d.min(RETRY_AFTER_CAP).as_secs_f64().ceil() as u64;
    let response = if ex.attempts > 0 && ex.rate_limited == ex.attempts {
        let mut r = shape.error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_error",
            "The provider is rate limiting this request. Try again later.",
        );
        // Always a wait: one second when no provider said how long.
        let wait = seconds(ex.retry_after.unwrap_or_default()).max(1);
        r.headers_mut().insert(RETRY_AFTER, wait.into());
        r
    } else {
        let mut r = shape.error(
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
    // Digits too many for a number are a very long wait: the cap.
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        return Some(RETRY_AFTER_CAP);
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
    /// The provider answered 404. The caller is not shown its body.
    UnknownModel,
    /// Retryable: the caller sees only that no provider could serve it.
    Lost,
}

impl CallError {
    fn into_response(self, shape: Shape) -> Response {
        match self {
            CallError::Translate(e) => shape.translate_error(&e),
            CallError::Redirect(provider) => shape.error(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                &format!("Provider '{provider}' answered with a redirect."),
            ),
            CallError::TooLarge => shape.error(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "The provider response was too large.",
            ),
            CallError::UnknownModel => shape.error(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "The provider does not know this model.",
            ),
            CallError::Lost => shape.error(
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
    Embeddings(EmbeddingsResponse),
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
/// A rejected credential, or a model the provider does not know, is the
/// gateway's problem with this target, not the caller's: the next target is
/// tried.
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
        // The provider does not know the model (retired, or an Azure
        // deployment that is missing): another target may.
        TranslateError::Provider { status: 404, .. } => Failure::Failover {
            error: CallError::UnknownModel,
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
    call: &Call,
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
    let built = match call {
        Call::Chat(req) => build_request(&wire, req),
        Call::Embed(req) => embeddings::build_request(&wire, req),
    };
    let out = built.map_err(|e| Failure::Fatal {
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
    if call.stream() && status < 400 {
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
    let parsed = match call {
        Call::Chat(_) => parse_response(provider.kind, status, &bytes).map(Served::Whole),
        Call::Embed(_) => embeddings::parse_response(provider.kind, status, &bytes, &target.model)
            .map(Served::Embeddings),
    };
    match parsed {
        Ok(value) => Ok(Success {
            value,
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
                // Some providers give their last event only when the stream closes.
                let events = decoder.finish();
                if events.is_empty() {
                    tracing::warn!(provider = %provider.name, "provider stream ended before an event");
                    return Err(retryable(CallError::Lost, Some(status)));
                }
                return Ok(Success {
                    value: Served::Stream(Box::new(Committed {
                        target,
                        started,
                        deadline,
                        chunks,
                        decoder,
                        events,
                        error: None,
                    })),
                    status: Some(status),
                });
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

fn stream_id(prefix: &str) -> String {
    let mut bytes = [0u8; 12];
    crate::secrets::fill_random(&mut bytes);
    format!("{prefix}-{}", hex::encode(bytes))
}

/// Renders a stream in the caller's format.
enum StreamFormat {
    OpenAi {
        id: String,
        model: String,
        created: u64,
    },
    Anthropic(anthropic::StreamRenderer),
}

impl StreamFormat {
    fn new(endpoint: Endpoint, model: &str) -> Self {
        match endpoint {
            Endpoint::Messages => {
                StreamFormat::Anthropic(anthropic::StreamRenderer::new(&stream_id("msg"), model))
            }
            _ => StreamFormat::OpenAi {
                id: stream_id("chatcmpl"),
                model: model.to_string(),
                created: now_secs(),
            },
        }
    }

    fn event(&mut self, ev: &StreamEvent) -> String {
        match self {
            StreamFormat::OpenAi { id, model, created } => {
                openai::render_stream_event(ev, id, model, *created)
            }
            StreamFormat::Anthropic(r) => r.render(ev),
        }
    }

    fn error(&self, message: &str) -> String {
        match self {
            StreamFormat::OpenAi { .. } => openai::render_stream_error(message),
            StreamFormat::Anthropic(_) => anthropic::render_stream_error(message),
        }
    }
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
fn stream_failure(format: &StreamFormat, provider: &str, e: &TranslateError) -> String {
    log_stream_error(provider, e);
    let (_, _, message) = caller_message(e);
    format.error(&message)
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
    /// `chars` characters of the answer went to the caller.
    fn streamed(&mut self, chars: usize) {
        if let Some(scope) = self.scope.as_mut() {
            scope.streamed(chars);
        }
    }

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

/// Forwards the provider's stream to the caller as server-sent events in the
/// format of the endpoint it came in on.
///
/// The body owns the upstream response, so when the caller disconnects and the
/// body is dropped, the provider request is dropped with it.
fn stream_to_caller(committed: Committed, record: StreamRecord, endpoint: Endpoint) -> Response {
    let body = async_stream::stream! {
        let mut record = record;
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
        let mut format = StreamFormat::new(endpoint, &target.model);
        let mut pending = Some((events, error));
        loop {
            let (events, error) = match pending.take() {
                Some(first) => first,
                None => {
                    let chunk = match timeout_at(deadline, chunks.next()).await {
                        Err(_) => {
                            tracing::warn!(provider = %provider, "request ran out of time during the stream");
                            record.end_out_of_time();
                            yield Ok::<String, Infallible>(format.error(TIMED_OUT));
                            return;
                        }
                        Ok(None) => {
                            let tail = decoder.finish();
                            if tail.is_empty() {
                                record.end(AttemptOutcome::Retryable, None);
                                tracing::warn!(provider = %provider, "provider stream ended before completion");
                                yield Ok(format.error("The provider stream ended before completion."));
                                return;
                            }
                            // The closing event the provider held back.
                            pending = Some((tail, None));
                            continue;
                        }
                        Ok(Some(Err(e))) => {
                            let e = e.without_url();
                            tracing::warn!(provider = %provider, error = %e, "provider stream was lost");
                            record.end(AttemptOutcome::Retryable, None);
                            yield Ok(format.error(
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
                if let StreamEvent::Delta { text } = &ev {
                    record.streamed(text.chars().count());
                }
                let usage = match &ev {
                    StreamEvent::Done { usage, .. } => Some(*usage),
                    _ => None,
                };
                let rendered = format.event(&ev);
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
                yield Ok(stream_failure(&format, &provider, &e));
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
        // Too many digits for a number: the cap, not no hint.
        assert_eq!(
            retry_after_of(&with("99999999999999999999999999")),
            Some(RETRY_AFTER_CAP)
        );
        assert_eq!(RETRY_AFTER_CAP, Duration::from_secs(60));
        // A date in the past is no wait; one far ahead is a long one.
        assert_eq!(
            retry_after_of(&with("Mon, 01 Jan 1990 00:00:00 GMT")),
            Some(Duration::ZERO)
        );
        let far = retry_after_of(&with("Fri, 01 Jan 2100 00:00:00 GMT")).unwrap();
        assert!(far > Duration::from_secs(3600 * 24 * 365));
    }
}
