//! The `/v1` call handlers: chat completions, Anthropic messages, the OpenAI
//! Responses API and embeddings. They share authentication, body reading, resolution, the
//! routing engine and recording; only the caller's format differs.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::time::timeout_at;

use crate::otel::TraceParent;
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
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use ultrafast_translate::audio::{
    self, FileInfo, SpeechRequest, Task, TranscribeRequest, Transcript, TranscriptAnswer,
};
use ultrafast_translate::embeddings::{
    self, EmbeddingsRequest, EmbeddingsResponse, NOT_SUPPORTED as EMBEDDINGS_NOT_SUPPORTED,
};
use ultrafast_translate::error::TranslateError;
use ultrafast_translate::images::{self, ImageRequest, ImageResponse};
use ultrafast_translate::ingress::prompt::PromptRef;
use ultrafast_translate::ingress::{anthropic, openai, responses};
use ultrafast_translate::provider::{
    build_request, parse_response, HttpRequest, StreamDecoder, Target,
};
use ultrafast_translate::types::{
    ChatRequest, ChatResponse, FinishReason, Part, StreamEvent, Usage,
};

use crate::access::{self, Denied, Resolved};
use crate::app::AppState;
use crate::auth::authenticate;
use crate::cache::{Answer, CacheKey, CacheScope, Cached, KeyParts, ScopeId};
use crate::errors::{caller_message, Shape};
use crate::guardrails::external::CallMeta;
use crate::guardrails::log::{GuardrailRef, SideLog};
use crate::guardrails::run::{Active, Hooks, ScanFailed};
use crate::guardrails::{Direction, Outcome, Release, StreamScanner};
use crate::prompts::{self, UseError};
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
/// What an image counts for in the estimate of a call's input, in tokens.
const IMAGE_TOKEN_ESTIMATE: u64 = 1_000;
/// What one generated image counts for in the estimate of an image call, in
/// tokens (a 1024 x 1024 image of a GPT image model is about this much).
const IMAGE_OUTPUT_TOKEN_ESTIMATE: u64 = 1_000;

/// The three calls of `/v1` that reach a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Chat,
    Messages,
    /// The OpenAI Responses API, stateless: chat in another shape.
    Responses,
    Embeddings,
    /// Image generation.
    Images,
    /// Speech to text, in the language spoken.
    Transcriptions,
    /// Speech to English text.
    Translations,
    /// Text to speech.
    Speech,
    /// A chat call the console makes for a signed-in user: the answer of
    /// `/v1/chat/completions`, recorded as its own endpoint.
    Playground,
    /// An image generation the console makes for a signed-in user. Recorded
    /// as `playground` too.
    PlaygroundImages,
    /// A transcription the console makes for a signed-in user.
    PlaygroundTranscriptions,
    /// A speech the console makes for a signed-in user.
    PlaygroundSpeech,
}

impl Endpoint {
    /// What records call it.
    fn name(self) -> &'static str {
        match self {
            Endpoint::Chat => "chat",
            Endpoint::Messages => "messages",
            Endpoint::Responses => "responses",
            Endpoint::Embeddings => "embeddings",
            Endpoint::Images => "images",
            Endpoint::Transcriptions => "transcriptions",
            Endpoint::Translations => "translations",
            Endpoint::Speech => "speech",
            Endpoint::Playground
            | Endpoint::PlaygroundImages
            | Endpoint::PlaygroundTranscriptions
            | Endpoint::PlaygroundSpeech => "playground",
        }
    }

    fn shape(self) -> Shape {
        match self {
            Endpoint::Messages => Shape::Anthropic,
            Endpoint::Chat
            | Endpoint::Responses
            | Endpoint::Embeddings
            | Endpoint::Images
            | Endpoint::Transcriptions
            | Endpoint::Translations
            | Endpoint::Speech
            | Endpoint::Playground
            | Endpoint::PlaygroundImages
            | Endpoint::PlaygroundTranscriptions
            | Endpoint::PlaygroundSpeech => Shape::OpenAi,
        }
    }

    /// The call and the prompt template it names, if it names one (only
    /// chat calls may: `/v1/chat/completions`, `/v1/responses` and the
    /// playground). The template is applied by [`use_prompt`].
    fn parse(self, body: &[u8]) -> Result<(Call, Option<PromptRef>), TranslateError> {
        let plain = |call: Call| (call, None);
        match self {
            Endpoint::Chat | Endpoint::Playground => {
                let (request, prompt) = openai::parse_prompted(body)?;
                Ok((Call::Chat(request), prompt))
            }
            Endpoint::Messages => anthropic::parse_request(body).map(|r| plain(Call::Chat(r))),
            Endpoint::Responses => {
                let parsed = responses::parse_request(body)?;
                Ok((Call::Chat(parsed.request), parsed.prompt))
            }
            Endpoint::Embeddings => embeddings::parse_request(body).map(|r| plain(Call::Embed(r))),
            Endpoint::Images | Endpoint::PlaygroundImages => {
                images::parse_request(body).map(|r| plain(Call::Image(r)))
            }
            Endpoint::Speech | Endpoint::PlaygroundSpeech => {
                audio::parse_speech(body).map(|r| plain(Call::Speech(r)))
            }
            // A form, read by `read_upload`.
            Endpoint::Transcriptions
            | Endpoint::Translations
            | Endpoint::PlaygroundTranscriptions => Err(TranslateError::InvalidRequest(
                "the request must be multipart/form-data".into(),
            )),
        }
    }

    /// The task of an endpoint whose body is an audio upload.
    fn upload_task(self) -> Option<Task> {
        match self {
            Endpoint::Transcriptions | Endpoint::PlaygroundTranscriptions => {
                Some(Task::Transcription)
            }
            Endpoint::Translations => Some(Task::Translation),
            _ => None,
        }
    }
}

/// The audio of an upload as it was read: the chunks as they arrived, so
/// that holding it takes no more memory than the file and a repeat of the
/// call shares it.
type Chunks = Arc<Vec<Bytes>>;

/// What the caller asked for, in the common form.
enum Call {
    Chat(ChatRequest),
    Embed(EmbeddingsRequest),
    Image(ImageRequest),
    Transcribe(TranscribeRequest, Chunks),
    Speech(SpeechRequest),
}

/// What one token of input is worth in bytes of audio, for the estimate of a
/// transcription (a minute of compressed speech is about 500 KB and 600
/// tokens).
const AUDIO_BYTES_PER_TOKEN: usize = 800;

impl Call {
    fn model(&self) -> &str {
        match self {
            Call::Chat(r) => &r.model,
            Call::Embed(r) => &r.model,
            Call::Image(r) => &r.model,
            Call::Transcribe(r, _) => &r.model,
            Call::Speech(r) => &r.model,
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
            Call::Embed(_) | Call::Transcribe(..) | Call::Speech(_) => self.input_estimate(),
            // The prompt, and the tokens of the images it may ask for.
            Call::Image(r) => {
                self.input_estimate() + u64::from(r.n.unwrap_or(1)) * IMAGE_OUTPUT_TOKEN_ESTIMATE
            }
        }
    }

    /// The input of the call in tokens, at four characters to a token: what
    /// a stream that ends without a usage report is charged for its input.
    fn input_estimate(&self) -> u64 {
        fn tokens(chars: usize) -> u64 {
            chars.div_ceil(4) as u64
        }
        match self {
            Call::Chat(r) => {
                let mut chars = 0usize;
                let mut images = 0u64;
                for m in &r.messages {
                    for part in &m.content {
                        match part {
                            Part::Text(t) => chars += t.chars().count(),
                            Part::Image(_) => images += 1,
                        }
                    }
                    for c in &m.tool_calls {
                        chars += c.name.chars().count() + c.arguments.chars().count();
                    }
                }
                for t in &r.tools {
                    chars += serde_json::to_string(t).map_or(0, |s| s.chars().count());
                }
                tokens(chars) + images * IMAGE_TOKEN_ESTIMATE
            }
            Call::Embed(r) => tokens(r.input.iter().map(|s| s.chars().count()).sum()),
            Call::Image(r) => tokens(r.prompt.chars().count()),
            Call::Transcribe(r, _) => r.file.len.div_ceil(AUDIO_BYTES_PER_TOKEN) as u64,
            Call::Speech(r) => tokens(r.input.chars().count()),
        }
    }

    /// Whether the call is paid for as the provider works, so that a repeat
    /// is a second charge for the same wish: after the request was sent and
    /// no answer came in time it is not tried again, on this target or on a
    /// fallback.
    fn billed_once(&self) -> bool {
        matches!(
            self,
            Call::Image(_) | Call::Transcribe(..) | Call::Speech(_)
        )
    }

    /// Whether the call is slow enough to need longer timeouts.
    fn is_slow(&self) -> bool {
        self.billed_once()
    }

    /// The largest answer of the provider that is read for this call.
    fn response_cap(&self, state: &AppState) -> usize {
        match self {
            Call::Image(_) => state.max_image_response_bytes,
            Call::Speech(_) => state.max_speech_response_bytes,
            Call::Chat(_) | Call::Embed(_) | Call::Transcribe(..) => {
                state.max_provider_response_bytes
            }
        }
    }

    /// Whether a provider of this kind can serve it.
    fn served_by(&self, kind: ultrafast_translate::provider::ProviderKind) -> bool {
        match self {
            Call::Chat(_) => true,
            Call::Embed(_) => kind.supports_embeddings(),
            Call::Image(_) => kind.supports_images(),
            Call::Transcribe(..) | Call::Speech(_) => kind.supports_audio(),
        }
    }

    /// What a caller is told when no target of the plan can serve it.
    fn not_supported(&self) -> &'static str {
        match self {
            Call::Chat(_) | Call::Embed(_) => EMBEDDINGS_NOT_SUPPORTED,
            Call::Image(_) => images::NOT_SUPPORTED,
            Call::Transcribe(..) | Call::Speech(_) => audio::NOT_SUPPORTED,
        }
    }
}

pub async fn chat_completions(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Chat).await
}

pub async fn messages(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Messages).await
}

pub async fn responses(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Responses).await
}

pub async fn embeddings(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Embeddings).await
}

pub async fn images(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Images).await
}

pub async fn transcriptions(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Transcriptions).await
}

pub async fn translations(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Translations).await
}

pub async fn speech(state: State<Arc<AppState>>, request: Request) -> Response {
    handle(state.0, request, Endpoint::Speech).await
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
                team_only: false,
                guardrails: Vec::new(),
            }),
            key_id: None,
        }
    }
}

/// A transcription of the console playground for a signed-in user: the same
/// pipeline as `/v1/audio/transcriptions`, recorded without a key.
pub(crate) async fn playground_transcriptions(
    state: Arc<AppState>,
    user_id: i64,
    body: Body,
    form_type: Option<&str>,
) -> Response {
    let snapshot = state.snapshot.load_full();
    run(
        &state,
        &snapshot,
        &Actor::of_user(user_id),
        None,
        form_type,
        body,
        Endpoint::PlaygroundTranscriptions,
    )
    .await
}

/// A speech of the console playground for a signed-in user: the same
/// pipeline as `/v1/audio/speech`, recorded without a key.
pub(crate) async fn playground_speech(state: Arc<AppState>, user_id: i64, body: Body) -> Response {
    let snapshot = state.snapshot.load_full();
    run(
        &state,
        &snapshot,
        &Actor::of_user(user_id),
        None,
        None,
        body,
        Endpoint::PlaygroundSpeech,
    )
    .await
}

/// An image generation of the console playground for a signed-in user: the
/// same pipeline as `/v1/images/generations`, recorded without a key.
pub(crate) async fn playground_images(state: Arc<AppState>, user_id: i64, body: Body) -> Response {
    let snapshot = state.snapshot.load_full();
    run(
        &state,
        &snapshot,
        &Actor::of_user(user_id),
        None,
        None,
        body,
        Endpoint::PlaygroundImages,
    )
    .await
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
        parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
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
    form_type: Option<&str>,
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
    begun.traced(state.otel.clone());
    begun.watched(state.alert_engine.clone());
    // Only the `/v1` handlers pass headers: the playground has no parent.
    if let Some(parent) = headers
        .and_then(|h| h.get("traceparent"))
        .and_then(|v| v.to_str().ok())
        .and_then(TraceParent::parse)
    {
        begun.parented(parent);
    }
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
    let response = dispatch(
        state, snapshot, actor, form_type, body, endpoint, &mut scope,
    )
    .await;
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
    guardrails: &Active,
) -> Option<CachePlan> {
    let key: &SnapKey = &actor.access;
    let Resolved::Route(route) = resolved else {
        return None;
    };
    // An image, a transcript or a speech is never kept: every call is a new
    // one, and the answers are large.
    if !route.cache.enabled || call.stream() || !matches!(call, Call::Chat(_) | Call::Embed(_)) {
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
        config: cache_config(snapshot, guardrails),
    };
    let cache_key = match call {
        Call::Chat(r) => CacheKey::chat(&parts, r),
        Call::Embed(r) => CacheKey::embeddings(&parts, r),
        Call::Image(_) | Call::Transcribe(..) | Call::Speech(_) => return None,
    };
    let seconds = u64::try_from(route.cache.ttl_s).unwrap_or(0).max(1);
    Some(CachePlan {
        key: cache_key,
        ttl: Duration::from_secs(seconds),
    })
}

/// The configuration a cached answer is found under: the snapshot's, and the
/// guardrails the call ran with. Two callers of one cache scope whose keys
/// carry different guardrails get different answers (one redacted, one not),
/// so they must not share an entry.
fn cache_config(snapshot: &Snapshot, guardrails: &Active) -> [u8; 32] {
    let fingerprint = snapshot.cache_fingerprint();
    if guardrails.refs().is_empty() {
        return fingerprint;
    }
    let mut hash = Sha256::new();
    hash.update(fingerprint);
    for id in guardrails.ids() {
        hash.update(id.to_le_bytes());
    }
    hash.finalize().into()
}

/// The kept answer in the shape the caller asked in, or `None` when it is
/// of the other kind of call (which a key never allows).
fn render_cached(endpoint: Endpoint, call: &Call, cached: &Cached) -> Option<Response> {
    match (&cached.answer, endpoint) {
        (Answer::Chat(r), Endpoint::Messages) => {
            Some(Json(anthropic::render_response(r)).into_response())
        }
        (Answer::Chat(r), Endpoint::Responses) => Some(
            Json(responses::render_response(
                r,
                &response_id(),
                now_secs(),
                &echo_of(call),
            ))
            .into_response(),
        ),
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

/// The most the non-file fields of an upload may take together, beyond the
/// file: field headers, boundaries and the text fields.
const FORM_SLACK: usize = 1024 * 1024;
/// The largest text field of an upload.
const MAX_FORM_FIELD: usize = 64 * 1024;
const MAX_FORM_FIELDS: usize = 32;

/// What an upload asks of the call before its file is read: given the model
/// the form names and the size the sender declared, it decides access and
/// takes the rate limits, or answers with the refusal.
type Early<'a> = &'a mut (dyn FnMut(&str, u64) -> Result<(), Response> + Send);

/// The place an upload holds among the gateway's concurrent uploads, until
/// the call that carries its file ends.
type UploadSlot = tokio::sync::OwnedSemaphorePermit;

/// Reads the body of a call and parses it.
async fn read_call(
    state: &AppState,
    endpoint: Endpoint,
    body: Body,
    form_type: Option<&str>,
    shape: Shape,
    early: Early<'_>,
) -> Result<(Call, Option<PromptRef>, Option<UploadSlot>), Response> {
    if let Some(task) = endpoint.upload_task() {
        return read_upload(state, task, body, form_type, shape, early)
            .await
            .map(|(call, slot)| (call, None, Some(slot)));
    }
    let body = match axum::body::to_bytes(body, state.max_body_bytes).await {
        Ok(b) => b,
        Err(e) => {
            let too_large = e.into_inner().is::<LengthLimitError>();
            return Err(if too_large {
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
            });
        }
    };
    endpoint
        .parse(&body)
        .map(|(call, prompt)| (call, prompt, None))
        .map_err(|e| shape.translate_error(&e))
}

/// Renders the template a chat call names into the call. A template that
/// does not exist, a version that does not, and values that do not fit are
/// refused (see [`refuse_prompt`]) before any limit counts the call and
/// before any provider is reached. Anyone who can call may use any template.
fn use_prompt(
    snapshot: &Snapshot,
    reference: &PromptRef,
    call: &mut Call,
    record: &mut Scope,
) -> Result<(), UseError> {
    let Call::Chat(request) = call else {
        return Ok(());
    };
    let template = snapshot
        .prompt(&reference.id)
        .ok_or_else(|| UseError::NotFound(reference.id.clone()))?;
    let version = template.pick(reference.version.as_deref())?;
    record.prompt(&template.name, version.number);
    prompts::apply(version, reference, request)
}

fn refuse_prompt(shape: Shape, e: &UseError) -> Response {
    let (status, kind) = if e.is_not_found() {
        (StatusCode::NOT_FOUND, "not_found_error")
    } else {
        (StatusCode::BAD_REQUEST, "invalid_request_error")
    };
    shape.error(status, kind, &e.to_string())
}

/// A file name that is safe to pass on in a header.
fn clean_file_name(raw: Option<&str>) -> String {
    let name: String = raw
        .unwrap_or_default()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\\' | ';'))
        .take(255)
        .collect();
    if name.trim().is_empty() {
        "audio".to_string()
    } else {
        name
    }
}

/// Reads a multipart upload while it arrives: nothing past the cap is ever
/// read or held. The file is kept as the chunks it came in, to be sent on
/// to the provider; an upload over `max_audio_bytes` (declared or found) is
/// refused with 413 before any provider is called.
async fn read_upload(
    state: &AppState,
    task: Task,
    body: Body,
    form_type: Option<&str>,
    shape: Shape,
    early: Early<'_>,
) -> Result<(Call, UploadSlot), Response> {
    let cap = state.max_audio_bytes;
    let too_large = || {
        shape.error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_request_error",
            "The audio file is too large.",
        )
    };
    let bad =
        |message: &str| shape.error(StatusCode::BAD_REQUEST, "invalid_request_error", message);
    let Some(boundary) = form_type.and_then(|t| multer::parse_boundary(t).ok()) else {
        return Err(bad("The request must be multipart/form-data."));
    };
    // Only so many uploads are received at once, whatever the keys: each
    // holds up to the cap in memory. The body of a ninth is not read.
    let Ok(slot) = state.audio_uploads.clone().try_acquire_owned() else {
        tracing::warn!("an audio upload was refused: too many are being received");
        let mut busy = shape.error(
            StatusCode::SERVICE_UNAVAILABLE,
            "upstream_error",
            "The gateway is busy receiving other uploads. Try again shortly.",
        );
        busy.headers_mut().insert(RETRY_AFTER, 1.into());
        return Err(busy);
    };
    let whole = cap.saturating_add(FORM_SLACK);
    // A declared size is believed only to refuse: the limits below hold
    // whatever the sender says.
    let declared = axum::body::HttpBody::size_hint(&body).lower();
    if declared > whole as u64 {
        return Err(too_large());
    }
    let limits = multer::Constraints::new().size_limit(
        multer::SizeLimit::new()
            .whole_stream(whole as u64)
            .per_field(MAX_FORM_FIELD as u64)
            .for_field("file", cap as u64),
    );
    let mut form = multer::Multipart::with_constraints(body.into_data_stream(), boundary, limits);
    let failed = |e: multer::Error| match e {
        multer::Error::FieldSizeExceeded {
            field_name: Some(name),
            ..
        } if name != "file" => {
            // Only the names of the form's fields are echoed.
            let known = [
                "model",
                "language",
                "prompt",
                "response_format",
                "temperature",
                "timestamp_granularities",
                "timestamp_granularities[]",
            ];
            let which = if known.contains(&name.as_str()) {
                format!("The field '{name}' is too large.")
            } else {
                "A field of the form is too large.".to_string()
            };
            shape.error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "invalid_request_error",
                &which,
            )
        }
        multer::Error::StreamSizeExceeded { .. } | multer::Error::FieldSizeExceeded { .. } => {
            too_large()
        }
        _ => bad("The upload could not be read."),
    };
    let slow = || {
        shape.error(
            StatusCode::REQUEST_TIMEOUT,
            "invalid_request_error",
            "The upload was too slow.",
        )
    };
    let idle = state.upload_idle;
    let read = async {
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut file: Option<(Vec<Bytes>, FileInfo)> = None;
        let mut asked = false;
        loop {
            let mut field = match tokio::time::timeout(idle, form.next_field()).await {
                Err(_) => return Err(slow()),
                Ok(Ok(Some(field))) => field,
                Ok(Ok(None)) => break,
                Ok(Err(e)) => return Err(failed(e)),
            };
            if fields.len() >= MAX_FORM_FIELDS {
                return Err(bad("The form has too many fields."));
            }
            let name = field.name().unwrap_or_default().to_string();
            if name == "file" {
                if file.is_some() {
                    return Err(bad("field 'file' is given twice"));
                }
                // The model is known: access and the rate limits are decided
                // now, before the file is read. A form that sends the model
                // after the file pays for the read first.
                if !asked {
                    asked = true;
                    if let Some((_, model)) =
                        fields.iter().find(|(n, v)| n == "model" && !v.is_empty())
                    {
                        early(model, declared)?;
                    }
                }
                let name = clean_file_name(field.file_name());
                let content_type = field
                    .content_type()
                    .map(ToString::to_string)
                    .filter(|t| {
                        t.len() <= 100 && t.chars().all(|c| c.is_ascii_graphic() || c == ' ')
                    })
                    .unwrap_or_else(|| "application/octet-stream".to_string());
                let mut chunks = Vec::new();
                let mut len = 0usize;
                loop {
                    match tokio::time::timeout(idle, field.chunk()).await {
                        Err(_) => return Err(slow()),
                        Ok(Ok(Some(chunk))) => {
                            len += chunk.len();
                            if len > cap {
                                return Err(too_large());
                            }
                            chunks.push(chunk);
                        }
                        Ok(Ok(None)) => break,
                        Ok(Err(e)) => return Err(failed(e)),
                    }
                }
                file = Some((
                    chunks,
                    FileInfo {
                        name,
                        content_type,
                        len,
                    },
                ));
            } else {
                match tokio::time::timeout(idle, field.text()).await {
                    Err(_) => return Err(slow()),
                    Ok(Ok(text)) => fields.push((name, text)),
                    Ok(Err(e)) => return Err(failed(e)),
                }
            }
        }
        Ok((fields, file))
    };
    let (fields, file) = match tokio::time::timeout(state.upload_total, read).await {
        Err(_) => return Err(slow()),
        Ok(read) => read?,
    };
    let Some((chunks, info)) = file else {
        return Err(bad("file is required"));
    };
    audio::parse_transcription(task, &fields, info)
        .map(|r| (Call::Transcribe(r, Arc::new(chunks)), slot))
        .map_err(|e| shape.translate_error(&e))
}

async fn dispatch(
    state: &AppState,
    snapshot: &Snapshot,
    actor: &Actor<'_>,
    form_type: Option<&str>,
    body: Body,
    endpoint: Endpoint,
    scope: &mut Option<Scope>,
) -> Response {
    let key: &SnapKey = &actor.access;
    let shape = endpoint.shape();
    let record = scope.as_mut().expect("the scope is taken only by a stream");

    // 2. Read and parse the body. An audio upload whose form names the model
    // before the file has its access decided and its rate limits taken here,
    // before the file is read (`early`); the permit then stands for 3a.
    let mut early_permit = false;
    // A refusal is a whole response, as everywhere else in this function.
    #[allow(clippy::result_large_err)]
    let mut early = |model: &str, declared: u64| -> Result<(), Response> {
        record.requested(model, false);
        let resolved = match access::resolve(snapshot, key, model) {
            Ok(r) => r,
            Err(Denied::Unknown) => return Err(not_found(shape, model)),
            Err(Denied::Forbidden) => return Err(forbidden(shape, model)),
        };
        record.resolved(match &resolved {
            Resolved::Route(route) => Some(route.name.as_str()),
            _ => None,
        });
        let subjects = snapshot.subjects_of(actor.key_id, key.user_id, key.team_id);
        let tokens = declared.div_ceil(AUDIO_BYTES_PER_TOKEN as u64);
        match state.rate.acquire(&subjects, tokens, Instant::now()) {
            Ok(permit) => record.hold(permit),
            Err(refusal) => {
                state.metrics.rate_limited(refusal.limit_name);
                return Err(shape.rate_limited(&refusal));
            }
        }
        early_permit = true;
        Ok(())
    };
    let (mut call, prompt, _upload_slot) =
        match read_call(state, endpoint, body, form_type, shape, &mut early).await {
            Ok(read) => read,
            Err(refusal) => {
                record.refund_permit();
                return refusal;
            }
        };
    // 2a. A call that names a prompt template becomes the template's
    // messages and settings plus its own. Before access (the template may
    // name the model), the limits and the guardrails: they all see what the
    // provider is going to be sent.
    if let Some(reference) = prompt {
        if let Err(e) = use_prompt(snapshot, &reference, &mut call, record) {
            return refuse_prompt(shape, &e);
        }
    }
    record.requested(call.model(), call.stream());

    // 3. Resolve the name to something this key may call.
    let resolved = match access::resolve(snapshot, key, call.model()) {
        Ok(r) => r,
        Err(Denied::Unknown) => return not_found(shape, call.model()),
        Err(Denied::Forbidden) => return forbidden(shape, call.model()),
    };
    record.resolved(match &resolved {
        Resolved::Route(route) => Some(route.name.as_str()),
        _ => None,
    });
    // 3a. The rate limits of the key, its owner, their teams and the gateway.
    // First, so that the work of scanning a body is only done for a call the
    // limits let in. The permit goes with the scope, which a stream carries to
    // its end. A call that a limit refuses counts nowhere; one that is refused
    // after this (a guardrail block, a spent budget, no usable model) gives
    // back its request and its token estimate at every scope. The estimate is
    // of the input as it came, before any redaction.
    let subjects = snapshot.subjects_of(actor.key_id, key.user_id, key.team_id);
    if !early_permit {
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
    }
    // 3b. The guardrails of the call (the gateway's defaults, then the
    // route's, then the key's), over its input. After access and the rate
    // limits, before any budget or cache: a blocked call gives its permit
    // back and counts nowhere, and everything after this sees the redacted
    // input.
    let effective = snapshot.effective_guardrails(
        match &resolved {
            Resolved::Route(route) => Some(route),
            Resolved::Model(_) => None,
        },
        Some(key),
    );
    // Only a call with an external guardrail needs to say who is asking.
    let hooks = effective
        .iter()
        .any(|g| g.external.is_some())
        .then(|| Hooks {
            http: state.http.clone(),
            gates: state.hook_gates.clone(),
            meta: Arc::new(CallMeta {
                endpoint: endpoint.name(),
                model: call.model().to_string(),
                route: match &resolved {
                    Resolved::Route(route) => Some(route.name.clone()),
                    Resolved::Model(_) => None,
                },
                key_id: actor.key_id,
                team_id: key.team_id,
                user_id: key.user_id,
            }),
        });
    let guard = Active::of(&effective, hooks);
    let input_rules = match check_input_rules(&guard, &mut call, record, shape).await {
        Ok(outcome) => outcome,
        Err(refusal) => {
            record.refund_permit();
            return refusal;
        }
    };
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
    // 3c'. The external guardrails over the input: a call a limit or a budget
    // refused never reaches a third party, and one the hooks refuse gives its
    // permit back. They see the input as the rules left it, and what they
    // redact is what the cache key and the provider get.
    if let Err(refusal) = check_input_hooks(&guard, &mut call, input_rules, record, shape).await {
        record.refund_permit();
        return refusal;
    }
    // Seeded from the thread's generator (itself seeded once per thread), not
    // from the operating system on every request. It is `Send`: it lives
    // across awaits.
    let mut rng = StdRng::from_rng(&mut rand::rng());
    let (candidates, mut settings) = plan_of(snapshot, key, &call, &resolved, &mut rng);
    if call.is_slow() {
        settings = state.slow_calls.raise(settings);
    }
    record.targets(
        candidates
            .iter()
            .map(|c| (c.target.provider.clone(), c.target.model.clone()))
            .collect(),
    );
    record.provider_kinds(
        candidates
            .iter()
            .filter_map(|c| {
                let name = &c.target.provider;
                snapshot
                    .provider(name)
                    .map(|p| (name.clone(), p.kind.as_str()))
            })
            .collect(),
    );
    if candidates.is_empty() {
        record.refund_permit();
        return shape.error(StatusCode::SERVICE_UNAVAILABLE, "upstream_error", NO_MODEL);
    }

    // 3d. The response cache of the route: after access, limits and budgets,
    // so a hit is refused as a call would be. A hit calls no provider.
    //
    // A miss takes the flight of its key before it calls a provider, so
    // concurrent identical calls make one provider call: the others wait,
    // read the cache again and find its answer. The wait is after the
    // limits, so a waiter keeps its concurrency slot while it waits (a slow
    // leader can hold a route's slots). Only the caller that got the flight
    // without waiting (the leader) holds it to the end of the function:
    // through the provider call and `keep`, released on every exit, an error
    // and a dropped future included. A caller that waited and still misses
    // calls on its own, without the flight.
    let cache = cache_plan(snapshot, actor, &call, &resolved, &candidates, &guard);
    let mut flight = None;
    if let Some(plan) = &cache {
        let answered = |state: &AppState, record: &mut Scope| {
            let now = tokio::time::Instant::now().into_std();
            let hit = state.cache.get(&plan.key, now)?;
            let response = render_cached(endpoint, &call, &hit)?;
            state.metrics.cache_hit();
            record.cache_hit(&hit.provider, &hit.model, hit.usage());
            record.guardrails_found(Direction::Output, hit.guardrails.clone());
            Some(response)
        };
        if let Some(response) = answered(state, record) {
            return response;
        }
        // The wait is part of this request's time: it ends with the deadline
        // the request has (the route's total timeout), and what the call
        // that follows may take is what is left of it.
        let deadline = tokio::time::Instant::now() + settings.total_timeout;
        let Ok(held) = timeout_at(deadline, state.flights.hold(plan.key)).await else {
            // Out of time while waiting: the same end as a call that ran out
            // of time before it could try a target.
            return exhausted_response(
                shape,
                Exhausted {
                    attempts: 0,
                    rate_limited: 0,
                    retry_after: None,
                    refused: None,
                },
            );
        };
        settings.total_timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
        if held.waited() {
            state.metrics.cache_flight_wait();
            if let Some(response) = answered(state, record) {
                return response;
            }
            // The answer was not kept (the call that held the flight
            // failed): this caller makes its own call without the flight,
            // so waiters of a failed call do not queue behind each other.
            drop(held);
        } else {
            flight = Some(held);
        }
        state.metrics.cache_miss();
    }
    let _flight = flight;

    // 4. Try the targets in order. The request ends `total_timeout` from now.
    let request_deadline = tokio::time::Instant::now() + settings.total_timeout;
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
                call.response_cap(state),
            )
        },
    )
    .await;
    match served {
        Ok(Served::Whole(mut response)) => {
            let record = scope.as_mut().expect("a whole answer keeps the scope");
            record.usage(response.usage);
            // Before the cache keeps it: the cache never holds an answer the
            // guardrails have not seen.
            match check_output(&guard, &mut response, record, shape, request_deadline).await {
                Ok(Kept::Yes) => keep(state, &cache, record, Answer::Chat(response.clone())),
                // A blocked answer is not kept: every call is checked afresh
                // and recorded as blocked. Nor is one an external guardrail
                // could not check: the next call asks again.
                Ok(Kept::No) => {}
                Err(refusal) => return refusal,
            }
            match endpoint {
                Endpoint::Messages => Json(anthropic::render_response(&response)).into_response(),
                Endpoint::Responses => Json(responses::render_response(
                    &response,
                    &response_id(),
                    now_secs(),
                    &echo_of(&call),
                ))
                .into_response(),
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
        Ok(Served::Image(response)) => {
            let record = scope.as_mut().expect("a whole answer keeps the scope");
            // Cost comes from the usage the provider reports; without it the
            // call is unpriced. The images are never inspected.
            record.usage(response.usage.as_ref().map(|u| Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
            }));
            Json(images::render_response(&response)).into_response()
        }
        Ok(Served::Transcript(mut answer)) => {
            let record = scope.as_mut().expect("a whole answer keeps the scope");
            record.usage(answer.usage.map(|(input_tokens, output_tokens)| Usage {
                input_tokens,
                output_tokens,
            }));
            if let Err(refusal) = check_transcript(
                &guard,
                &mut answer.transcript,
                record,
                shape,
                request_deadline,
            )
            .await
            {
                return refusal;
            }
            let mut response = Response::new(Body::from(answer.transcript.render()));
            response.headers_mut().insert(
                CONTENT_TYPE,
                axum::http::HeaderValue::from_static(answer.transcript.content_type()),
            );
            response
        }
        Ok(Served::Speech(speech)) => {
            // The audio is never inspected; the scope goes with the body and
            // records the call when the body ends.
            let scope = scope.take().expect("a speech keeps the scope");
            speech_response(*speech, scope)
        }
        Ok(Served::Stream(committed)) => {
            if let Some(scope) = scope.as_mut() {
                scope.begin_stream(call.input_estimate());
            }
            let scanner = guard.stream_scanner();
            // An external guardrail on the output sees the whole answer at
            // once, so the stream is held until it has.
            let hold = guard.holds_streams().then(|| {
                Hold::new(
                    guard.clone(),
                    state.max_provider_response_bytes,
                    state.stream_keepalive,
                )
            });
            let guard = StreamRecord {
                guard: (scanner.is_some() || hold.is_some()).then(|| StreamGuard {
                    scanner,
                    hold,
                    checked: guard.refs().to_vec(),
                }),
                scope: scope.take(),
                started: committed.started,
                health: state.health.clone(),
                target: committed.target.clone(),
                breaker: settings.breaker,
            };
            stream_to_caller(*committed, guard, endpoint, echo_of(&call))
        }
        Err(Stop::Fatal(e)) => e.into_response(shape),
        Err(Stop::Exhausted(_)) if wrong_kind(snapshot, key, &call, &candidates) => shape.error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            call.not_supported(),
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
        guardrails: record.output_guardrails(),
    };
    state.cache.put(plan.key, value, plan.ttl, now);
}

/// Joins each maximal run of consecutive text parts of a message into one
/// part (the providers' request builders join them anyway), so a match split
/// across parts is seen whole. Images keep their place between the runs.
fn merge_text_parts(request: &mut ChatRequest) {
    for message in &mut request.messages {
        if message
            .content
            .windows(2)
            .all(|w| !(matches!(w[0], Part::Text(_)) && matches!(w[1], Part::Text(_))))
        {
            continue;
        }
        let mut merged: Vec<Part> = Vec::with_capacity(message.content.len());
        for part in std::mem::take(&mut message.content) {
            match (merged.last_mut(), part) {
                (Some(Part::Text(last)), Part::Text(next)) => last.push_str(&next),
                (_, part) => merged.push(part),
            }
        }
        message.content = merged;
    }
}

/// The text slots of a chat request that guardrails read and may rewrite:
/// every text part (system text and tool results included), the `name` of a
/// message and the arguments of the tool calls in the history. Call
/// [`merge_text_parts`] first.
fn chat_slots(request: &mut ChatRequest) -> Vec<&mut String> {
    let mut slots = Vec::new();
    for message in &mut request.messages {
        for part in &mut message.content {
            if let Part::Text(text) = part {
                slots.push(text);
            }
        }
        if let Some(name) = &mut message.name {
            slots.push(name);
        }
        for call in &mut message.tool_calls {
            slots.push(&mut call.arguments);
        }
    }
    slots
}

const SCAN_FAILED: &str = "The guardrails could not check this request.";
const SCAN_BUSY: &str = "The guardrails are busy checking other requests. Try again shortly.";

/// The chat or embeddings text slots of a call, joined text parts first.
fn input_slots(call: &mut Call) -> Vec<&mut String> {
    match call {
        Call::Chat(r) => {
            merge_text_parts(r);
            chat_slots(r)
        }
        Call::Embed(r) => r.input.iter_mut().collect(),
        Call::Image(r) => vec![&mut r.prompt],
        // The audio is never inspected; the prompt is user text sent on, and
        // a transcript is checked as an answer.
        Call::Transcribe(r, _) => r.prompt.iter_mut().collect(),
        Call::Speech(r) => vec![&mut r.input],
    }
}

fn scan_failed(shape: Shape, failed: ScanFailed) -> Response {
    if failed == ScanFailed::Busy {
        tracing::warn!("a guardrail scan found no free CPU slot");
        return shape.error(StatusCode::SERVICE_UNAVAILABLE, "upstream_error", SCAN_BUSY);
    }
    tracing::error!("a guardrail scan did not finish");
    shape.error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "server_error",
        SCAN_FAILED,
    )
}

/// Checks the input of a call against the rules of `guard` and redacts it in
/// place. A block is the refusal to give the caller. What was found goes to
/// the record either way, and is returned for [`check_input_hooks`].
async fn check_input_rules(
    guard: &Active,
    call: &mut Call,
    record: &mut Scope,
    shape: Shape,
) -> Result<Outcome, Response> {
    if !guard.covers(Direction::Input) {
        return Ok(Outcome::default());
    }
    let mut slots = input_slots(call);
    let outcome = match guard.check_rules(Direction::Input, &mut slots, None).await {
        Ok(outcome) => outcome,
        Err(failed) => return Err(scan_failed(shape, failed)),
    };
    record.guardrails_found(Direction::Input, SideLog::of(guard.refs(), &outcome));
    match outcome.blocked_by {
        Some((_, name)) => Err(shape.guardrail_blocked(&name)),
        None => Ok(outcome),
    }
}

/// Asks the external guardrails about the input, after the rules (whose
/// `outcome` is joined with theirs in the record).
async fn check_input_hooks(
    guard: &Active,
    call: &mut Call,
    mut outcome: Outcome,
    record: &mut Scope,
    shape: Shape,
) -> Result<(), Response> {
    if !guard.has_hooks(Direction::Input) {
        return Ok(());
    }
    let mut slots = input_slots(call);
    let asked = guard.check_hooks(Direction::Input, &mut slots, None).await;
    outcome.merge(&asked);
    record.guardrails_found(Direction::Input, SideLog::of(guard.refs(), &outcome));
    match asked.blocked_by {
        Some((_, name)) => Err(shape.guardrail_blocked(&name)),
        None => Ok(()),
    }
}

/// Whether a checked answer may go in the cache.
enum Kept {
    Yes,
    No,
}

/// Checks a whole answer against `guard`: text and tool-call arguments are
/// redacted in place; a block empties the answer and ends it with
/// `content_filter`. Says whether the answer may be kept: not when it was
/// blocked or when an external guardrail could not check it. No external
/// guardrail runs past `deadline`, the end of the request.
async fn check_output(
    guard: &Active,
    response: &mut ChatResponse,
    record: &mut Scope,
    shape: Shape,
    deadline: tokio::time::Instant,
) -> Result<Kept, Response> {
    if !guard.covers(Direction::Output) {
        return Ok(Kept::Yes);
    }
    let mut slots = vec![&mut response.content];
    slots.extend(response.tool_calls.iter_mut().map(|c| &mut c.arguments));
    let outcome = match guard.check(Direction::Output, slots, Some(deadline)).await {
        Ok(outcome) => outcome,
        Err(failed) => return Err(scan_failed(shape, failed)),
    };
    record.guardrails_found(Direction::Output, SideLog::of(guard.refs(), &outcome));
    if outcome.blocked_by.is_none() {
        return Ok(if outcome.external_failed() {
            Kept::No
        } else {
            Kept::Yes
        });
    }
    response.content.clear();
    response.tool_calls.clear();
    response.finish_reason = Some(FinishReason::ContentFilter);
    Ok(Kept::No)
}

/// Checks the text of a transcript against `guard`, as an answer: redacted
/// in place (subtitles only in their spoken lines, a verbose transcript in
/// its segments), or refused when a guardrail blocks it. Timed words are
/// dropped when a rewrite changed the text they were cut from.
async fn check_transcript(
    guard: &Active,
    transcript: &mut Transcript,
    record: &mut Scope,
    shape: Shape,
    deadline: tokio::time::Instant,
) -> Result<(), Response> {
    if !guard.covers(Direction::Output) {
        return Ok(());
    }
    let before: Vec<String> = transcript.slots().into_iter().map(|s| s.clone()).collect();
    let outcome = match guard
        .check(Direction::Output, transcript.slots(), Some(deadline))
        .await
    {
        Ok(outcome) => outcome,
        Err(failed) => return Err(scan_failed(shape, failed)),
    };
    record.guardrails_found(Direction::Output, SideLog::of(guard.refs(), &outcome));
    if let Some((_, name)) = outcome.blocked_by {
        return Err(shape.guardrail_blocked(&name));
    }
    let after: Vec<String> = transcript.slots().into_iter().map(|s| s.clone()).collect();
    if before != after {
        transcript.drop_words();
    }
    Ok(())
}

/// Passes the audio of a speech call on as it comes, up to the cap and the
/// deadline, and records the call when the body ends. A provider that fails
/// after the first byte ends the body with an error rather than a clean end.
fn speech_response(speech: SpeechStream, scope: Scope) -> Response {
    let SpeechStream {
        content_type,
        mut chunks,
        started,
        deadline,
        idle,
        cap,
    } = speech;
    let body = async_stream::stream! {
        // Dropped with the body when the caller goes away: recorded then.
        let mut scope = Some(scope);
        let mut sent = 0usize;
        loop {
            let by = deadline.min(tokio::time::Instant::now() + idle);
            let (outcome, message) = match timeout_at(by, chunks.next()).await {
                Ok(None) => (AttemptOutcome::Ok, None),
                Ok(Some(Ok(chunk))) => {
                    sent = sent.saturating_add(chunk.len());
                    if sent > cap {
                        tracing::warn!("a speech answer was larger than the cap");
                        (AttemptOutcome::Fatal, Some("The provider response was too large."))
                    } else {
                        yield Ok::<Bytes, std::io::Error>(chunk);
                        continue;
                    }
                }
                Ok(Some(Err(e))) => {
                    tracing::warn!(error = %e.without_url(), "a speech answer was lost");
                    (AttemptOutcome::Retryable, Some("The provider stream was lost."))
                }
                Err(_) => {
                    tracing::warn!("a speech answer ran out of time");
                    (AttemptOutcome::Retryable, Some(TIMED_OUT))
                }
            };
            if let Some(mut scope) = scope.take() {
                scope.set_last_outcome(outcome);
                scope.end_last_attempt(started);
                scope.finish(200);
            }
            if let Some(message) = message {
                yield Err(std::io::Error::other(message));
            }
            break;
        }
    };
    let mut response = Response::new(Body::from_stream(body));
    if let Ok(v) = axum::http::HeaderValue::from_str(&content_type) {
        response.headers_mut().insert(CONTENT_TYPE, v);
    }
    response.headers_mut().insert(
        CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
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
    /// A call that is paid for once was sent and not answered in time: it
    /// may still be billed, and it is not repeated.
    Unfinished,
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
            CallError::Unfinished => shape.error(
                StatusCode::GATEWAY_TIMEOUT,
                "upstream_error",
                "The provider did not answer in time. The request may still be \
                 processed and billed; it was not repeated.",
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
    Image(ImageResponse),
    Transcript(TranscriptAnswer),
    Speech(Box<SpeechStream>),
    Stream(Box<Committed>),
}

/// The audio of a speech call: the provider's answer, to be passed on as it
/// comes.
struct SpeechStream {
    content_type: String,
    chunks: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    /// When the try began.
    started: Instant,
    /// The end of the request.
    deadline: tokio::time::Instant,
    /// The longest wait for the next chunk.
    idle: Duration,
    /// The most audio that is passed on.
    cap: usize,
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
    let once = call.billed_once();
    let first_by = tokio::time::Instant::now() + limits.first_token;
    let wire = Target {
        kind: provider.kind,
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        model: target.model.clone(),
        api_version: provider.api_version.clone(),
    };
    let built = match call {
        Call::Chat(req) => build_request(&wire, req).map(Outgoing::Json),
        Call::Embed(req) => embeddings::build_request(&wire, req).map(Outgoing::Json),
        Call::Image(req) => images::build_request(&wire, req).map(Outgoing::Json),
        Call::Speech(req) => audio::build_speech(&wire, req).map(Outgoing::Json),
        Call::Transcribe(req, chunks) => audio::build_upload(&wire, req)
            .map(|upload| Outgoing::Upload(upload, req.file.clone(), chunks.clone())),
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
            // A connection that was never made sent nothing; any other
            // break may have come after the provider took the request.
            if once && !e.is_connect() {
                return Err(unfinished(None));
            }
            return Err(retryable(CallError::Lost, None));
        }
        Err(_) => {
            tracing::warn!(provider = %provider.name, "provider gave no answer in time");
            if once {
                return Err(unfinished(None));
            }
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
    if matches!(call, Call::Speech(_)) && status < 400 {
        // The audio is passed on as it comes; the answer is the caller's from
        // here, and what goes wrong later ends the body.
        let content_type = upstream
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|t| t.starts_with("audio/") || *t == "application/octet-stream")
            .filter(|t| t.len() <= 100 && t.chars().all(|c| c.is_ascii_graphic() || c == ' '))
            .map(ToString::to_string);
        let format = match call {
            Call::Speech(r) => r.response_format.as_deref(),
            _ => None,
        };
        return Ok(Success {
            value: Served::Speech(Box::new(SpeechStream {
                content_type: content_type
                    .unwrap_or_else(|| audio::speech_content_type(format).to_string()),
                chunks: upstream.bytes_stream().boxed(),
                started,
                deadline: limits.deadline,
                idle: limits.first_token,
                cap: max_response,
            })),
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
            if once {
                return Err(unfinished(Some(status)));
            }
            return Err(retryable(CallError::Lost, Some(status)));
        }
    };
    let parsed = match call {
        Call::Chat(_) => parse_response(provider.kind, status, &bytes).map(Served::Whole),
        Call::Embed(_) => embeddings::parse_response(provider.kind, status, &bytes, &target.model)
            .map(Served::Embeddings),
        Call::Image(_) => images::parse_response(provider.kind, status, &bytes).map(Served::Image),
        Call::Transcribe(req, _) => {
            audio::parse_transcription_response(provider.kind, req.response_format, status, &bytes)
                .map(Served::Transcript)
        }
        // Only a failed speech call is read; a good one is passed on above.
        Call::Speech(_) => Err(audio::parse_speech_error(provider.kind, status, &bytes)),
    };
    match parsed {
        Ok(value) => Ok(Success {
            value,
            status: Some(status),
        }),
        // A 200 that cannot be read was generated and billed: not repeated.
        Err(e @ TranslateError::Malformed(_)) if once && status < 400 => Err(Failure::Fatal {
            error: CallError::Translate(e),
            status: Some(status),
        }),
        Err(e) => Err(failure_of(e, Some(status), retry_after)),
    }
}

/// The failure of a call that is paid for once and was sent: no retry.
fn unfinished(status: Option<u16>) -> Failure<CallError> {
    Failure::Fatal {
        error: CallError::Unfinished,
        status,
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

/// What is sent to a provider.
enum Outgoing {
    Json(HttpRequest),
    /// A multipart upload: the fields, and the file as the chunks it was
    /// read in.
    Upload(audio::UploadRequest, FileInfo, Chunks),
}

async fn send(http: &reqwest::Client, out: Outgoing) -> Result<reqwest::Response, reqwest::Error> {
    match out {
        Outgoing::Json(out) => {
            let mut rb = http.post(&out.url);
            for (k, v) in &out.headers {
                rb = rb.header(k, v);
            }
            rb.body(out.body).send().await
        }
        Outgoing::Upload(upload, file, chunks) => {
            let mut rb = http.post(&upload.url);
            for (k, v) in &upload.headers {
                rb = rb.header(k, v);
            }
            let mut form = reqwest::multipart::Form::new();
            for (k, v) in upload.fields {
                form = form.text(k, v);
            }
            let part = |content_type: &str| {
                let pieces: Vec<Result<Bytes, std::io::Error>> =
                    chunks.iter().cloned().map(Ok).collect();
                reqwest::multipart::Part::stream_with_length(
                    reqwest::Body::wrap_stream(futures::stream::iter(pieces)),
                    file.len as u64,
                )
                .file_name(file.name.clone())
                .mime_str(content_type)
            };
            let part = part(&file.content_type)
                .or_else(|_| part("application/octet-stream"))
                .expect("a fixed media type parses");
            rb.multipart(form.part("file", part)).send().await
        }
    }
}

fn stream_id(prefix: &str) -> String {
    let mut bytes = [0u8; 12];
    crate::secrets::fill_random(&mut bytes);
    format!("{prefix}-{}", hex::encode(bytes))
}

/// `resp_` and 24 hex digits, as OpenAI's response ids.
fn response_id() -> String {
    let mut bytes = [0u8; 12];
    crate::secrets::fill_random(&mut bytes);
    format!("resp_{}", hex::encode(bytes))
}

/// The settings of the request a response object repeats.
fn echo_of(call: &Call) -> responses::Echo {
    match call {
        Call::Chat(r) => responses::Echo::of(r),
        // Only a chat call is answered on `/v1/responses`.
        Call::Embed(_) | Call::Image(_) | Call::Transcribe(..) | Call::Speech(_) => {
            responses::Echo::of(&ChatRequest {
                model: String::new(),
                messages: Vec::new(),
                max_tokens: None,
                temperature: None,
                top_p: None,
                stop: None,
                stream: false,
                tools: Vec::new(),
                tool_choice: None,
                parallel_tool_calls: None,
                response_format: None,
                reasoning_effort: None,
            })
        }
    }
}

/// Renders a stream in the caller's format.
enum StreamFormat {
    OpenAi {
        id: String,
        model: String,
        created: u64,
    },
    Anthropic(anthropic::StreamRenderer),
    Responses(responses::StreamRenderer),
}

impl StreamFormat {
    fn new(endpoint: Endpoint, model: &str, echo: responses::Echo) -> Self {
        match endpoint {
            Endpoint::Messages => {
                StreamFormat::Anthropic(anthropic::StreamRenderer::new(&stream_id("msg"), model))
            }
            Endpoint::Responses => StreamFormat::Responses(responses::StreamRenderer::new(
                &response_id(),
                model,
                now_secs(),
                echo,
            )),
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
            StreamFormat::Responses(r) => r.render(ev),
        }
    }

    fn error(&self, message: &str) -> String {
        match self {
            StreamFormat::OpenAi { .. } => openai::render_stream_error(message),
            StreamFormat::Anthropic(_) => anthropic::render_stream_error(message),
            StreamFormat::Responses(r) => r.error(message),
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

/// The guardrails of a stream: a scanner with hold-back, and the guardrails
/// it runs, for the record.
struct StreamGuard {
    /// The rules guardrails over the output, when there are any.
    scanner: Option<StreamScanner>,
    /// The answer held for the external guardrails over the output.
    hold: Option<Hold>,
    checked: Vec<GuardrailRef>,
}

/// An answer held back until the external guardrails have seen all of it.
/// What the rules scanner lets through is kept here instead of being sent,
/// appended to one buffer for the text and one per tool call as it arrives
/// (so the memory held is about the bytes counted against the cap); at the end
/// the hooks are asked once, and the answer is sent as one text and one piece
/// of arguments per tool call (with whatever they redacted), or replaced by a
/// `content_filter` ending.
struct Hold {
    active: Active,
    /// The order things came in; text and tool arguments are in the buffers.
    slots: Vec<Slot>,
    text: String,
    /// The arguments of each tool call, in the order the calls first appeared.
    tools: Vec<(u32, String)>,
    /// Where each call index is in `tools`.
    tool_at: HashMap<u32, usize>,
    /// The answer text has a slot.
    has_text: bool,
    /// Bytes of text and arguments held.
    bytes: usize,
    /// What the slots themselves cost (each one, and the ids and names of
    /// the calls), counted against the cap with `bytes`.
    overhead: usize,
    /// The most that is held: the provider response cap.
    cap: usize,
    outcome: Outcome,
    /// The cap was passed and the guardrails fail open: the rest goes on
    /// (already through the rules scanner) without the hooks.
    passing: bool,
    /// How often an SSE comment is sent while the answer is held.
    keepalive: Duration,
}

/// What one held slot costs against the cap, besides its bytes (an event or a
/// tool call's buffer, with its place in the lists).
const SLOT_COST: usize = 64;

/// Where a held thing goes back when the answer is sent.
enum Slot {
    Event(StreamEvent),
    Text,
    Tool(u32),
}

impl Hold {
    fn new(active: Active, cap: usize, keepalive: Duration) -> Self {
        Hold {
            active,
            slots: Vec::new(),
            text: String::new(),
            tools: Vec::new(),
            tool_at: HashMap::new(),
            has_text: false,
            bytes: 0,
            overhead: 0,
            cap,
            outcome: Outcome::default(),
            passing: false,
            keepalive,
        }
    }

    fn push(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::Delta { text } => {
                if !self.has_text {
                    self.has_text = true;
                    self.slots.push(Slot::Text);
                    self.overhead += SLOT_COST;
                }
                self.bytes += text.len();
                self.text.push_str(&text);
            }
            StreamEvent::ToolCallDelta { index, arguments } => {
                self.bytes += arguments.len();
                match self.tool_at.get(&index) {
                    Some(&at) => self.tools[at].1.push_str(&arguments),
                    None => {
                        self.tool_at.insert(index, self.tools.len());
                        self.slots.push(Slot::Tool(index));
                        self.tools.push((index, arguments));
                        self.overhead += SLOT_COST;
                    }
                }
            }
            other => {
                if let StreamEvent::ToolCallStart { id, name, .. } = &other {
                    self.overhead += id.len() + name.len();
                }
                self.overhead += SLOT_COST;
                self.slots.push(Slot::Event(other));
            }
        }
    }

    /// Forgets everything held.
    fn clear(&mut self) {
        self.slots = Vec::new();
        self.text = String::new();
        self.tools = Vec::new();
        self.tool_at = HashMap::new();
        self.has_text = false;
        self.bytes = 0;
        self.overhead = 0;
    }

    /// The held answer as events, in the order things came in. `texts` holds
    /// the answer text first, then the arguments of each tool call.
    fn events(&mut self, texts: Vec<String>) -> Vec<StreamEvent> {
        let indices: Vec<u32> = self.tools.iter().map(|(i, _)| *i).collect();
        let mut texts: Vec<Option<String>> = texts.into_iter().map(Some).collect();
        let slots = std::mem::take(&mut self.slots);
        self.clear();
        let mut events = Vec::with_capacity(slots.len());
        for slot in slots {
            match slot {
                Slot::Event(ev) => events.push(ev),
                Slot::Text => {
                    if let Some(text) = texts[0].take().filter(|t| !t.is_empty()) {
                        events.push(StreamEvent::Delta { text });
                    }
                }
                Slot::Tool(index) => {
                    let at = indices.iter().position(|i| *i == index).map(|p| p + 1);
                    if let Some(arguments) =
                        at.and_then(|at| texts[at].take()).filter(|a| !a.is_empty())
                    {
                        events.push(StreamEvent::ToolCallDelta { index, arguments });
                    }
                }
            }
        }
        events
    }

    /// The held buffers as the texts a hook is asked about.
    fn take_texts(&mut self) -> Vec<String> {
        let mut texts = vec![std::mem::take(&mut self.text)];
        texts.extend(self.tools.iter_mut().map(|(_, a)| std::mem::take(a)));
        texts
    }

    /// Takes what the scanner let through. Nothing is released before the
    /// end of the answer, except when the cap is passed and the guardrails
    /// fail open: then what is held goes out and every later event follows
    /// as it comes (the closing one included).
    async fn step(&mut self, guarded: Guarded, deadline: tokio::time::Instant) -> Guarded {
        if self.passing {
            return guarded;
        }
        if guarded.cut {
            // A rule ended the answer: nothing of it was sent and nothing
            // will be.
            self.clear();
            return guarded;
        }
        let mut out = Vec::new();
        for ev in guarded.events {
            if self.passing {
                out.push(ev);
                continue;
            }
            if let StreamEvent::Done {
                finish_reason,
                usage,
            } = ev
            {
                let done = self.finish(finish_reason, usage, deadline).await;
                out.extend(done.events);
                return Guarded {
                    events: out,
                    cut: done.cut,
                };
            }
            self.push(ev);
            if self.bytes + self.overhead > self.cap {
                let failed = self.active.fail_output_buffer();
                let blocked = failed.blocked_by.is_some();
                self.outcome.merge(&failed);
                if blocked {
                    self.clear();
                    return StreamGuard::cut(None);
                }
                // Fail open: what is held was never checked by the hooks,
                // and goes out; the failure is flagged.
                self.passing = true;
                let texts = self.take_texts();
                out.extend(self.events(texts));
            }
        }
        Guarded {
            events: out,
            cut: false,
        }
    }

    async fn finish(
        &mut self,
        finish_reason: Option<FinishReason>,
        usage: Option<Usage>,
        deadline: tokio::time::Instant,
    ) -> Guarded {
        // The answer text first, then the arguments of each tool call in the
        // order they began.
        let mut texts = self.take_texts();
        let asked = self
            .active
            .check_externals(Direction::Output, &mut texts, Some(deadline))
            .await;
        self.outcome.merge(&asked);
        if asked.blocked_by.is_some() {
            self.clear();
            // The provider's usage is the truth for the whole answer.
            return StreamGuard::cut(usage);
        }
        let mut events = self.events(texts);
        events.push(StreamEvent::Done {
            finish_reason,
            usage,
        });
        Guarded { events, cut: false }
    }
}

/// What the scanner makes of one event of the stream.
struct Guarded {
    /// What to send in its place, in order.
    events: Vec<StreamEvent>,
    /// A guardrail ended the answer: `events` ends with the closing event.
    cut: bool,
}

impl StreamGuard {
    /// What the rules and the external guardrails found so far.
    fn outcome(&self) -> Outcome {
        let mut outcome = self
            .scanner
            .as_ref()
            .map(|s| s.outcome().clone())
            .unwrap_or_default();
        if let Some(hold) = &self.hold {
            outcome.merge(&hold.outcome);
        }
        outcome
    }

    /// How often to send a keepalive while the answer is held, or `None`
    /// when nothing is being held.
    fn keepalive(&self) -> Option<Duration> {
        self.hold
            .as_ref()
            .filter(|h| !h.passing)
            .map(|h| h.keepalive)
    }

    /// Holds back what the external guardrails have yet to see.
    async fn held(&mut self, guarded: Guarded, deadline: tokio::time::Instant) -> Guarded {
        match self.hold.as_mut() {
            Some(hold) => hold.step(guarded, deadline).await,
            None => guarded,
        }
    }

    /// The closing event of an answer a guardrail ended.
    fn cut(usage: Option<Usage>) -> Guarded {
        Guarded {
            events: vec![StreamEvent::Done {
                finish_reason: Some(FinishReason::ContentFilter),
                usage,
            }],
            cut: true,
        }
    }

    fn released(release: Release, event: impl FnOnce(String) -> StreamEvent) -> Guarded {
        if release.blocked.is_some() {
            return Self::cut(None);
        }
        Guarded {
            events: if release.text.is_empty() {
                Vec::new()
            } else {
                vec![event(release.text)]
            },
            cut: false,
        }
    }

    /// The clean text the scanner still holds when the provider fails: it is
    /// released as a normal end would release it (and scanned once more). An
    /// answer held for an external guardrail is dropped instead, because the
    /// guardrail has not seen it.
    fn tail_on_error(&mut self) -> Vec<StreamEvent> {
        if self.hold.is_some() {
            return Vec::new();
        }
        let Some(scanner) = self.scanner.as_mut() else {
            return Vec::new();
        };
        let tail = scanner.finish();
        if tail.blocked.is_some() {
            return Vec::new();
        }
        let mut events = Vec::new();
        if !tail.text.is_empty() {
            events.push(StreamEvent::Delta { text: tail.text });
        }
        for (index, arguments) in tail.tools {
            events.push(StreamEvent::ToolCallDelta { index, arguments });
        }
        events
    }

    /// Text and tool-call arguments go through the scanner and come out
    /// redacted, a little later (the scanner holds back the end of what it
    /// has seen until it can tell it is clean). The last event releases what
    /// is held. A block ends the stream with `content_filter`; what was sent
    /// before stays sent, because it was clean.
    fn apply(&mut self, event: StreamEvent) -> Guarded {
        let Some(scanner) = self.scanner.as_mut() else {
            return Guarded {
                events: vec![event],
                cut: false,
            };
        };
        match event {
            StreamEvent::Delta { text } => {
                let release = scanner.push_text(&text);
                Self::released(release, |text| StreamEvent::Delta { text })
            }
            StreamEvent::ToolCallDelta { index, arguments } => {
                let release = scanner.push_tool_args(index, &arguments);
                Self::released(release, |arguments| StreamEvent::ToolCallDelta {
                    index,
                    arguments,
                })
            }
            StreamEvent::Done {
                finish_reason,
                usage,
            } => {
                let tail = scanner.finish();
                if tail.blocked.is_some() {
                    // The provider's usage is the truth for the whole answer.
                    return Self::cut(usage);
                }
                let mut events = Vec::new();
                if !tail.text.is_empty() {
                    events.push(StreamEvent::Delta { text: tail.text });
                }
                for (index, arguments) in tail.tools {
                    events.push(StreamEvent::ToolCallDelta { index, arguments });
                }
                events.push(StreamEvent::Done {
                    finish_reason,
                    usage,
                });
                Guarded { events, cut: false }
            }
            other => Guarded {
                events: vec![other],
                cut: false,
            },
        }
    }
}

/// Holds a stream's record until the stream ends. Dropping it, which is what
/// happens when the caller goes away, emits the record as a gone caller.
struct StreamRecord {
    /// Scans the answer as it streams, when a rule applies to outputs.
    guard: Option<StreamGuard>,
    scope: Option<Scope>,
    started: Instant,
    health: Arc<dyn HealthStore>,
    target: TargetRef,
    breaker: crate::routing::BreakerSettings,
}

impl StreamRecord {
    /// What the scanner holds of the answer when the provider fails, as events
    /// to send before the error (counted as streamed).
    fn error_tail(&mut self) -> Vec<StreamEvent> {
        let Some(guard) = self.guard.as_mut() else {
            return Vec::new();
        };
        let events = guard.tail_on_error();
        for ev in &events {
            match ev {
                StreamEvent::Delta { text } => self.streamed(text.chars().count()),
                StreamEvent::ToolCallDelta { arguments, .. } => {
                    self.streamed(arguments.chars().count());
                }
                _ => {}
            }
        }
        events
    }

    /// Puts what the stream scanner found so far in the record.
    fn guard_found(&mut self) {
        if let (Some(guard), Some(scope)) = (self.guard.as_ref(), self.scope.as_mut()) {
            scope.guardrails_found(
                Direction::Output,
                SideLog::of(&guard.checked, &guard.outcome()),
            );
        }
    }

    /// A guardrail ends the answer: the provider is dropped mid-answer.
    fn cut_short(&mut self) {
        if let Some(scope) = self.scope.as_mut() {
            scope.cut_short();
        }
    }

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
        self.guard_found();
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
        self.guard_found();
        // The caller went away before the end: the attempt did not finish.
        if let Some(scope) = self.scope.as_mut() {
            scope.set_last_outcome(AttemptOutcome::Retryable);
            scope.end_last_attempt(self.started);
        }
    }
}

/// An SSE comment: ignored by every client, it only keeps the connection busy.
const KEEPALIVE: &str = ": keepalive\n\n";

/// Forwards the provider's stream to the caller as server-sent events in the
/// format of the endpoint it came in on.
///
/// The body owns the upstream response, so when the caller disconnects and the
/// body is dropped, the provider request is dropped with it.
fn stream_to_caller(
    committed: Committed,
    record: StreamRecord,
    endpoint: Endpoint,
    echo: responses::Echo,
) -> Response {
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
        let mut format = StreamFormat::new(endpoint, &target.model, echo);
        let mut pending = Some((events, error));
        loop {
            let (events, error) = match pending.take() {
                Some(first) => first,
                None => {
                    // While an answer is held the caller hears nothing, so a
                    // comment goes out now and then to keep a proxy from
                    // cutting the connection for being idle.
                    let keepalive = record.guard.as_ref().and_then(StreamGuard::keepalive);
                    let waited = {
                        let mut next = std::pin::pin!(timeout_at(deadline, chunks.next()));
                        let mut tick = std::pin::pin!(tokio::time::sleep(
                            keepalive.unwrap_or(Duration::from_secs(3600))
                        ));
                        loop {
                            tokio::select! {
                                r = &mut next => break r,
                                _ = &mut tick, if keepalive.is_some() => {
                                    yield Ok::<String, Infallible>(KEEPALIVE.to_string());
                                    tick.as_mut().reset(
                                        tokio::time::Instant::now() + keepalive.unwrap_or_default(),
                                    );
                                }
                            }
                        }
                    };
                    let chunk = match waited {
                        Err(_) => {
                            tracing::warn!(provider = %provider, "request ran out of time during the stream");
                            for ev in record.error_tail() {
                                yield Ok::<String, Infallible>(format.event(&ev));
                            }
                            record.end_out_of_time();
                            yield Ok::<String, Infallible>(format.error(TIMED_OUT));
                            return;
                        }
                        Ok(None) => {
                            let tail = decoder.finish();
                            if tail.is_empty() {
                                for ev in record.error_tail() {
                                    yield Ok::<String, Infallible>(format.event(&ev));
                                }
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
                            for ev in record.error_tail() {
                                yield Ok::<String, Infallible>(format.event(&ev));
                            }
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
                match &ev {
                    StreamEvent::Delta { text } => record.streamed(text.chars().count()),
                    StreamEvent::ToolCallDelta { arguments, .. } => {
                        record.streamed(arguments.chars().count())
                    }
                    _ => {}
                }
                let guarded = match record.guard.as_mut() {
                    Some(guard) => guard.apply(ev),
                    None => Guarded { events: vec![ev], cut: false },
                };
                let keepalive = record.guard.as_ref().and_then(StreamGuard::keepalive);
                let guarded = match record.guard.as_mut() {
                    Some(guard) => {
                        let mut held = std::pin::pin!(guard.held(guarded, deadline));
                        let mut tick = std::pin::pin!(tokio::time::sleep(
                            keepalive.unwrap_or(Duration::from_secs(3600))
                        ));
                        loop {
                            tokio::select! {
                                r = &mut held => break r,
                                _ = &mut tick, if keepalive.is_some() => {
                                    yield Ok::<String, Infallible>(KEEPALIVE.to_string());
                                    tick.as_mut().reset(
                                        tokio::time::Instant::now() + keepalive.unwrap_or_default(),
                                    );
                                }
                            }
                        }
                    }
                    None => guarded,
                };
                if guarded.cut {
                    record.cut_short();
                }
                for ev in guarded.events {
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
            }
            // An error that ended the stream, after the events before it.
            if let Some(e) = error {
                for ev in record.error_tail() {
                    yield Ok::<String, Infallible>(format.event(&ev));
                }
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

    #[test]
    fn a_held_answer_is_appended_to_one_buffer_not_kept_event_by_event() {
        let mut hold = Hold::new(Active::default(), 1 << 20, Duration::from_secs(10));
        for i in 0..20_000 {
            hold.push(StreamEvent::Delta { text: "ab".into() });
            hold.push(StreamEvent::ToolCallDelta {
                index: i % 2,
                arguments: "cd".into(),
            });
        }
        // One slot for the text, one for each of two tool calls.
        assert_eq!(hold.slots.len(), 3);
        assert_eq!(hold.text.len(), 40_000);
        assert_eq!(hold.bytes, 80_000);
        let texts = hold.take_texts();
        let events = hold.events(texts);
        assert_eq!(events.len(), 3);
    }

    #[tokio::test]
    async fn a_hold_with_many_tool_calls_is_bounded_in_time_and_memory() {
        // 200 000 tool calls, none with any arguments: nothing counted as
        // bytes before, so nothing capped them and every one searched the list
        let started = std::time::Instant::now();
        let mut hold = Hold::new(Active::default(), 1 << 20, Duration::from_secs(10));
        let mut pushed = 0u32;
        for i in 0..200_000u32 {
            hold.push(StreamEvent::ToolCallStart {
                index: i,
                id: format!("call_{i}"),
                name: "look".into(),
            });
            hold.push(StreamEvent::ToolCallDelta {
                index: i,
                arguments: String::new(),
            });
            pushed += 1;
            if hold.bytes + hold.overhead > hold.cap {
                break;
            }
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        // the cap is reached long before 200 000 calls, and what is held is
        // about the cap, not a multiple of it
        assert!(pushed < 40_000, "{pushed} calls held before the cap");
        assert!(hold.slots.len() < 120_000, "{}", hold.slots.len());
        // many deltas for one call still cost their bytes only
        let mut hold = Hold::new(Active::default(), 1 << 20, Duration::from_secs(10));
        hold.push(StreamEvent::ToolCallDelta {
            index: 7,
            arguments: "x".into(),
        });
        let before = hold.overhead;
        for _ in 0..1000 {
            hold.push(StreamEvent::ToolCallDelta {
                index: 7,
                arguments: "x".into(),
            });
        }
        assert_eq!(hold.overhead, before);
        assert_eq!(hold.bytes, 1001);
    }

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
