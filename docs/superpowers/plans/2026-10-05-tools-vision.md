# Tools, Vision and Cache Single-Flight Implementation Plan (plan 10, v2.0.0-beta.2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development via the project skill `uf-workflow` (Sonnet implementers and reviewers, Opus final review). Conventions: `docs/CONVENTIONS.md`. Run mode: back-to-back, no owner stops except push/merge/deploy.

**Goal:** Tool (function) calling and image input on both chat formats (OpenAI Chat Completions, Anthropic Messages), streaming and not, across every provider kind (OpenAI and compatibles, Azure, Anthropic, Gemini); single-flight for the response cache; the console Playground and the three clients able to use both.

**Architecture:** The common types in `crates/translate/src/types.rs` grow content parts, tool definitions, tool calls and tool-call stream events; each ingress parses into them and renders from them, each provider adapter builds from them and decodes into them, so any ingress works with any provider. The gateway learns nothing provider-specific: it keys the cache on the new fields, estimates their tokens, and serializes concurrent cache misses per key.

**Tech Stack:** Rust (serde_json, axum 0.8, tokio), React/TS console (TanStack, shadcn), PyO3/maturin, wasm-bindgen + TypeScript.

**Spec:** `docs/superpowers/specs/2026-09-28-gateway-v2-design.md` sections 3 (endpoints, providers), 5 (translation: "any ingress with any provider"), 9 (cache). This plan lifts the README known limit "Text chat only: no tools or images" and "There is no single-flight".

## Global Constraints

- One common model, no provider JSON passthrough: a field the target cannot express is an `Unsupported` error (400 to the caller), never silently dropped. Existing `rejects_*` tests change from "tools are rejected" to "tools are carried"; tests that pin unknown-field rejection stay.
- The gateway never fetches an image URL (no SSRF): `http(s)` URLs are passed to providers that take URLs (OpenAI, Azure, Anthropic `source.type = "url"`); for Gemini only `data:` URLs are accepted, an `http(s)` URL is `Unsupported("Gemini takes images as data: URLs only")`.
- Accepted image media types: `image/png`, `image/jpeg`, `image/gif`, `image/webp`. A `data:` URL must be `data:<type>;base64,<data>`; anything else is `InvalidRequest`. Base64 is not decoded or re-encoded (passed through as text), only checked for the alphabet.
- Tool arguments are carried as the JSON TEXT the model produced (`arguments: String`), never re-serialized, so a model's malformed JSON reaches the caller as is. Where a provider gives an object (Anthropic `input`, Gemini `args`) it is serialized once with `serde_json::to_string`; where a provider needs an object (Anthropic, Gemini history) the text is parsed and a parse failure is `InvalidRequest("tool call arguments are not valid JSON")`.
- Tool call ids: kept as the provider gave them. Gemini gives none: ids are `call_<n>` with `n` the 0-based index of the call in the answer (stable, so a cached answer is identical). A Gemini tool result names its function: it is found by `tool_call_id` among earlier assistant tool calls of the same request; not found is `InvalidRequest`.
- Request body limit stays `DEFAULT_MAX_BODY_BYTES` (10 MiB); README states that images count toward it and that over it is 413.
- Logs keep metadata only (no tool names, arguments or images are logged). The finish reason `tool_calls` already exists.
- Cache: every new request field is part of `CacheKey::chat` (the destructuring there must keep failing to compile when a field is added). Image bytes are hashed like text. A cached answer with tool calls is answered like any other.
- Single-flight: per `CacheKey`, one caller calls the provider; concurrent callers with the same key wait for it, then read the cache; if the answer was not kept (error, not cacheable) each waiter makes its own call. A waiter whose own caller leaves stops waiting. No new config option.
- Commits by the owner identity only, no assistant trailers (docs/CONVENTIONS.md, Git). Clean `git archive HEAD` must build.
- Version becomes `2.0.0-beta.2` everywhere at the last task (Cargo workspace, `crates/client-py` → `2.0.0b2`, `clients/ts/package.json`, CHANGELOG).

## Review Focus

1. A tool conversation that crosses formats: OpenAI-shaped request with `tool` messages sent to an Anthropic provider, and an Anthropic-shaped request with `tool_result` blocks sent to an OpenAI provider, round-trip ids and arguments exactly (Tasks 3 and 4 own one test each).
2. A streamed answer that interleaves text and two parallel tool calls renders correct indices on both ingress formats: OpenAI `tool_calls[].index` 0 and 1 with `id`/`name` only on the first chunk of each, Anthropic one `content_block_start/stop` pair per block with increasing `index` (Task 2 and Task 1 own them).
3. An assistant message with tool calls and empty or null content (OpenAI sends `"content": null`) is valid, and a `tool` message without `tool_call_id` is `InvalidRequest` (Task 1).
4. An image to Gemini as an `https://` URL is a 400 with the Gemini message, before any provider call, and no URL is ever fetched by the gateway (Task 5, plus a gateway test that counts upstream calls in Task 6).
5. Fifty concurrent identical cacheable calls reach the provider once; when that one call fails, the waiters each call the provider and none hangs (Task 7).

---

### Task 1: Common types and the OpenAI ingress

**Files:**
- Modify: `crates/translate/src/types.rs`, `crates/translate/src/ingress/openai.rs`
- Modify (compile only, behavior unchanged for text): `crates/gateway/src/cache/key.rs`, `crates/gateway/src/cache/mod.rs`, `crates/gateway/src/proxy.rs`, `crates/translate/src/provider/{openai,anthropic,gemini}.rs`, `crates/translate/src/ingress/anthropic.rs`, `crates/client/src/request.rs`, `crates/client-py/src/lib.rs`, `crates/client-wasm/src/api.rs`, and every test helper that builds a `Message`/`ChatResponse`.

**Interfaces (Produces — every later task uses exactly these):**

```rust
// types.rs
pub enum Role { System, User, Assistant, Tool }          // serde lowercase

pub enum ImageSource {
    /// An http(s) URL, passed on as is.
    Url(String),
    /// `data:<media_type>;base64,<data>`, split.
    Base64 { media_type: String, data: String },
}

pub enum Part {
    Text(String),
    Image(ImageSource),
}

pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The arguments as the JSON text the model produced.
    pub arguments: String,
}

pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    /// JSON Schema of the arguments; `{"type":"object"}` when not given.
    pub parameters: serde_json::Value,
}

pub enum ToolChoice { Auto, None, Required, Tool(String) }

pub struct Message {
    pub role: Role,
    pub content: Vec<Part>,            // may be empty for an assistant message with tool calls
    pub name: Option<String>,
    pub tool_calls: Vec<ToolCall>,     // assistant only
    pub tool_call_id: Option<String>,  // Role::Tool only, required there
}

impl Message {
    pub fn text(role: Role, text: impl Into<String>) -> Self;   // one Text part, no tools
    /// Concatenation of the Text parts.
    pub fn joined_text(&self) -> String;
    pub fn has_images(&self) -> bool;
}

pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Vec<String>>,
    pub stream: bool,
    pub tools: Vec<Tool>,
    pub tool_choice: Option<ToolChoice>,
    pub parallel_tool_calls: Option<bool>,
}

pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<FinishReason>,
    pub usage: Option<Usage>,
}

pub enum StreamEvent {
    Delta { text: String },
    /// A tool call begins. `index` counts tool calls of this answer from 0.
    ToolCallStart { index: u32, id: String, name: String },
    /// More argument text for the call at `index`.
    ToolCallDelta { index: u32, arguments: String },
    Done { finish_reason: Option<FinishReason>, usage: Option<Usage> },
}

/// Parses `data:<type>;base64,<data>` or an http(s) URL. Shared by both ingresses.
pub fn image_source(url: &str) -> Result<ImageSource, TranslateError>;
pub const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
```

`ImageSource`, `Part`, `ToolCall`, `Tool`, `ToolChoice` derive `Debug, Clone, PartialEq, Serialize, Deserialize` like their neighbours.

**OpenAI ingress (`parse_request`)** accepts, on top of today:
- top-level `tools` (array of `{"type":"function","function":{"name","description"?,"parameters"?,"strict"?}}`; `strict` accepted and ignored; any other `type` is `Unsupported`), `tool_choice` (`"auto"|"none"|"required"` or `{"type":"function","function":{"name"}}`), `parallel_tool_calls` (bool). Remove these three from the rejection lists.
- message role `tool` (needs `tool_call_id`, string content or text parts) → `Role::Tool`.
- assistant `tool_calls` (`[{"id","type":"function","function":{"name","arguments"}}]`); `content` may then be null, absent, or `""`. `function_call` (legacy) stays `Unsupported`.
- content parts `{"type":"image_url","image_url":{"url", "detail"?}}` (`detail` accepted and ignored) on `user` messages only; elsewhere `InvalidRequest("images are only allowed in user messages")`.
- Order of parts is kept; text parts are no longer joined.

**OpenAI ingress (render)**: `render_response` puts `tool_calls` (`[{"id","type":"function","function":{"name","arguments"}}]`) in `message` when non-empty and `"content": null` when the text is empty and there are tool calls. `render_stream_event` renders `ToolCallStart` as `delta.tool_calls:[{"index","id","type":"function","function":{"name","arguments":""}}]` and `ToolCallDelta` as `delta.tool_calls:[{"index","function":{"arguments"}}]`.

**Compile-through changes in this task** (no new behavior): provider adapters build text from `joined_text()` and return `Unsupported("tools are not supported yet by this provider")` / `Unsupported("images are not supported yet by this provider")` when the request carries tools, tool messages, tool calls or images (Tasks 3–5 replace these); `CacheKey::chat` hashes the new fields (tag numbers 20+: per message `tool_calls` count, then id/name/arguments; `tool_call_id` optional; parts as tag 21 text / 22 image-url / 23 image-base64 media type + 24 data; request `tools` count + name/description/parameters as canonical `serde_json::to_vec`, `tool_choice` as its string form, `parallel_tool_calls` optional bool); `Cached::size` adds tool call bytes; `Call::input_estimate` counts text chars of parts, tool call names+arguments, tool definitions (`serde_json::to_string` length), and 1 000 tokens per image (constant `IMAGE_TOKEN_ESTIMATE`); stream recording counts `ToolCallDelta.arguments` chars like text; Anthropic ingress keeps rejecting non-text blocks (Task 2 replaces); clients map the new types (Rust builder unchanged in API; Py/WASM output `tool_calls: []` and ignore tool events — Task 9 adds the API).

- [ ] **Step 1: Failing tests in `ingress/openai.rs`** — replace `rejects_tools_and_images_instead_of_dropping_them` with:
  - `parses_tools_and_tool_choice` (two tools, one without `parameters` → `{"type":"object"}`; each `tool_choice` form; `parallel_tool_calls:false`).
  - `parses_a_tool_conversation` (user → assistant `content:null` + two tool calls → two `tool` messages → user) and checks ids, names, argument text byte-identical (use arguments `"{\"a\": 1,  \"b\":[ ]}"` with odd spacing).
  - `tool_message_without_tool_call_id_is_invalid`, `image_in_assistant_message_is_invalid`, `non_function_tool_is_unsupported`, `legacy_function_call_is_unsupported`.
  - `parses_image_parts_in_order` (text, https image, data:image/png image, text → four parts in order; `detail` ignored).
  - `rejects_bad_data_urls` (`data:text/plain;base64,QQ==`, `data:image/png,raw`, `data:image/png;base64,***`).
  - `renders_tool_calls_in_response` (content null when empty) and `renders_tool_call_stream_events` (start then two deltas; `id`/`name`/`type` only on start).
  - Remove `tools`, `tool_choice`, `parallel_tool_calls`, `tool_calls` from the `rejects_every_unsupported_top_level_field` / `rejects_top_level_fields_that_are_not_on_the_allowlist` / `rejects_unsupported_message_fields` tables; keep the rest.
- [ ] **Step 2:** `cargo test -p ultrafast-translate ingress::openai` → the new tests fail to compile / fail.
- [ ] **Step 3:** Implement the types, `image_source`, the parse and render changes, and the compile-through changes listed above. In `types.rs` add tests: `image_source_parses_both_forms`, `joined_text_skips_images`.
- [ ] **Step 4:** `cargo test --all`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check` all green; `cargo build -p ultrafast-client-wasm --target wasm32-unknown-unknown` if the target is installed (else note it in the report; CI covers it).
- [ ] **Step 5:** In `crates/gateway/src/cache/key.rs` add `tools_images_and_tool_calls_change_the_key` (each new field changed alone changes the key) next to the existing per-field table.
- [ ] **Step 6: Commit** `feat(translate): tools, tool calls and images in the common model and the OpenAI format`.

### Task 2: Anthropic ingress

**Files:** Modify `crates/translate/src/ingress/anthropic.rs`.

**Interfaces:** Consumes Task 1 types. Produces no new public names; `StreamRenderer` keeps `new(id, model)` and its render method signature.

Parse:
- `tools`: `[{"name","description"?,"input_schema"}]` → `Tool { parameters: input_schema }`; an entry with a `type` field (server tools such as `web_search_20250305`, `bash_*`) is `InvalidRequest("server tool '<type>' is not supported")`. `cache_control` on a tool is accepted and ignored.
- `tool_choice`: `{"type":"auto"}`→Auto, `{"type":"any"}`→Required, `{"type":"none"}`→None, `{"type":"tool","name"}`→Tool(name); `disable_parallel_tool_use: true` inside it → `parallel_tool_calls = Some(false)`.
- Blocks: `image` with `source.type` `base64` (`media_type`, `data`) or `url` (`url`) on user messages; `tool_use` (`id`,`name`,`input` object → arguments `serde_json::to_string(input)`) on assistant messages → `Message.tool_calls`, text blocks before/after them → content; `tool_result` (`tool_use_id`, `content` string or text blocks, `is_error` bool) on user messages → each becomes its own `Role::Tool` message (`tool_call_id = tool_use_id`, content = text; when `is_error` is true prefix nothing and keep the text — note in a comment that the error flag does not survive into other formats) placed BEFORE the remaining parts of that user message, which become a following `Role::User` message if non-empty. `tool_result` content containing an image is `Unsupported`. `thinking`/`document`/other blocks keep failing with the existing message.
- `system` stays text only.

Render:
- `render_response`: content blocks = a text block when text is non-empty, then one `{"type":"tool_use","id","name","input"}` per call (`input` = arguments parsed as JSON. Ruling: arguments that are not a JSON object are rendered as `input: {}` and a `warn` is logged without the arguments; this only happens on a provider bug, and Anthropic clients require an object. In a stream the raw `partial_json` is passed through unchanged, since it cannot be checked until the end). Empty text with no tool calls still renders one empty text block as today.
- `StreamRenderer`: text deltas open block 0 as `text` when first needed; a `ToolCallStart` closes the open block (`content_block_stop`) and opens `{"type":"tool_use","id","name","input":{}}` at the next index; `ToolCallDelta` renders `content_block_delta` with `{"type":"input_json_delta","partial_json": arguments}` on the block of that tool index; text after a tool call opens a new text block. `Done` closes the open block, then `message_delta` (stop_reason `tool_use` for ToolCalls) and `message_stop` as today.

- [ ] **Step 1: Failing tests:** `parses_tools_and_each_tool_choice`, `server_tools_are_invalid`, `parses_tool_use_and_tool_result_blocks` (assistant text+tool_use; user with two tool_result then text → Tool, Tool, User in that order; arguments of `{"q":"x"}` become text `{"q":"x"}`), `parses_image_blocks_both_sources`, `tool_result_with_image_is_unsupported`, `renders_tool_use_blocks`, `stream_renderer_interleaves_text_and_two_tool_calls` (Review Focus 2: assert the exact event sequence and indices 0 text, 1 tool, 2 tool), `stream_renderer_tool_call_first_has_no_empty_text_block`.
- [ ] **Step 2:** run, see failures.
- [ ] **Step 3:** implement.
- [ ] **Step 4:** all gates green.
- [ ] **Step 5: Commit** `feat(translate): tools and images in the Anthropic format`.

### Task 3: OpenAI and Azure provider adapters

**Files:** Modify `crates/translate/src/provider/openai.rs` (Azure shares `body`/`parse`/`decode`; add an Azure test in `provider/azure.rs`).

Build: messages with parts → `content` as a string when the message is a single Text part (keeps today's bodies byte-identical for text), else an array of `{"type":"text","text"}` / `{"type":"image_url","image_url":{"url"}}` (base64 re-joined as `data:<type>;base64,<data>`); assistant `tool_calls` as OpenAI objects with `content` null when empty; `Role::Tool` → `{"role":"tool","tool_call_id","content"}`; `tools`, `tool_choice` (`"auto"|"none"|"required"|{"type":"function","function":{"name"}}`), `parallel_tool_calls` (only when Some, and only when tools are non-empty — OpenAI rejects it otherwise).

Parse: `message.tool_calls` → `ChatResponse.tool_calls` (non-function type → `Malformed`); `function_call` (legacy) stays `Unsupported`; remove `tool_calls_unsupported` for `tool_calls`.

Decode: `delta.tool_calls[]` items: an item with `id` emits `ToolCallStart { index, id, name }` (name from `function.name`, may be empty string when absent — some compatibles send it later: a later chunk with a name for an index that started without one is ignored, rule documented in a comment) and, when `function.arguments` is non-empty, a `ToolCallDelta`; an item without `id` emits only `ToolCallDelta` for non-empty arguments. An item whose `index` never started (no id ever seen) is `Malformed("tool call delta before its start")`. Some compatibles (Ollama, older Groq) send a whole call in one chunk with no `index`: treat a missing `index` as the next unused index.

- [ ] **Step 1: Failing tests:** `builds_tool_conversation_body` (check JSON exactly), `text_only_messages_keep_string_content` (regression), `builds_image_parts_with_data_url_rejoined`, `parallel_tool_calls_only_sent_with_tools`, `parses_response_tool_calls`, `decodes_streamed_parallel_tool_calls` (fixture `tests/fixtures/openai/stream_tool_calls.txt` with two calls interleaved by index, Review Focus 2), `decodes_whole_call_without_index`, `delta_for_unknown_index_is_malformed`, `anthropic_shaped_tool_conversation_round_trips_to_openai` (Review Focus 1: `ingress::anthropic::parse_request` of a tool_use/tool_result conversation → `build_request` for OpenAI → assert `tool_call_id` and arguments text), Azure `azure_body_carries_tools`.
- [ ] **Step 2:** run, see failures. **Step 3:** implement. **Step 4:** gates. 
- [ ] **Step 5: Commit** `feat(translate): tools and images to OpenAI and Azure`.

### Task 4: Anthropic provider adapter

**Files:** Modify `crates/translate/src/provider/anthropic.rs`.

Build: user parts → blocks `text` / `image` (`source` `{"type":"url","url"}` or `{"type":"base64","media_type","data"}`); assistant → text block (if non-empty) + `tool_use` blocks (`input` = arguments parsed; parse failure `InvalidRequest`); consecutive `Role::Tool` messages → ONE user message of `tool_result` blocks (`tool_use_id`, `content` text), merged with an immediately following `Role::User` message's blocks (Anthropic requires alternation); `tools` → `{"name","description"?,"input_schema"}`; `tool_choice` Auto→`{"type":"auto"}`, Required→`{"type":"any"}`, None→`{"type":"none"}`, Tool(n)→`{"type":"tool","name":n}`; `parallel_tool_calls == Some(false)` → `disable_parallel_tool_use: true` inside `tool_choice` (with `{"type":"auto"}` when no choice was given). Text-only bodies stay byte-identical to today (`content` as a string).

Parse: `tool_use` blocks → `tool_calls` (`arguments = serde_json::to_string(&input)`); `server_tool_use`/`web_search_tool_result` and other unknown blocks keep `unsupported_block`.

Decode: `content_block_start` with `tool_use` → `ToolCallStart` (tool index counts tool blocks only, from 0; keep a map from Anthropic block index to tool index in `StreamState` — add `pub tool_blocks: Vec<(u64, u32)>` or a small `HashMap`); `content_block_delta` `input_json_delta` → `ToolCallDelta` with `partial_json` (skip empty); the `input` of the start block (always `{}`) is not emitted.

- [ ] **Step 1: Failing tests:** `builds_tool_use_and_tool_result_blocks` (tool messages merged with the next user message), `builds_image_blocks_both_sources`, `invalid_tool_arguments_are_invalid_request`, `tool_choice_and_disable_parallel`, `text_only_body_is_unchanged` (regression: compare to a literal), `parses_tool_use_response`, `decodes_streamed_tool_use` (fixture `tests/fixtures/anthropic/stream_tool_use.txt`: text block, then two tool_use blocks with split partial_json), `openai_shaped_tool_conversation_round_trips_to_anthropic` (Review Focus 1).
- [ ] **Step 2–4:** fail, implement, gates.
- [ ] **Step 5: Commit** `feat(translate): tools and images to Anthropic`.

### Task 5: Gemini provider adapter

**Files:** Modify `crates/translate/src/provider/gemini.rs`.

Build: parts → `{"text"}` / `{"inlineData":{"mimeType","data"}}`; an `ImageSource::Url` → `Unsupported("Gemini takes images as data: URLs only")` (Review Focus 4); assistant tool calls → `{"functionCall":{"name","args"}}` parts (args parsed; failure `InvalidRequest`); consecutive `Role::Tool` messages → one `"role":"user"` content of `{"functionResponse":{"name","response":{"content": <text>}}}` parts, `name` found by `tool_call_id` among earlier assistant tool calls (not found → `InvalidRequest("tool result for unknown tool call '<id>'")`); `tools` → `[{"functionDeclarations":[{"name","description"?,"parameters"}]}]` — `parameters` passed through as given (Gemini accepts an OpenAPI subset; a schema it rejects is the provider's 400, passed on); `tool_choice` → `toolConfig.functionCallingConfig` `{"mode":"AUTO"|"NONE"|"ANY"}`, Tool(n) → `{"mode":"ANY","allowedFunctionNames":[n]}`; `parallel_tool_calls` has no Gemini equivalent: `Some(false)` is `Unsupported("parallel_tool_calls=false is not supported by this provider")`, `Some(true)`/None send nothing.

Parse / decode: `functionCall` parts → tool calls with id `call_<n>` (n = 0-based index within the answer, counted across stream chunks via `StreamState`), arguments `serde_json::to_string(&args)` (`{}` when absent); in a stream each call is a `ToolCallStart` followed by one `ToolCallDelta` with the whole arguments. A response with function calls has Gemini `finishReason: "STOP"`: report `FinishReason::ToolCalls` when the answer contains at least one call (non-stream and stream). `thoughtSignature` on a `functionCall` part is accepted and dropped (document: Gemini 2.5+ thinking models may need signatures echoed for multi-turn tool use — LATER, add to Known limits in Task 10).

- [ ] **Step 1: Failing tests:** `builds_function_declarations_and_tool_config` (each choice), `builds_function_call_and_response_history`, `tool_result_for_unknown_call_is_invalid`, `https_image_is_unsupported`, `data_image_becomes_inline_data`, `parallel_false_is_unsupported`, `parses_function_calls_with_stable_ids_and_tool_calls_finish`, `decodes_streamed_function_calls` (fixture `tests/fixtures/gemini/stream_function_call.txt`, two calls in two chunks → ids call_0, call_1).
- [ ] **Step 2–4:** fail, implement, gates.
- [ ] **Step 5: Commit** `feat(translate): tools and images to Gemini`.

### Task 6: Gateway end to end for tools and images

**Files:** Modify `crates/gateway/src/api/playground.rs` (OpenAPI schema: `PlaygroundMessage` gains `content: String | Vec<Object>`, `tool_calls`, `tool_call_id`; request gains `tools`, `tool_choice`, `parallel_tool_calls` — documented as "as `/v1/chat/completions`"); regenerate `openapi/admin.json` (`cargo run -p ultrafast-gateway -- openapi > openapi/admin.json`, CI checks it). Tests: `crates/gateway/tests/proxy.rs` (OpenAI ingress), `crates/gateway/tests/messages.rs` (Anthropic ingress), `crates/gateway/tests/cache.rs`, `crates/gateway/tests/playground.rs`, with the mock upstream in `crates/gateway/tests/common/`.

No proxy logic change is expected beyond Task 1; this task proves it end to end and fixes what the tests find.

- [ ] **Step 1: Failing integration tests** (mock upstream per provider kind, as the existing proxy tests do):
  - `openai_ingress_tool_call_to_anthropic_provider` and `anthropic_ingress_tool_call_to_openai_provider` (non-stream and stream; assert the caller sees the tool calls in its own format and the log row has `finish_reason`/status as for a normal call).
  - `gemini_https_image_is_400_without_upstream_call` (mock counts requests: 0 — Review Focus 4).
  - `image_body_over_limit_is_413`.
  - `cached_tool_call_answer_is_served_from_cache` (route with cache on, temperature 0, same tools → second call hits the cache, upstream count 1; changing a tool description misses).
  - `stream_estimate_counts_tool_arguments` (a stream that breaks after tool deltas is charged an estimate > input estimate alone).
  - `playground_accepts_tools_and_images` (session + CSRF).
- [ ] **Step 2–4:** fail, fix, gates (incl. `openapi/admin.json` current).
- [ ] **Step 5: Commit** `test(gateway): tools and images end to end` (or `feat` if code changed).

### Task 7: Response cache single-flight

**Files:** Create `crates/gateway/src/cache/flight.rs`; modify `crates/gateway/src/cache/mod.rs` (`pub mod flight` / re-export), `crates/gateway/src/app.rs` (state field), `crates/gateway/src/proxy.rs` (around the cache lookup, step 3d).

**Interfaces:**

```rust
/// Serializes concurrent misses of one cache key in this process.
pub struct Flights { /* Mutex<HashMap<CacheKey, Weak<tokio::sync::Mutex<()>>>> */ }

impl Flights {
    pub fn new() -> Self;
    /// Waits until no other caller holds `key`, then holds it until the
    /// guard is dropped. Entries of dropped guards are removed.
    pub async fn hold(&self, key: CacheKey) -> FlightGuard;
    #[cfg(test)] pub fn len(&self) -> usize;
}
pub struct FlightGuard { /* OwnedMutexGuard<()> + Arc<Mutex<()>> + key + &Flights cleanup on Drop */ }
```

Proxy flow on a cache plan: `get` → hit: answer (as today). Miss: `let _guard = state.flights.hold(plan.key).await;` → `get` again → hit: answer as a cache hit (metrics hit, record cache hit) → miss: call the provider as today; the guard drops when the call (and `keep`) is done, including on error and on caller disconnect (future dropped). The wait happens after limits and budgets were checked (unchanged order), so a waiter holds its concurrency slot while waiting — document in the function comment and in README Known limits. A metric `ultrafast_cache_flight_waits_total` counter (Prometheus, existing metrics module pattern).

- [ ] **Step 1: Failing unit tests in `flight.rs`:** `second_holder_waits_for_the_first`, `different_keys_do_not_wait`, `dropped_guard_releases_and_cleans_up` (`len() == 0`), `cancelled_waiter_does_not_block_others` (drop a pending `hold` future).
- [ ] **Step 2: Failing integration tests** in `crates/gateway/tests/cache.rs`: `concurrent_identical_calls_reach_the_provider_once` (50 tasks, mock upstream with a 200 ms delay, count == 1, all 50 answered, 49 logged as cached — Review Focus 5), `failed_leader_lets_waiters_call` (upstream fails the first call with 500 and a non-retryable route, then answers: every caller gets an answer or the error within the timeout, none hangs; use `tokio::time::timeout` 10 s), `uncacheable_calls_do_not_wait` (temperature 1.0: two concurrent calls → 2 upstream calls overlapping in time).
- [ ] **Step 3–4:** implement, gates.
- [ ] **Step 5: Commit** `feat(cache): one provider call per key for concurrent misses`.

### Task 8: Console — Playground tools and images

**Files:** Modify `ui/src/pages/Playground.tsx`, `ui/src/pages/PlaygroundThread.tsx`, `ui/src/pages/PlaygroundRun.ts`, the playground request types under `ui/src/api/`, `ui/src/pages/playground.test.tsx`, `ui/e2e/playground.spec.ts`.

- **Attach image** button on the composer (file input `accept="image/png,image/jpeg,image/gif,image/webp"`, max 5 MiB each, read as a data URL, thumbnail chips with a remove button and alt text = file name; over 5 MiB → inline error "Images over 5 MB are not sent."). A user message with images is sent as content parts.
- **Tools** collapsible section under parameters: a JSON textarea for the `tools` array (OpenAI shape), validated on Send (not an array of `{type:"function", function:{name}}` → inline error, nothing sent), and a Tool choice select (auto, none, required, or one of the tool names).
- **Tool calls in the answer**: an assistant turn with tool calls shows each call (name, pretty-printed arguments in a `<pre>`, copy button) and a "Tool result" textarea per call plus a "Send results" button that appends `tool` messages (`tool_call_id`) and runs the next turn. Streaming assembles calls from `tool_calls` deltas by index (in `PlaygroundRun.ts`, unit-tested).
- "Copy as curl" includes tools and image parts (images abbreviated as `data:image/png;base64,…` with a note "image data omitted").
- Accessibility as in CONVENTIONS (labels, one h1, 390 px).
- [ ] **Step 1: Failing tests:** `PlaygroundRun` stream assembly of two interleaved tool calls; image attach → request body has parts; invalid tools JSON blocks Send; tool result round trip sends `tool` messages; oversized image error.
- [ ] **Step 2–4:** implement; `pnpm --dir ui lint && pnpm --dir ui typecheck && pnpm --dir ui test`.
- [ ] **Step 5: E2E** in `ui/e2e/playground.spec.ts`: the mock provider `ui/e2e/mock-provider.ts`, extended to answer with a tool call when the request has `tools` and no `tool` message yet → send with a tool → call shown → send result → final text shown. Run `pnpm --dir ui build && cargo build --release -p ultrafast-gateway && pnpm --dir ui test:e2e` (free port, never 3900).
- [ ] **Step 6: Commit** `feat(console): tools and images in the playground`.

### Task 9: Clients — Rust, Python, TypeScript

**Files:** `crates/client/src/request.rs`, `crates/client/src/stream.rs` (re-export), `crates/client/README.md`; `crates/client-py/src/lib.rs`, the stubs under `crates/client-py/python/ultrafast/`, `crates/client-py/tests/`; `crates/client-wasm/src/api.rs`; `clients/ts/src/{types,client}.ts`, `clients/ts/test/`, `clients/ts/README.md`.

- **Rust**: `ChatRequest::user_parts(Vec<Part>)`, `.image(url_or_data_url)` (appends an Image part to the last user message, or starts one; returns `Result<Self, Error>` because `image_source` can fail), `.tool(Tool)`, `.tool_choice(ToolChoice)`, `.parallel_tool_calls(bool)`, `.assistant_tool_calls(text, Vec<ToolCall>)`, `.tool_result(id, content)`; re-export `Part, ImageSource, Tool, ToolCall, ToolChoice` from `ultrafast_client`. `ChatResponse.tool_calls` already exists via translate. Stream events pass through.
- **Python**: `chat`/`chat_stream` accept messages as either `(role, content)` tuples (unchanged) or OpenAI-shaped dicts (`role`, `content` str or parts list with `text`/`image_url`, `tool_calls`, `tool_call_id`); new keyword args `tools` (list of OpenAI-shaped dicts), `tool_choice` (`"auto"|"none"|"required"` or a tool name), `parallel_tool_calls`. Response gains `tool_calls: list[ToolCall]` (`id`, `name`, `arguments` str); stream events gain `kind` `"tool_call_start"` (`index`,`id`,`name`) and `"tool_call_delta"` (`index`,`arguments`). Conversion reuses `ultrafast_translate::ingress::openai` parsing of a message array where possible (build a JSON value and call a shared helper) — no second parser. Stubs and README updated.
- **TypeScript**: `Message` becomes `{ role: "system"|"user"|"assistant"|"tool"; content: string | ContentPart[] | null; toolCalls?: ToolCall[]; toolCallId?: string }`, `ContentPart = {type:"text",text}|{type:"image",url}`, `ChatRequest` gains `tools?: Tool[]` (`{name, description?, parameters?}`), `toolChoice?`, `parallelToolCalls?`; `ChatResponse.toolCalls: ToolCall[]`; `StreamEvent` gains `{type:"tool_call_start",index,id,name}` and `{type:"tool_call_delta",index,arguments}`. `checkMessages` validates the new shape. WASM `api.rs` maps both ways.
- [ ] **Step 1: Failing tests** in each: Rust builder produces the expected `types::ChatRequest`; Python round trip against the mock (existing pytest pattern) with a tool call answer and a stream with tool events; TS vitest with mocked fetch for both, plus `checkMessages` rejects a `tool` message without `toolCallId`.
- [ ] **Step 2–4:** implement; gates: `cargo test --all`, the clients workflow commands (`.github/workflows/clients.yml`: maturin develop + pytest, `pnpm --dir clients/ts test`, wasm build).
- [ ] **Step 5: Commit** `feat(clients): tools and images in the Rust, Python and TypeScript clients`.

### Task 10: Docs, version and release check

**Files:** `README.md`, `CHANGELOG.md`, `docs/release-notes/v2.0.0-beta.2.md`, `Cargo.toml` (+ `Cargo.lock`), `crates/client-py/pyproject.toml`/`Cargo.toml`, `clients/ts/package.json`, `ui/package.json` if versioned.

- README: Features list tools and images; "Using the gateway" gains a tool-calling example (curl, OpenAI SDK) and an image example; provider notes (Gemini: data: URLs only; Gemini tool ids `call_<n>`); Known limits: remove "Text chat only: no tools or images" and "There is no single-flight", add "Images, audio output and the Responses API are not supported yet (phase 2)", "Gemini thinking signatures are not echoed in multi-turn tool use", "A caller waiting on a single-flight call holds its concurrency slot", and replace "nobody sees another person's spend" with the accurate rule (members see their own usage, leads their teams', admins all). Clients section: tools/images examples.
- Version `2.0.0-beta.2`; CHANGELOG `[2.0.0-beta.2] - <date>`; release notes.
- [ ] **Step 1:** edit docs; bump versions; `cargo build` updates the lock.
- [ ] **Step 2:** all gates: `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all`, `pnpm --dir ui lint && pnpm --dir ui typecheck && pnpm --dir ui test`, E2E three runs green, clients workflow commands, `git archive HEAD | tar -x -C <tmp> && (cd <tmp> && pnpm --dir ui install --frozen-lockfile && pnpm --dir ui build && cargo build --release -p ultrafast-gateway)`.
- [ ] **Step 3: Commit** `chore: 2.0.0-beta.2`. No tag, no push (owner stop).

## Known limits after this plan

- No Responses API, image output, audio, or `response_format` / structured outputs (phase 2).
- Gemini thinking signatures are not echoed back in multi-turn tool use.
- The gateway does not fetch image URLs; Gemini needs `data:` URLs.
- Single-flight is per process; waiters hold their concurrency slot.
- Whole-cache clearing on configuration changes stays.
