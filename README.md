<p align="center"><img src="docs/brand/banner.png" alt="ultrafast — one small binary between your apps and every model" width="100%"></p>

# Ultrafast Gateway

[![CI](https://img.shields.io/github/actions/workflow/status/techgopal/ultrafast-ai-gateway/ci.yml?branch=main)](https://github.com/techgopal/ultrafast-ai-gateway/actions)
[![Rust](https://img.shields.io/badge/Rust-1.94+-orange.svg)](rust-toolchain.toml)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Ultrafast is one small Rust binary that sits between your apps and your LLM
providers. It speaks the OpenAI and Anthropic APIs, adds an admin API, and
serves a built-in web console, with SQLite for storage and nothing else to
run (or PostgreSQL, when several processes share one database). It is for small teams who host it themselves. This is v2; the v1 code is
tagged `v1-final`.

## Screenshots

The console is built into the binary. Overview, routing and the playground;
more (dark theme, logs, models) are in [`docs/images/`](docs/images/).

![The Overview page: counts, requests, errors, tokens and spend, top models and keys](docs/images/console-overview.png)

![The routing editor: weighted primary targets and an ordered fallback](docs/images/console-routing.png)

![The playground: a chat with a route, with tokens and cost](docs/images/console-playground.png)

## Features

- **Endpoints.** `/v1/chat/completions`, `/v1/messages` (Anthropic format),
  `/v1/responses` (OpenAI Responses API, stateless), `/v1/embeddings`,
  `/v1/images/generations`, `/v1/audio/transcriptions`,
  `/v1/audio/translations`, `/v1/audio/speech`, `/v1/models`; streaming on the
  chat formats and the Responses API, with tool calling and image input
  (vision), structured outputs (`response_format`) and reasoning effort, over
  any provider kind that has the feature. See Endpoints.
- **Prompt templates.** Named, versioned messages with `{{variables}}`; a call
  names a template and gives its values. Managed in the console and with
  `/api/prompts/*`. See Prompt templates.
- **Providers.** OpenAI, Anthropic, Gemini, Azure OpenAI, and any
  OpenAI-compatible API through the `openai` kind: Groq
  (`https://api.groq.com/openai/v1`), Mistral (`https://api.mistral.ai/v1`),
  OpenRouter (`https://openrouter.ai/api/v1`), Ollama
  (`http://localhost:11434/v1`) and others. Credentials are encrypted at rest
  with a master key.
- **Routing and resilience.** Routes with weighted targets, ordered fallbacks,
  retries, timeouts and circuit breakers; an exact-match response cache that
  sends concurrent identical calls to the provider once (single-flight).
- **Access control.** Email and password sign-in or single sign-on with OpenID
  Connect, roles (admin, team lead,
  member), teams, virtual keys with expiry and an allowlist of models and
  routes, a model catalog with enable and grant rules, an audit log.
- **Usage, logs, limits, budgets.** Request logs (metadata only; no prompt or
  answer is stored) with retention, tags, usage and spend reports, prices per
  model, rate limits (requests, tokens, concurrency) and budgets (block or
  alert) for the gateway, teams, users and keys.
- **Console.** Overview, logs, playground (chat, images, audio), providers,
  models, routing, prompts, keys, users, teams, limits, guardrails, alerts,
  settings; light and dark themes, phone layout. Compiled
  into the binary; no Node at runtime.
- **Metrics.** Prometheus at `/metrics` (opt in, token protected).
- **Tracing.** Every `/v1` call as an OpenTelemetry trace over OTLP/HTTP (JSON),
  with a span per provider attempt; opt in.
- **Alerts.** Budget, error-rate and circuit-breaker rules, delivered as signed
  webhooks (generic or Slack-compatible); managed by admins in the console.
- **Guardrails.** Block, redact or flag what goes to models and what comes
  back: keyword, regular-expression and PII rules that run in the gateway, or
  a signed external webhook that decides; on routes, keys or every call;
  streams included. See Guardrails.
- **Operations.** Online backup, configuration export and import, a CLI for
  setup, an OpenAPI description of the admin API, and admin SDKs for
  TypeScript and Python generated from it (`clients/admin-ts`,
  `clients/admin-py`; built from source, see Clients).
- **Clients.** Rust, Python and TypeScript, sharing one Rust core.

Not yet (phase 2): MCP tools and alerts by email. Not planned for now: a stateful Responses API, image edits and
variations, and realtime audio.

## Quickstart

### 1. Get it

**Docker** (linux/amd64; listens on 3000, keeps its data in `/var/lib/ultrafast`):

```bash
# The first admin, in a file of its own: not on the command line, where the
# password would be in the shell history and in `docker inspect`.
printf 'UF_ADMIN_EMAIL=you@example.com\nUF_ADMIN_PASSWORD=a long password\n' > admin.env
chmod 600 admin.env
docker run -d --name ultrafast -p 3000:3000 -v ultrafast-data:/var/lib/ultrafast \
  --env-file admin.env ghcr.io/techgopal/ultrafast-ai-gateway:2.0.0-beta.3
```

**Or a binary** from the [latest release](https://github.com/techgopal/ultrafast-ai-gateway/releases)
(Linux x86_64/aarch64, macOS Intel/Apple Silicon, Windows x64). For Linux x86_64:

```bash
V=2.0.0-beta.3; T=x86_64-unknown-linux-musl
curl -LO https://github.com/techgopal/ultrafast-ai-gateway/releases/download/v$V/ultrafast-v$V-$T.tar.gz
curl -LO https://github.com/techgopal/ultrafast-ai-gateway/releases/download/v$V/SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS   # macOS: shasum -a 256 --ignore-missing -c
tar xzf ultrafast-v$V-$T.tar.gz && cd ultrafast-v$V-$T
UF_DATA_DIR=./data UF_ADMIN_EMAIL=you@example.com UF_ADMIN_PASSWORD='a long password' \
  ./ultrafast serve
```

(Other targets: `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`,
`aarch64-apple-darwin`, and `x86_64-pc-windows-msvc.zip`. To build from source,
see [Development](#development).)

### 2. First admin

`UF_ADMIN_EMAIL` and `UF_ADMIN_PASSWORD` create the first admin when there is
no user yet (password 12 to 256 characters). They are read only then: once the
admin exists, start the gateway without them (run the container again without
`--env-file` and delete `admin.env`, or unset them) so the password does not
stay in the environment.

Without them, the console asks you to create the first admin on first visit,
and for a **setup code**: a gateway that starts with no user prints a one-time
code to its log (`Setup code: XXXX-XXXX-XXXX; open the console to create the
first admin.`, see `docker logs` for the container). Only who can read the log
can create the first admin, so a gateway reachable by others before setup
cannot be taken over. A restart prints a new code.

### 3. Provider, model, key

In the console at <http://127.0.0.1:3000> (plain HTTP works on localhost):

1. **Providers**, Add: a name, a kind (`openai`, `anthropic`, `gemini`,
   `azure`), the base URL (for example `https://api.openai.com/v1`) and the API key.
2. **Models**: sync the provider's models, enable the ones you want and grant
   them (to everyone, or to teams and users). Nothing is callable until it is
   enabled and granted.
3. **Virtual keys**, Create: the key (`uf-sk-...`) is shown once.

### 4. Call it

Use a model written `provider/model`:

```bash
curl http://127.0.0.1:3000/v1/chat/completions \
  -H "Authorization: Bearer $UF_KEY" -H "Content-Type: application/json" \
  -d '{"model":"openai/gpt-4o","messages":[{"role":"user","content":"Say hi."}]}'
```

```python
from openai import OpenAI
client = OpenAI(base_url="http://127.0.0.1:3000/v1", api_key=UF_KEY)
client.chat.completions.create(model="openai/gpt-4o", messages=[{"role": "user", "content": "Say hi."}])
```

```python
from anthropic import Anthropic
client = Anthropic(base_url="http://127.0.0.1:3000", api_key=UF_KEY)  # the SDK adds /v1/messages
client.messages.create(model="anthropic/claude-sonnet-5", max_tokens=256,
                       messages=[{"role": "user", "content": "Say hi."}])
```

The gateway accepts the key as `Authorization: Bearer` or `x-api-key`.

**Tools.** Send `tools` on `/v1/chat/completions` or `/v1/messages`, to any
provider kind, streaming or not; the gateway translates them. The arguments of
a call are a JSON string you parse yourself. Between an OpenAI-format caller and
an OpenAI-format provider (OpenAI, Azure, compatibles) the text passes through
unchanged; otherwise it is converted from or to the provider's JSON object
(Anthropic, Gemini), so it is serialized once and its keys come out sorted.

```bash
curl http://127.0.0.1:3000/v1/chat/completions \
  -H "Authorization: Bearer $UF_KEY" -H "Content-Type: application/json" \
  -d '{"model":"openai/gpt-4o","messages":[{"role":"user","content":"Weather in Paris?"}],
       "tools":[{"type":"function","function":{"name":"weather","description":"Current weather",
         "parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}}]}'
```

```python
from openai import OpenAI
client = OpenAI(base_url="http://127.0.0.1:3000/v1", api_key=UF_KEY)
tools = [{"type": "function", "function": {"name": "weather", "description": "Current weather",
          "parameters": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}}}]
messages = [{"role": "user", "content": "Weather in Paris?"}]
first = client.chat.completions.create(model="openai/gpt-4o", messages=messages, tools=tools)
call = first.choices[0].message.tool_calls[0]          # call.function.arguments is JSON text
messages += [first.choices[0].message,
             {"role": "tool", "tool_call_id": call.id, "content": "18 C, clear"}]
client.chat.completions.create(model="openai/gpt-4o", messages=messages, tools=tools)
```

`tool_choice` is `auto`, `none`, `required` or a named tool. With `tools` empty
or absent, `auto`, `none` and `parallel_tool_calls` are ignored (SDKs send them
anyway); `required` is a 400, and so is a named tool, with no tools or when it
is not among `tools`. A `tool` message may carry `name` (OpenAI's older form);
Anthropic ignores it and Gemini uses it when the call id matches no earlier
call. `function.strict` is sent to OpenAI and Azure; Anthropic and Gemini have
no such setting and ignore it. Gemini has no call ids, so the gateway names
its tool calls `call_<8 hex>_<n>`, where the hex comes from the response id and
differs from answer to answer (an answer with a response id always gets the same ids; one without gets random hex), and
sends tool schemas as `parametersJsonSchema` (full JSON Schema).

**Images.** Send an `image_url` part in a user message (PNG, JPEG, GIF or WebP;
an `http(s)` URL or a `data:` URL). Only user messages may carry images.

```python
client.chat.completions.create(model="openai/gpt-4o", messages=[{"role": "user", "content": [
    {"type": "text", "text": "What is in this picture?"},
    {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0..."}},
]}])
```

The gateway never fetches an image URL: the provider does, so it must be able
to reach it. Gemini takes `data:` URLs only (an `https` URL is a 400). Images
count toward the 10 MiB request body limit (413 above it). The Anthropic
format takes `image` blocks with a `source` of type `base64` or `url`.

The same setup from the command line (no console), against the same data
directory:

```bash
export UF_DATA_DIR=./data
UF_PROVIDER_API_KEY=sk-... ./ultrafast provider add \
  --name openai --kind openai --base-url https://api.openai.com/v1
./ultrafast model add --provider openai --model gpt-4o --enable --everyone
./ultrafast key create --name my-app     # printed once
# In Docker: docker exec -e UF_PROVIDER_API_KEY=sk-... ultrafast ultrafast provider add ...
```

A running gateway picks up CLI changes within 30 seconds; changes through the
console or `/api` apply at once.

## Configuration

There is no config file. Everything is a flag or an environment variable
(`ultrafast --help`, `ultrafast serve --help`). Prefer the variable for
secrets: a flag shows in the process list.

| Variable | Flag | Default | Meaning |
| --- | --- | --- | --- |
| `UF_DATA_DIR` | `--data-dir` | `./data` (`/var/lib/ultrafast` in the image) | Directory of the SQLite database (`gateway.db`) and `master.key`. Not used with `UF_DATABASE_URL`. Any subcommand. |
| `UF_MASTER_KEY` | `--master-key` | generated into `master.key` | 64 hex characters; encrypts provider credentials. Any subcommand. Required with `UF_DATABASE_URL`, and the same for every gateway on that database. |
| `UF_DATABASE_URL` | `--database-url` | unset (SQLite in the data directory) | A PostgreSQL URL (`postgres://user:password@host:5432/db`). Set: the gateway uses that database and `UF_MASTER_KEY` is required. Add `?sslmode=require` (or `verify-full` with `sslrootcert=/path/ca.pem`) for TLS. Prefer the variable: the URL holds the password. Any subcommand. See Using PostgreSQL. |
| `UF_DATABASE_MAX_CONNECTIONS` | `--database-max-connections` | `10` | The most connections one gateway process opens to PostgreSQL, 1 to 1000. Ignored on SQLite. |
| `UF_HOST` | `--host` | `127.0.0.1` (`0.0.0.0` in the image) | Address to listen on. `serve`. |
| `UF_PORT` | `--port` | `3000` | Port to listen on. `serve`. |
| `UF_ADMIN_EMAIL` | none | unset | With `UF_ADMIN_PASSWORD`: create the first admin at start when no user exists. Setting only one is an error. `serve`. |
| `UF_ADMIN_PASSWORD` | none | unset | See above. 12 to 256 characters. |
| `UF_INSECURE_COOKIES` | `--insecure-cookies` | off | Send the session cookie without `Secure`, for plain HTTP on a trusted network or while developing. |
| `UF_TRUSTED_PROXIES` | `--trusted-proxy CIDR` (repeatable) | none | Networks of reverse proxies whose `CF-Connecting-IP` and `X-Forwarded-For` are believed. Comma separated in the variable. |
| `UF_PUBLIC_URL` | `--public-url` | unset | The address people reach the gateway at, like `https://gateway.example.com`: origin only, no path. Needed for single sign-on, whose redirect URI is `<url>/api/auth/oidc/callback`. Plain `http` only for localhost unless `--insecure-cookies`. `serve`. See Single sign-on. |
| `UF_METRICS_TOKEN` | `--metrics-token` | unset | Enables `GET /metrics` for callers sending this bearer token. |
| `UF_OTEL_ENDPOINT` | `--otel-endpoint` | unset | Enables trace export. The OTLP base URL of a collector or backend (`http://localhost:4318`): spans are posted to `<base>/v1/traces`. A URL that already ends in `/v1/traces` is used as it is; a query string is kept. `serve`. See Tracing. |
| `UF_OTEL_HEADERS` | `--otel-headers` | unset | Headers sent with every export, `name=value` pairs separated by commas (for example `authorization=Bearer ...`). Prefer the variable: it can hold a credential. |
| `UF_OTEL_SERVICE_NAME` | `--otel-service-name` | `ultrafast` | The `service.name` resource attribute. |
| `UF_OTEL_SAMPLE_RATIO` | `--otel-sample-ratio` | `1.0` | Share of calls traced, 0.0 to 1.0, when the caller sent no `traceparent`. |
| `UF_MAX_AUDIO_BYTES` | `--max-audio-bytes` | `26214400` (25 MiB) | `serve`: the largest audio file `/v1/audio/transcriptions` and `/v1/audio/translations` take, 1 to 1 GiB. A larger upload is refused with 413 while it is read. |
| `UF_MAX_CONCURRENT_UPLOADS` | `--max-concurrent-uploads` | `8` | `serve`: how many audio uploads are received at once by this process, 1 to 1024. Each holds up to `UF_MAX_AUDIO_BYTES` in memory while its body arrives; the place is given back once the body has been received, not kept while the provider answers. One past the bound is refused with 503 before its body is read. |
| `UF_PROVIDER_API_KEY` | `--api-key` | unset | `provider add` only: the provider's API key. |
| `RUST_LOG` | none | `info` | Log filter. A gateway that starts with no user logs its one-time setup code at `info` under the target `ultrafast::setup`: when you lower the level, keep it, as in `RUST_LOG=warn,ultrafast::setup=info`. |

For the dev tooling only: `UF_DEV_GATEWAY` (where `pnpm --dir ui dev` proxies
to), `UF_E2E_BINARY` (the binary the browser tests run), `UF_E2E_DATABASE_URL`
(run the browser tests on PostgreSQL, one schema per gateway; needs `psql`;
point it at a throwaway server: a run that is killed leaves its `uf_e2e_*`
schemas behind) and
`UF_TEST_DATABASE_URL` (run the Rust tests on PostgreSQL; build with
`--features test-support`).

Subcommands: `serve`, `provider add`, `model add`, `key create`,
`backup <path>`, `config export|import <file>`, `openapi`.

## Using the gateway

- **Models.** Call `provider/model` (for example `anthropic/claude-sonnet-5`)
  or the name of a route. `GET /v1/models` lists what the key may call.
- **Access.** A model is callable when it is enabled, granted to the caller
  (everyone, or a team or user they belong to), and, if the key has an
  allowlist, on that list. Routes follow the same idea. A call that fails these
  rules is refused.
- **Tags.** Send `x-uf-tags: {"job":"nightly","env":"dev"}` on any `/v1` call (at most 20 tags; names `A-Z a-z 0-9 _ . -`; names and
  values 1 to 64 characters; header at most 1 KiB; never forwarded to the
  provider). Keys can carry tags too, set by an admin; on the same name the
  key's tag wins. Logs filter with `GET /api/logs?tag=env:prod`, usage groups
  with `GET /api/usage?group=tag:team`.
- **Errors.** Rate limits and `block` budgets answer 429 with `Retry-After`
  (budgets: `budget_exceeded`). Errors use the shape of the endpoint called
  (OpenAI or Anthropic).
- **Cache.** A route can cache answers (TTL 1 to 86 400 s; shared per team, key
  or user). Streams and temperature above 0.5 are never cached. When identical
  cacheable calls arrive at once, one reaches the provider and the others wait
  for its answer (single-flight); if it fails, each waiter calls the provider
  itself.
- **Prices and budgets.** A model without a price is logged without a cost, so
  spend is a lower bound. Budgets are per UTC day, week (from Monday) or month.
- **Admin API.** `/api/*`, described by [`openapi/admin.json`](openapi/admin.json)
  (also `ultrafast openapi`). Sign in with a session, or send an access token
  (`uf-at-...`, from Account) as a bearer token.

## Endpoints

Every `/v1` endpoint takes a virtual key as a bearer token, a model written
`provider/model` or a route name, and answers with the errors of its own
format (OpenAI's, or Anthropic's on `/v1/messages`). Calls are logged under the
endpoint they came in on: `chat`, `messages`, `responses`, `embeddings`,
`images`, `transcriptions`, `translations` or `speech` (`playground` for the
console's own calls), and the same names are the `endpoint` label of
`uf_requests_total`, the span names (`uf.<endpoint>`) and the `endpoint` an
external guardrail is told.

### What each provider serves

| | OpenAI kind (any base URL) | Azure OpenAI | Anthropic | Gemini |
| --- | --- | --- | --- | --- |
| `/v1/chat/completions`, `/v1/messages`, `/v1/responses` | yes | yes | yes | yes |
| `/v1/embeddings` | yes | yes | no, 400 | yes |
| `/v1/images/generations` | yes | yes | no, 400 | no, 400 |
| `/v1/audio/*` | yes | yes | no, 400 | no, 400 |
| `response_format` | as is | as is | `output_config.format` | `responseMimeType` and `responseJsonSchema` |
| `reasoning_effort` | yes | yes | no, 400 | no, 400 |
| `max_completion_tokens` | sent as such to `api.openai.com` | sent as such | `max_tokens` | `maxOutputTokens` |
| `function.strict` | yes | yes | ignored | ignored |

For Azure, set the provider's API version (`provider add --api-version`, or
the console) to a current one: the default, `2024-10-21`, predates the image
and audio models (`gpt-image-1`, `gpt-4o-transcribe`, `gpt-4o-mini-tts`) and
`reasoning_effort`, so use a newer version, such as a 2025 preview, for them.

Groq, Mistral, OpenRouter, Ollama and other OpenAI-compatible APIs are the
`openai` kind: the gateway sends them the OpenAI form, and whether a given host
has the images, audio or reasoning API is up to it (its error comes back to
the caller). A model that cannot serve images or audio answers 400 "This model
does not support image generation." (or "audio."), without a provider being
called; on a route, targets that cannot serve a call are skipped and the first
that can serves it. For features of chat calls the rule is different: a feature
the first target lacks (`reasoning_effort` on Anthropic or Gemini, `tool_choice`
with no tools, ...) is a 400, and the call does not move on to a fallback that
has it.

### Structured outputs

`response_format` on `/v1/chat/completions` is `{"type":"text"}`,
`{"type":"json_object"}` or a JSON schema, as in the OpenAI API:

```bash
curl http://127.0.0.1:3000/v1/chat/completions \
  -H "Authorization: Bearer $UF_KEY" -H "Content-Type: application/json" \
  -d '{"model":"openai/gpt-4o","messages":[{"role":"user","content":"A city and its country."}],
       "response_format":{"type":"json_schema","json_schema":{"name":"place","strict":true,
         "schema":{"type":"object","properties":{"city":{"type":"string"},"country":{"type":"string"}},
                   "required":["city","country"],"additionalProperties":false}}}}'
```

How each provider gets it:

- **OpenAI and Azure:** `response_format` as it came (name, description,
  strict, schema).
- **Anthropic:** `output_config.format` of `{"type":"json_schema","schema":...}`
  (generally available, no beta header; the gateway does not emulate it with a
  forced tool). `json_object` becomes the schema `{"type":"object"}`, so the
  model must answer an object but may answer `{}`; `text` sends nothing. The
  `name`, `description` and `strict` of the schema are not sent: Anthropic has
  no such settings and enforces the schema anyway. Anthropic limits what a
  schema may hold (no recursion, no numeric or string constraints,
  `additionalProperties` only `false`) and answers 400 for one that breaks
  them; that error comes back as it is. `/v1/messages` takes
  `output_config.format` too (any other `output_config` key is a 400); a
  schema given that way is sent to OpenAI and Azure with `strict: true`, since
  Anthropic always enforces it. OpenAI and Azure accept a strict schema only
  when every object in it has `additionalProperties: false` and lists all of
  its properties in `required`; a schema sent on `/v1/messages` that does not is
  answered with their 400, which comes back as it is. Write the schema that way
  if it may be served by an OpenAI or Azure model.
- **Gemini:** `generationConfig.responseMimeType` of `application/json`, and
  for a schema `generationConfig.responseJsonSchema` (full JSON Schema).
  `json_object` sends the mime type only; `name`, `description` and `strict` are
  not sent.

On `/v1/responses` the same is `text.format` (flat: `{"type":"json_schema",
"name":...,"schema":...}`). The clients take `response_format` (TypeScript:
`responseFormat: {type: "json_schema", jsonSchema: {name, schema, strict?,
description?}}`, with `jsonSchema` in camelCase). The format is part of the
response cache key.

### Responses API

`POST /v1/responses` is the OpenAI Responses API over any provider, converted
to and from the chat form. It is **stateless**: nothing is stored, so there is
no `GET /v1/responses/{id}`, and the response id (`resp_...`) names nothing you
can fetch.

```bash
curl http://127.0.0.1:3000/v1/responses \
  -H "Authorization: Bearer $UF_KEY" -H "Content-Type: application/json" \
  -d '{"model":"anthropic/claude-sonnet-5","instructions":"Be brief.","input":"Say hi."}'
```

Supported: `input` as a string or as items (`message` with the roles `user`,
`system`, `developer` and `assistant`, whose content is `input_text`, `input_image`
(an `http(s)` or `data:` URL in `image_url`) or, for the assistant,
`output_text`; `function_call`; `function_call_output` with a **string**
`output`), `instructions`, function `tools`, `tool_choice` (`auto`, `none`,
`required`, a function), `parallel_tool_calls`, `max_output_tokens`,
`temperature`, `top_p`, `text.format`, `stream`, and `prompt` (a prompt
template, see below). Consecutive `function_call` items form one assistant
turn. Accepted and ignored: `metadata`, `user`, `service_tier`,
`stream_options`, `safety_identifier`, `prompt_cache_key`,
`prompt_cache_retention`.

Reasoning: `reasoning.effort` is sent as `reasoning_effort` to OpenAI and Azure
and is a 400 on other providers; `reasoning.summary` is accepted and ignored
(no summaries are produced); `reasoning` **items** in `input` and
`include: ["reasoning.encrypted_content"]` are accepted and ignored, because
they are opaque state of the provider (this lets clients that replay them work);
any other `include` value is a 400. `truncation: "disabled"` is accepted and
`"auto"` is a 400: the gateway never drops input.

Refused with 400: `store: true`, `previous_response_id`, `conversation`,
`background: true`, tools that are not functions (web search, file search,
computer use, ...), `input_file` and `file_id`, an **array** `output` in
`function_call_output` (send a string), refusal parts, `max_tool_calls`,
`text.verbosity` and any other field ("field 'x' is not supported yet").

A stream is the Responses event stream (`event:` and `data:` lines with
`sequence_number`, from `response.created` to `response.completed`, or
`response.incomplete` for a length or content-filter ending). **There is no
`data: [DONE]`** on this API. Guardrails, rate limits, budgets, the cache and
tags apply as for chat; the official OpenAI SDKs parse the stream.

### Images

`POST /v1/images/generations` (OpenAI, Azure and compatible providers):

```bash
curl http://127.0.0.1:3000/v1/images/generations \
  -H "Authorization: Bearer $UF_KEY" -H "Content-Type: application/json" \
  -d '{"model":"openai/gpt-image-1","prompt":"A lighthouse at dusk","size":"1024x1024","n":1}' \
  | jq -r '.data[0].b64_json' | base64 -d > lighthouse.png
```

Fields: `model`, `prompt` (up to 32 000 characters), `n` (1 to 10), `size`,
`quality`, `background` (`transparent`, `opaque`, `auto`), `output_format`
(`png`, `jpeg`, `webp`), `output_compression` (0 to 100), `moderation`
(`low`, `auto`), `response_format` (`url`, `b64_json`), `style` (`vivid`,
`natural`) and `user`. `stream` and `partial_images` are a 400. The GPT image
models (names that start with `gpt-image`) take neither `response_format` nor
`style`: the gateway answers 400 for them and passes both for `dall-e-*`. The
answer has `created`, `data` (each item `b64_json` or `url`, with
`revised_prompt`), the answer's `background`, `output_format`, `quality` and
`size`, and `usage` when the provider reports it (a call without usage is logged
unpriced). Images are never cached, and the gateway never fetches a returned
`url`.

An image costs money when the request is sent, so a call that was sent is
**never repeated**: if the provider does not answer within the time (a first-byte
timeout of at least 180 s and a total of at least 300 s, or your route's
settings if longer), the request runs out of time after it was sent, or the
connection breaks after it was sent, the caller gets 504 "The provider did not
answer in time. The request may still be processed and billed; it was not
repeated." and no fallback is tried. An answer that arrives but cannot be read
is a 502, also not repeated. A connection that failed before the request was
sent is retried as usual. A
provider's image answer may be up to **128 MiB** (chat answers are capped at
32 MiB); it is held in memory whole and converted, which takes a few hundred
MiB for one such answer, so use a concurrency limit on routes that generate
many large images.

### Audio

Three endpoints, for OpenAI, Azure and compatible providers:

```bash
# Speech to text, in the language spoken
curl http://127.0.0.1:3000/v1/audio/transcriptions \
  -H "Authorization: Bearer $UF_KEY" \
  -F model=openai/whisper-1 -F response_format=verbose_json -F file=@meeting.mp3

# Speech to English text
curl http://127.0.0.1:3000/v1/audio/translations \
  -H "Authorization: Bearer $UF_KEY" -F model=openai/whisper-1 -F file=@entrevista.mp3

# Text to speech
curl http://127.0.0.1:3000/v1/audio/speech \
  -H "Authorization: Bearer $UF_KEY" -H "Content-Type: application/json" \
  -d '{"model":"openai/tts-1","input":"Hello there.","voice":"alloy"}' --output hello.mp3
```

Fields: *transcriptions* `file`, `model`, `language`, `prompt`,
`response_format` (`json`, `text`, `verbose_json`, `srt`, `vtt`), `temperature`
(0 to 1) and `timestamp_granularities` (with `verbose_json`); *translations* the
same without `language` and `timestamp_granularities`; *speech* `model`,
`input` (up to 4096 characters), `voice` (a name, or `{"id": ...}`),
`response_format`, `speed` (0.25 to 4) and `instructions`. Refused with 400:
`stream`, `include`, `chunking_strategy`, `known_speaker_*`, `languages`,
`keywords` and the `diarized_json` format, and `stream_format: "sse"` on speech.
Models differ and the gateway checks what it knows: the `gpt-4o` transcription
models take `json` and `text` and no timestamps, and `tts-1*` takes no
`instructions`.

Uploads: the form is read while it arrives. **Send `model` before `file`**:
the gateway then decides access and takes the rate-limit permit before it reads
the file, so an unknown or forbidden model or a rate-limited key is refused
after under 1 MiB; with `file` first the whole file (up to the cap) is read
before the refusal. The file is at most `UF_MAX_AUDIO_BYTES` (25 MiB by default;
a larger one is a 413 naming the field), other text fields at most 64 KiB, and
the whole upload must arrive within 60 s with no pause longer than 15 s (a 408
otherwise). At most `UF_MAX_CONCURRENT_UPLOADS` (**8** by default) uploads are
being received at once by a process; one more is a 503 "Too many audio uploads
are in progress. Try again shortly." with `Retry-After: 1`, and its body is not
read. A place is held from the first byte of the upload to the end of its
body, not while the provider answers, so many slow transcriptions can run at
once. A file is held in memory while it arrives and while its call runs, so
plan for the cap times the uploads that can be in flight, not only those
being received. Audio calls are slow, billed once
and not cached: the timeouts and the no-repeat rule are those of images. A
speech answer is streamed back with the provider's content type, up to 64 MiB.

Guardrails check the speech `input`, the `prompt` field of a transcription or
translation, and the transcript (see Guardrails, *What is checked*); the audio
itself is not inspected.

### Reasoning effort and token limits

`reasoning_effort` (`none`, `minimal`, `low`, `medium`, `high`, `xhigh`,
`max`) on `/v1/chat/completions`, and `reasoning.effort` on `/v1/responses`, is
sent to OpenAI and Azure; any other provider answers 400.

`max_completion_tokens` is accepted on `/v1/chat/completions` as the same limit
as `max_tokens` (it wins when both are given). The gateway sends OpenAI
(base URL host `api.openai.com`) and Azure targets `max_completion_tokens`, which
the o-series and GPT-5 models require (they reject `max_tokens`); other
OpenAI-compatible hosts keep getting `max_tokens`, which is what they know.

### Prompt templates

A template is a name and numbered **versions**; a version is 1 to 64 messages
(roles `system`, `developer`, `user`, `assistant`) with `{{variables}}`, an
optional model and optional `temperature`, `max_tokens`, `top_p` and
`response_format`. A call names the template and gives a value for each
variable; the gateway puts the template in front of the call before anything
else looks at it, so rate limits, guardrails (the rendered text), the cache and
budgets see the real request.

```bash
curl http://127.0.0.1:3000/v1/chat/completions \
  -H "Authorization: Bearer $UF_KEY" -H "Content-Type: application/json" \
  -d '{"prompt":{"id":"summarize","version":3,"variables":{"audience":"kids","text":"..."}},
       "messages":[{"role":"user","content":"Go."}]}'
```

The same `prompt` object works on `/v1/responses` (a version may be a number or
a string of digits; `variables` map names to strings). It is not accepted on
`/v1/messages`. With `prompt`, `model` and `messages` (`input`) may be left
out.

- **Syntax.** A variable is `{{name}}` with a name of letters, digits and `_`
  that does not start with a digit, at most 64 characters, case sensitive.
  Anything else with braces (`{{ name }}`, `{{1x}}`, `{{}}`, `{name}`) is plain
  text. A call must give a value for **every** variable of the version and for
  no other (400 otherwise). A value is a string of at most 32 KiB, put in as it
  is: nothing is escaped, and a value that contains `{{other}}` stays that text.
- **Versions never change.** Adding a version makes the next number; a version
  cannot be edited, and its row cannot be updated (a trigger refuses it). A call
  without `version` gets the latest.
- **Precedence.** The template's messages come first, then the call's own. The
  call's `model` is used, else the template's, else the call is a 400. The
  call's `temperature`, `max_tokens`, `top_p` and `response_format` win over the
  template's.
- **Latest in memory.** The gateway keeps the latest version of every
  template in its snapshot, so a call by name never waits for the database. An
  explicit older version is read from the database the first time and then
  kept (the last 256 used); if that read fails or takes longer than 2 s the
  call is a 503. A change reaches other processes on PostgreSQL within about
  30 s, as other settings do. A refresh reads the text of a template only when
  its latest version changed, and the Prompts list does not read texts at all.
- **Limits.** 1000 templates (a team lead who is not an admin may have made
  at most 100), 200 versions each, 64 messages per version, a
  message of at most 64 KiB and a version of at most 256 KiB, 64 variables, the
  rendered messages at most 1 MiB together. Names are 1 to 100 characters and
  cannot contain `@`. An unknown template or version is a 404
  `not_found_error`; any other fault of the `prompt` is a 400.
- **Logs.** The call is logged with `name@version` (the Logs page has a Prompt
  column); the text of a template is never in the logs, and no metric has a
  prompt label.
- **Who.** Anyone signed in reads templates and anyone who can call may use any
  template by name: a template is a convenience, not a secret. Admins make and
  change all of them, a team lead the ones they made.
- **Admin API.** `GET`/`POST /api/prompts`, `GET`/`DELETE /api/prompts/{id}`,
  `POST /api/prompts/{id}/versions`, `GET /api/prompts/{id}/versions/{version}`
  and `POST /api/prompts/{id}/render` (what a version renders to). The two
  `POST`s that write a version take a body of up to 1 MiB (the rest of `/api`
  64 KiB). Deleting a template deletes its versions; the logs keep the
  `name@version` they recorded.
- **Export and import.** `config export` writes the templates with all their
  versions (as `prompts`); an import adds the templates and versions a gateway
  lacks and never rewrites one it has: a version that differs from the stored
  one is an error. An imported template belongs to the importing admin. An
  export is refused (409, or an error on the command line) while a template has a
  version that cannot be read, naming it, rather than leaving it out of the file.

## Console

Served at `/`. What each role sees is decided by the API.

| Page | Admin | Team lead | Member |
| --- | --- | --- | --- |
| Overview, Logs | all | their teams' and own | own |
| Playground | yes | yes | yes |
| Providers | manage | view | view |
| Models, Routing | manage | see what they may use | see what they may use |
| Prompts | make, add versions, delete any | read; make, and manage the ones they made | read |
| Virtual keys | any user's | own and their teams' members' | own |
| Users | invite, role, status, delete | their teams' members | themselves |
| Teams | create, delete, leads | rename, add members | see own teams |
| Budgets and limits | set | read their teams' | read what applies to them |
| Guardrails | manage, attach to routes and keys, try | no | no |
| Settings (retention, sign-in, backup, config, audit log) | yes | no | no |
| Account (name, password, access tokens) | yes | yes | yes |

The playground has three modes. **Chat** sends images (5 MB each, 9 MiB per
request including the history), tools as JSON with a tool choice, a response
format (text, JSON or a JSON schema), and a prompt template: pick one, a
version, and a value for each variable (a template that names a model can be
called with "Template's model", and with no message of your own). The Prompts
page has an *Open in Playground* link for each template and version
(`/playground?prompt=<name>&version=<n>`). **Images** generates images and
**Audio** transcribes, translates and speaks. The audio file check in the
playground checks a file against the gateway's own `UF_MAX_AUDIO_BYTES` (it asks
`GET /api/playground/config`). The Logs page filters by endpoint and shows the
endpoint and the prompt (`name@version`) of each call.

MCP tools appears in the navigation as coming. Leads and members do not see the
Guardrails page, but the guardrails of a key show on the key, and a call a
guardrail stopped says so in the logs and in the playground.

## Single sign-on

Besides email and password, people can sign in with one OpenID Connect (OIDC)
identity provider: Google, Microsoft Entra ID, Okta, Keycloak or any other
provider that publishes `/.well-known/openid-configuration`. Passwords keep
working next to it. The gateway uses the authorization code flow with PKCE
(S256) and checks the ID token (signature, issuer, audience, nonce, expiry)
before it trusts anything in it. Every sign-in ends in the same session a
password sign-in makes.

**Set up.**

1. Start the gateway with its public address: `UF_PUBLIC_URL=https://gateway.example.com`
   (or `--public-url`). It is the origin people type in the browser: `http` or
   `https`, a host and optionally a port, **no path**, query or credentials
   (the console is served at the root). Plain `http` is accepted only for
   `localhost`, `127.0.0.1` and `[::1]`, unless you start with
   `--insecure-cookies`. A wrong value stops `serve` at start.
2. In your provider, register a web application (a confidential client with a
   client secret) with the redirect URI
   `<UF_PUBLIC_URL>/api/auth/oidc/callback`, for example
   `https://gateway.example.com/api/auth/oidc/callback`. Settings in the
   console shows it, with a copy button, once the public URL is set.
3. As an admin open Settings, Single sign-on (OIDC): enter the issuer, client
   ID and client secret, choose how users are mapped (below), use Test
   configuration, then switch it on. The sign-in page then offers
   "Sign in with <label>". The client secret is stored encrypted with the
   master key and is never shown again; leave the field empty to keep it.
   If the master key changes, the stored secret can no longer be read: single
   sign-on stays off (Settings shows an alert) until the client secret is
   entered again and saved, and a sign-in already under way fails with `state`.

Issuer and where to register, per provider:

- **Google.** Issuer `https://accounts.google.com`. Google Cloud console, APIs
  and Services, Credentials, OAuth client ID, type "Web application"; add the
  redirect URI. Google sends `email_verified`; it has no groups claim. With
  this issuer any Google account can attempt to sign in: only users who
  already exist are linked, and new accounts are made only for Allowed domains,
  so keep link by email and auto-create restricted to your own domains.
- **Microsoft Entra ID.** Issuer `https://login.microsoftonline.com/<tenant-id>/v2.0`
  (use the tenant ID, not `common`). App registrations, New registration, platform
  "Web", add the redirect URI; Certificates and secrets, new client secret
  (use its Value). To use groups for the admin role, add a groups claim in
  Token configuration and use the group's object ID as the admin group.
- **Okta.** Issuer `https://<your-domain>.okta.com` (the org authorization server)
  or `https://<your-domain>.okta.com/oauth2/default`. Applications, Create App
  Integration, OIDC, "Web Application"; add the redirect URI as a sign-in
  redirect URI. For groups, add a groups claim to the ID token (filter on the
  groups you need).
- **Keycloak.** Issuer `https://<host>/realms/<realm>`. Clients, Create client,
  OpenID Connect, "Client authentication" on, add the redirect URI as a valid
  redirect URI. For groups, add a "Group Membership" mapper to the client with
  "Add to ID token" on (set "Full group path" off to match plain names).

The authorization, token and key-set endpoints the provider announces must use
the same scheme as the issuer: an `https` issuer cannot send the gateway to an
`http` or loopback endpoint. `http` is accepted only when the issuer itself is
a loopback `http` address (development). The gateway asks for the scopes
`openid email profile` plus any extra scopes you list.

**Who gets in.** The gateway never creates a session for an identity it cannot
map to a user. For each sign-in, in this order:

1. The identity is already linked: the user whose link is
   `<issuer>|<subject>` (the provider's stable `sub`). Changing the email at
   the provider changes nothing here. A disabled user is refused.
2. Not linked yet, "Link existing users by email" on, and the provider says
   the email is **verified** (`email_verified`): the user with that email is
   linked to this identity. An invited user becomes active (the pending
   invitation is dropped), a password user keeps the password. A disabled user
   is refused. If that user is already linked to the same issuer under another
   `sub`, the sign-in is refused (a reassigned address is another person).
   Microsoft Entra ID usually omits `email_verified`: for an issuer under
   `https://login.microsoftonline.com/` the email counts as verified only when
   it is the ID token's own `email` claim and the token's `tid` equals the
   tenant in the issuer. An email taken from the userinfo endpoint is never
   treated as verified that way.
3. Otherwise, "Create users on first sign-in" on, a verified email, and an
   email domain in "Allowed domains" (exact domain, not subdomains; list
   internationalized domains as punycode, `xn--...`): a new active Member is
   created, with no password.
4. Otherwise the sign-in is refused (`not_allowed`).

An unverified email never links or creates anyone.

**Roles.** With an "admin group" set, the user's role follows the provider on
every sign-in: in the group (the groups claim, `groups` unless you name
another) means admin, not in it means member. Without an admin group, roles are
never changed by sign-in, and you change them in the console as before.

- If the ID token has **no** groups claim, the gateway does not know the groups
  and leaves the role as it is (a note goes to the log). Entra ID leaves the
  claim out when a user is in too many groups ("groups overage", it sends a
  link instead): such a user keeps their role. Reduce the groups sent (assign
  only the needed groups to the application) or use app roles.
- An explicitly **empty** list means "in no group": an admin who is in none is
  made a member.
- The last active admin is never demoted by the provider. The sign-in works,
  the role stays, and the audit log records
  `user.role_from_idp` explaining why.

Team membership is not synchronized.

**Sign-in errors.** A failed sign-in returns to the sign-in page with
`?sso_error=<code>`; the page shows this message (the code itself is never
shown):

| Code | Console message | Usual cause |
| --- | --- | --- |
| `state` | Sign-in took too long or was interrupted. Try again. | The attempt's cookie is missing, altered, or does not match (a second tab, a blocked cookie, a reused link). |
| `expired` | Sign-in took too long or was interrupted. Try again. | The attempt is older than 10 minutes. |
| `idp` | Your identity provider refused the sign-in. | The provider answered with an error (the user said no, or the app is not assigned to them). |
| `token` | Single sign-on is not set up correctly. Ask an admin. | The code exchange failed or the ID token is not acceptable: wrong client secret, wrong issuer or client ID, unsupported algorithm, no usable email, a clock off by more than a minute. |
| `config` | Single sign-on is not set up correctly. Ask an admin. | Single sign-on is off or incomplete, the provider's discovery cannot be reached, or an internal error (see the log). |
| `not_allowed` | Your account is not allowed to sign in here. Ask an admin to invite you. | No linked user, and no verified email that links or creates one (see Who gets in). |
| `disabled` | Your account is disabled. | The user is disabled. |
| `rate_limited` | Too many sign-in attempts. Try again in a few minutes. | See below. |

Any other value is shown as "Single sign-on did not work. Try again." Details
of a failure go to the log as a reason code only; tokens, codes and cookies are
never logged. `uf_oidc_signins_total{result}` on `/metrics` counts every
callback by `ok`, `state`, `expired`, `idp`, `token`, `not_allowed`,
`disabled`, `rate_limited` and `config`.

**Limits and caching.** Starting a sign-in is limited to 60 per client address
in 15 minutes, and so is the callback (60 per client address in 15 minutes, a
successful sign-in forgives its own attempt); each is counted in a bucket of its
own, apart from password failures, so a third party cannot lock out password
sign-in by hitting either address. With single sign-on off the callback answers
`config` and counts nothing. Password sign-in keeps its own limit (20 failures
in 15 minutes per address), so set `UF_TRUSTED_PROXIES` behind a proxy.
The provider's discovery document is cached for 1 hour (a failure for 30
seconds); its key set for 1 hour, refetched when a token names an unknown key
but at most once a minute. Saving the settings starts with empty caches.
Accepted ID token algorithms: RS256, RS384, RS512, PS256, PS384, PS512, ES256
and ES384 (never `none`, HMAC or EdDSA). The client authenticates to the token
endpoint with HTTP basic, or in the body when the provider offers only that.

**Turning single sign-on off.** Users made by single sign-on have no password
and can sign in only while single sign-on works. If it is turned off, the
issuer is changed so that they no longer match, or the master key changes, they
get the same "Email or password is incorrect." as for any wrong password. The
Users page shows who is affected: "SSO only" users have no password, "Password
and SSO" users keep the password they had. Before turning it off, filter Users
by SSO and check the "SSO only" ones. To recover, turn single sign-on back on;
an admin can also delete the user and invite them again (which loses their keys
and ownership), and a user who had a password keeps signing in with it. A
disabled user made by single sign-on can be enabled again without a password.
Setting a password for an active user who has none is not available yet.

**Test configuration.** The Test button (`POST /api/settings/oidc/test`, admin
only) makes the gateway fetch the issuer's discovery document and then the key
set it names, and reports what it found. Only admins can use it; the address
it contacts is the issuer you typed, and the second request goes where the
discovery document points.

## Guardrails

A guardrail checks the text of calls and either **blocks** the call, **redacts**
what matched or **flags** it in the log. Admins manage them in the console under
Guardrails, or with `/api/guardrails/*` (admin only; leads and members have no
page and no endpoint, see the key's guardrails in Virtual keys). Nothing that
matched is ever stored, logged or shown: the log keeps names and counts.

**Kinds.**

- `rules`: run inside the gateway. Each rule has an id, a matcher (keywords, a
  regular expression, or PII types), an action (`block`, `redact`, `flag`) and
  the directions it applies to (`input`, `output`, `both`).
- `external`: a signed webhook you run decides (see External guardrails below).

*Keywords* are matched case-insensitively and by whole word by default
(`whole_word: false` matches substrings). A whole-word keyword written in a
script without spaces between words (Chinese, Japanese kana, Thai, Lao, Khmer,
Myanmar) matches as a substring, since there are no word edges. There is no
Unicode normalisation (NFC and NFD forms of a letter are different) and no
full case folding. Up to 1000 words of up to 256 characters.

*Regular expressions* use the Rust `regex` syntax (linear time; no
look-around or backreferences), up to 1 MiB compiled. Anchors (`^`, `$`, `\A`,
`\z`, also in `(?m)`) are refused, because a stream sees only a window of the
text. A pattern that can match the empty string is refused. Keywords and
regular expressions that contain a backslash, or a character of those scripts,
read the text as it is; other keywords treat a JSON escape as a word edge.

*PII types.* A rule lists the types it looks for:

| Type | Finds | Known misses |
| --- | --- | --- |
| `EMAIL` | addresses; letters of any script in the local part and domain | quoted local parts, comments |
| `PHONE` | 10 to 15 digits with a `+`, a parenthesised area code, a 3-3-4 North American grouping, or a national number with a trunk `0` in groups of two or more | bare 10-digit numbers without `+` or separators (so ids and timestamps are left alone); numbers written in other groupings; two numbers separated only by a space can merge past 15 digits and be skipped |
| `CREDIT_CARD` | 13 to 19 digits (spaces or dashes) that pass the Luhn check | cards written with other separators |
| `IBAN` | two capitals, two digits and 11 to 30 more characters that pass mod-97 | no length table per country, so about 1 in 100 random IBAN-shaped strings is taken for one |
| `US_SSN` | `123-45-6789` (area codes 000, 666 and 9xx excluded) | numbers without dashes |
| `IPV4` | four numbers up to 255, no leading zeros | |
| `IPV6` | addresses that parse and have a hex digit or at least three colons (`a::b` is not one) | |
| `SECRET` | exactly these shapes: `sk-` (which includes `sk-ant-` and `sk-proj-`, 20 to 200 characters after the prefix), `AKIA` + 16, `ghp_` (36 to 100), `github_pat_` (22 to 100), `xox[abpre]-` (10 to 100), `AIza` + 35, `uf-sk-` and `uf-at-` (16 to 200), and private-key blocks (PEM and PGP, from the `BEGIN` line to the `END` line) | every other credential format, Stripe's `sk_live_` among them (use a regular expression); an `sk-` key longer than 203 characters is redacted only as far as the 203rd |

A redaction puts `[REDACTED:EMAIL]` (the type) or `[REDACTED]` (a keyword or
regular expression) in the text. Rules match the **original** text, never
another rule's replacement. Overlapping redactions merge into one span (the
placeholder of the one that starts first, ties to the earlier rule), so part of
a match is never left visible. A **block** anywhere beats every redaction and
leaves the text untouched. A private-key block is redacted whole, from its
`BEGIN` line to its `END` line; in a stream everything after a `BEGIN` line is
held back until the `END` line (or the end of the stream), and one placeholder
is sent. An address glued directly to an `END` line may show its domain in a
stream, or a separate `[REDACTED:EMAIL]`, so the stream is then not exactly the
whole-text redaction (no key material is involved).

**Which guardrails apply, and in which order.** For every call: the guardrails
marked *applies to every call* (by name), then those of the key's team (if it
has one) and the other teams of the key's owner, then those of the key's owner, then those of the route, then those of
the key, each in the order set on it, each guardrail once, at its first place.
A disabled guardrail is not part of it. Attach them in the console (route form,
key form, the team and user pages) or with `guardrail_ids` on
`POST/PUT /api/routes`, `POST /api/keys`, `PATCH /api/keys/{id}`,
`PUT /api/teams/{id}/guardrails` and `PUT /api/users/{id}/guardrails` (admins
only; on a route `PUT`, leaving the field out keeps the attachment and `[]`
takes them all off; at most 20 each). All **built-in rules run before any
external guardrail**, whatever the order; the external ones then run in order,
over the text the rules left. A rule block means no external guardrail is asked.

*What attaching does not do.* A guardrail on a key checks the calls made with
that key, and one on a route checks the calls that go through that route.
Neither follows a person or a model: a member can make a new key, which has no
guardrails of its own, and a key that may also call `openai/gpt-4o` directly
skips the guardrail on the route `support` that serves it. A guardrail on a
team or a user follows every key of that team or owned by that user, also a key
made afterwards. A policy that must hold for everyone is a guardrail marked
*applies to every call*. To make a route's guardrails certain, give people keys
whose models list names only the route.

**What is checked.**

- *Input* (`input`), before the call is routed: every text part of every message
  (system and tool results included; the text parts of one message that follow
  each other are joined and checked as one text, and multiple Anthropic
  `system` blocks are now joined with a newline), the `name` of a message,
  the arguments of tool calls in the history, each input of an embeddings
  call, the `input` and `instructions` and function results of a Responses
  call, the `prompt` of an image generation, the `input` of a speech call and
  the `prompt` field of a transcription or translation (the text of a prompt
  template is checked after it is rendered). The rate limits come first, so a caller who is rate-limited is refused
  before anything of the body is scanned or sent to your webhook (a body is
  not scanned for nothing). Then the built-in rules run, then the budgets, then
  the external guardrails. A call a guardrail blocks gives its request and its
  token estimate back to the limits: a blocked call spends no quota, no tokens
  and no budget. The token estimate is that of the input as it came, before
  redaction.
- *Output* (`output`): the text of the answer and the arguments of its tool
  calls, whole or streamed. Tool-call arguments are scanned as text, not as
  JSON: a keyword or regular expression whose match spans a quote, or a private
  key `BEGIN` line with no `END` line in streamed arguments, can leave arguments
  that are no longer valid JSON.
- *Output of audio calls*: the transcript of a transcription or translation,
  as the words that were spoken: the `text` of `json`, the `text`, the
  segments' text and the words of `verbose_json`, a `text` answer whole, and
  only the cue lines of `srt` and `vtt` (never the numbers or the times; the
  lines of one cue are checked as one text). A blocked transcript is a 400
  `guardrail_blocked`; a redacted one is rewritten. Images that are generated
  are not inspected.
- *Not checked*: the pictures and sounds themselves (images sent in, images
  generated, uploaded audio, spoken speech), tool definitions (names,
  descriptions, schemas) and stop sequences. No machine-learning classifier (toxicity, prompt
  injection) is built in: use an external guardrail.

**What happens.**

| Action | Input | Output |
| --- | --- | --- |
| `block` | the call is refused with 400 and the code `guardrail_blocked` (OpenAI shape; the Anthropic error has no code): "Blocked by guardrail '<name>'."; no provider is called and nothing is charged | the answer is replaced by an empty one that ends with `content_filter` (`refusal` in the Anthropic format); a stream ends the same way, nothing held is released |
| `redact` | the provider receives the redacted text | the caller receives the redacted text |
| `flag` | only recorded | only recorded |

*Streams.* To redact a match that arrives in pieces, the gateway holds back the
last 256 characters of each text until the next ones show whether they belong
to a match; a match that straddles the edge makes it hold up to 2 x 256 + 128
characters. A stream therefore lags by about that much, and ends as the
whole answer would have been redacted (with the one exception above). If the
provider fails in the middle of an answer, the clean text the scanner was still
holding is sent before the error (an answer held for an external guardrail is
dropped instead, see below). A regular-expression match **longer than
256 characters may be missed or split in a stream** (keywords are limited to 256
characters for that reason). Checking one text costs time proportional to its
length: texts over 64 KiB are scanned on a separate thread, and no more scans
of that kind run at once than the machine has CPUs, so a burst of large bodies
cannot take every core. A scan that finds no free slot within 10 seconds is
refused with 503. Text made of near-misses (`1 1 1 `, `10.0.0.`) is the slowest
to scan: about 0.2 s of CPU per MiB with every PII type on (a development
machine), against about 0.02 s for prose.

*Cache.* A blocked answer is never cached; a redacted answer is (the cache key
includes the guardrails that apply, so keys with different guardrails never
share an answer). A cache hit records the output outcome of the answer it
gives (what was redacted when the answer was kept), so the logs, the filter
and the metrics count it. Metrics and logs count calls that went through the
guardrails.

**In the logs.** Each call a guardrail acted on carries the worst action
(`blocked` > `redacted` > `flagged`), and for input and output the guardrails it
was checked with, the one that blocked, counts of replacements by PII type or
rule id, and the flag rules that matched. The Logs page shows a badge and has a
Guardrails filter (`GET /api/logs?guardrail=blocked|redacted|flagged`), and the
call's page lists the details; only admins see which guardrail, rule or type
did it, everyone else (members and team leads) sees the action of each
direction. Metrics:
`uf_guardrail_actions_total{action,direction}` and
`uf_guardrail_external_errors_total{reason}` (see Operations). The playground is
a call like any other: a blocked message shows the guardrail's name, and an
answer a guardrail stopped says so.

**Try it.** In the guardrail form, *Try it* runs the rules on a text and shows
the result with the placeholders marked (`POST /api/guardrails/test`; nothing
is stored or logged, and it never echoes what matched). For a saved external
guardrail it can call the webhook for real (`call_external`); the text is then
sent to its URL, and the call counts against the same 64-call limit as real
calls (below).

**Not in force.** The guardrail list shows *Not in force* on a rules guardrail
whose stored rules the gateway is not running (they do not compile; it keeps
running the rules it last compiled, if it had any) and *Cannot be called* on an
external guardrail whose URL or secret cannot be read (it fails by its mode on
every check). `GET /api/guardrails` carries this as `usable`.

### External guardrails

An `external` guardrail posts the texts of a call to a URL you run. The URL is
`http://` or `https://`, stored encrypted and never shown again (only its host
is); an admin can point it at any host, as for providers and alert channels.
Creating the guardrail (or *Rotate secret*) shows its signing secret
(`whsec_...`) once.

The request is a POST of JSON, signed like an alert delivery:

```json
{ "version": 1, "direction": "input", "endpoint": "chat", "model": "support-chat",
  "texts": ["first text", "second text"], "route": "support-chat",
  "key_id": 7, "team_id": 2, "user_id": 3 }
```

`endpoint` is `chat`, `messages`, `responses`, `embeddings`, `images`,
`transcriptions`, `translations`, `speech`, `playground` or `test`;
`direction` is `input` or `output`. `texts` are the slots described above (an
answer is `[answer text, arguments of each tool call]`; texts that are all
empty are not sent). No images, keys or credentials are sent. Answer 2xx with
JSON of at most 1 MiB:

```json
{ "action": "allow" }
{ "action": "block" }
{ "action": "redact", "texts": ["first text, changed", "second text"] }
```

`redact` carries exactly one replacement for each text, in order; a reason, if
you send one, is ignored. **Signature.** Every request carries
`x-uf-signature: t=<unix seconds>,v1=<hex>`, where `v1` is the lower-case hex
HMAC-SHA256, keyed by the whole secret string (`whsec_...` included), of
`<t>.<raw body>`. Check it over the raw bytes, compare in constant time, and
reject a `t` more than 5 minutes old. Python:

```python
import hmac, hashlib, time
t, v1 = [part.split("=", 1)[1] for part in header.split(",")]
signed = t.encode() + b"." + raw_body
expected = hmac.new(secret.encode(), signed, hashlib.sha256).hexdigest()
if not hmac.compare_digest(expected, v1): reject()
if abs(time.time() - int(t)) > 300: reject()
```

Node:

```js
const crypto = require("node:crypto");
const [t, v1] = header.split(",").map((part) => part.split("=")[1]);
const hmac = crypto.createHmac("sha256", secret).update(`${t}.${rawBody}`);
const expected = hmac.digest("hex");
if (expected.length !== v1.length || !crypto.timingSafeEqual(Buffer.from(expected), Buffer.from(v1))) reject();
if (Math.abs(Date.now() / 1000 - Number(t)) > 300) reject();
```

*Timeouts and failures.* The call has the guardrail's timeout (1000 to 10000 ms,
default 3000) and, for an answer (held, or a whole one), no more than the time the request has
left; no redirects. A timeout, a connection error, a status other than 2xx, a redirect,
an answer over 1 MiB or not in the form above, or a `redact` with the wrong
number of texts is a failure, and the **fail mode** decides: `open` lets the text
through and flags the call, `closed` blocks it (and flags it). The flag is
`external_error:<reason>` with the reason `timeout`, `connect`, `status`,
`too_large`, `invalid`, `buffer_full`, `busy` or `other` (`other` includes a
guardrail whose URL or secret cannot be read: it is never silently off). A
failure is counted in `uf_guardrail_external_errors_total{reason}`; the log
and metrics hold the guardrail's name and host, never the URL or any text. Two
external guardrails run one after the other, so a call can wait the sum of
their timeouts. At most 64 calls to one guardrail's URL are in flight in a
process; more fail by the fail mode with the reason `busy`. A call whose texts
are all empty asks no external guardrail, so even a fail-closed one does not
block it.

*Streams.* An external guardrail on the output holds the whole answer: nothing
is sent until the stream ends and the webhook has answered, then the answer
arrives in one piece (redacted if told), so the first byte is late. While it is
held, the gateway sends SSE comment lines (`: keepalive`) every 10 s so proxies
with idle timeouts keep the connection. Held text over the provider response
limit (32 MiB by default) cannot be checked: fail closed ends the stream with a
`content_filter`, fail open releases what is held and lets the rest through,
flagged `buffer_full`, and the built-in rules still apply. A provider error
while an answer is held drops the held text. An answer an external guardrail
could not check is not cached. A webhook cannot redact an answer, or the texts of
an input, so that they exceed 1 MiB.

*Configuration files.* Export and import carry guardrails and their attachments
(to routes and teams; user attachments are not in the file, as users are not)
by name, never a URL or secret: an imported external guardrail is created off,
with a warning, until an admin sets its URL in the console.

## Using PostgreSQL

SQLite is the default and needs nothing. Use PostgreSQL when more than one
gateway process should share a database (several replicas behind a load
balancer) or when you already run and back up PostgreSQL. It is tested on
PostgreSQL 14 and 17; the gateway creates its tables itself (migrations run at start) in the
database and schema the URL names, so give it a database of its own. The
gateway logs `database: postgres` (or `sqlite`) when it starts; the URL, which
holds the password, is never logged.

```bash
export UF_MASTER_KEY=$(openssl rand -hex 32)      # keep it: see below
export UF_DATABASE_URL='postgres://ultrafast:PASSWORD@db.example.com:5432/ultrafast?sslmode=require'
ultrafast serve
```

A complete example with PostgreSQL in a container is
[`docs/compose/postgres.yml`](docs/compose/postgres.yml). It pins
`2.0.0-beta.3`, the first release that can use PostgreSQL: a gateway of an
older release ignores `UF_DATABASE_URL` and runs SQLite, so use beta.3 or later.

- **The master key is required.** There is no data directory to keep a
  `master.key` in, and every gateway on the database must have the same key, or
  provider credentials cannot be read by the others. `UF_MASTER_KEY` is 64 hex
  characters; the gateway refuses to start without it. Keep it apart from the
  database backups.
- **TLS** is the URL's `sslmode`: `disable`, `prefer`, `require` (encrypted, the
  server's certificate is not checked) or `verify-full` (checked; give the
  authority with `sslrootcert=/path/ca.pem`). Use `require` or `verify-full`
  for any database that is not on a private network. The gateway gives up
  connecting after 10 s and says so at start (a refused connection is retried
  until then, and named); running the migrations is not timed. During an
  outage the console's and the API's database calls also answer after 10 s.
- **Connections.** Each process opens up to `UF_DATABASE_MAX_CONNECTIONS` (10).
  Keep processes times that number below the server's `max_connections`.
- **Several processes behind a load balancer.** They share the database, so
  users, sessions, keys, models, routes, teams, logs, the audit log and alert
  channels, rules and history are the same on every one, and a change made on
  one reaches the others' routing snapshot within about 30 s (each re-reads
  the database at that interval), while the process that took the change
  applies it at once. **That includes revocations**: after you revoke a key,
  disable a user or remove a grant, `/v1` on the other processes may keep
  accepting it for up to 30 s. Console sessions are checked in the database
  on every call and end at once on every process. Single sign-on settings
  follow the same interval (a start or a callback also checks the stored
  on/off switch at once). Budgets are shared too: each process adds its
  spend to the database about every 5 s and reads the others' back, so a
  `block` budget can be overshot by what the processes spend inside that
  interval. A process that starts next to running ones rebuilds its budgets
  from the logs, which already hold what the others have not yet added, and
  may count up to one interval of their spend a second time, once. Budget
  alert states are shared and fire once. An error-rate or circuit alert
  episode belongs to the process that opened it (it rests on that process's
  own calls or breaker): the other processes leave it alone, and take it over
  only when its owner has been silent for three alert ticks (90 s), when they
  resolve it or go on with it. These stay **per
  process**: rate limits (a limit of 60 requests a minute is 60 on each
  process), the response cache, single-flight, routing and circuit-breaker
  health, the first-run setup code, the sign-in attempt limits (a client
  limited on one process can still try the next), and the error windows and
  circuit episodes that alerts are evaluated from. Use sticky routing if a
  client relies on cache hits.
- **First start of several processes.** Each process makes a setup code of its
  own while no user exists, so give the first start `UF_ADMIN_EMAIL` and
  `UF_ADMIN_PASSWORD` (the admin is made once, whichever process gets there
  first), or start one process and finish the setup before starting the rest.
- **Backup** is `pg_dump` (or your provider's snapshots); the console's Backup
  panel and `ultrafast backup` say so and do nothing. Back up the database and
  keep `UF_MASTER_KEY` separately. Restore with `pg_restore` or `psql` into an
  empty database, with the gateways stopped, and start them with the same key.
- **Upgrade.** Migrations run when the first gateway starts and take a
  database-wide lock, so processes started together wait for each other. Take
  a `pg_dump` first. A binary older than the schema refuses to start once a
  newer one has migrated the database, so going back to an older release means
  restoring the dump taken before the upgrade.

**Moving from SQLite to Postgres.** There is no online migration. A
configuration export and import moves the setup, not the data:

| Moves (`config export`, then `config import` on the Postgres gateway) | Does not move |
| --- | --- |
| providers (name, kind, URL), models with their grants, routes, teams, gateway/team/user limits and budgets, alert channels (name and kind) and rules, the retention and session-hour settings | provider credentials (set them again, or the providers stay without one), users and their passwords, invitations and sessions (people are invited again, or the first admin is made from `UF_ADMIN_EMAIL`/`UF_ADMIN_PASSWORD`), virtual keys (create new ones), request logs and usage history, the audit log, alert history, alert channel URLs and secrets, single sign-on settings, and the response cache |

```bash
ultrafast --data-dir ./data config export ./config.json
UF_DATABASE_URL=... UF_MASTER_KEY=... UF_ADMIN_EMAIL=... UF_ADMIN_PASSWORD=... ultrafast serve   # first start: makes the admin
UF_DATABASE_URL=... UF_MASTER_KEY=... ultrafast config import ./config.json --dry-run
UF_DATABASE_URL=... UF_MASTER_KEY=... ultrafast config import ./config.json
```

Users and keys are not in the file, so the people and the applications that
call the gateway start over; keep the old gateway running until they have
moved. Going from Postgres back to SQLite is the same, the other way round.

## Operations

**Backup.** (SQLite; on PostgreSQL use `pg_dump`, see Using PostgreSQL.) A consistent copy of the database while it runs: Settings, Backup;
`GET /api/backup` (admin); or

```bash
ultrafast --data-dir ./data backup ./backups/ultrafast-2026-10-04.db   # the file must not exist
```

It holds users, sessions, logs, the audit log and encrypted provider
credentials, but not the master key. Without `master.key` (or `UF_MASTER_KEY`)
it is useless: keep that key safe, apart from the backups.

**Restore.** Stop the gateway cleanly, keep the old files, put the backup in
place, start:

```bash
mv data/gateway.db data/gateway.db.before
[ -e data/gateway.db-wal ] && mv data/gateway.db-wal data/gateway.db.before-wal
[ -e data/gateway.db-shm ] && mv data/gateway.db-shm data/gateway.db.before-shm
cp backups/ultrafast-2026-10-04.db data/gateway.db && chmod 600 data/gateway.db
```

The gateway must have the master key of the one the backup came from.

**Configuration export and import.** Moves the setup, not the data: providers
(without credentials), models and grants, routes, teams, gateway/team/user
limits and budgets, settings. Settings, Configuration in the console (with a
dry run first), `GET /api/config/export` and `POST /api/config/import`, or:

```bash
ultrafast --data-dir ./data config export ./config.json          # file must not exist
ultrafast --data-dir ./other config import ./config.json --dry-run
ultrafast --data-dir ./other config import ./config.json
```

A file with errors is refused whole. An import creates and updates by name and
never deletes; new providers have no credential until an admin sets one.

**Upgrade.** Stop, replace the binary (or image), start: migrations run at
start. Back up first. Coming from `2.0.0-alpha.1`: existing providers work only
after an admin syncs, enables and grants their models (console, Models, or
`ultrafast model add --provider NAME --model ID --enable --everyone`); until
then calls to `NAME/MODEL` are refused.

**Metrics.** Set `UF_METRICS_TOKEN`, then scrape `GET /metrics` with
`Authorization: Bearer <token>`. Without a token the path does not exist; a
wrong token is 401.

```yaml
scrape_configs:
  - job_name: ultrafast
    authorization: { credentials_file: /etc/prometheus/ultrafast-token }
    static_configs: [{ targets: ["127.0.0.1:3000"] }]
```

Series: `uf_requests_total{endpoint,status_class}`, `uf_tokens_total{direction}`,
`uf_cost_micros_total`, `uf_upstream_duration_seconds{provider}`,
`uf_cache_hits_total`, `uf_cache_misses_total`, `uf_cache_flight_waits_total`, `uf_rate_limited_total{limit}`,
`uf_budget_blocked_total`, `uf_guardrail_actions_total{action,direction}`
(`block`, `redact`, `flag`; `input`, `output`; one count per call),
`uf_guardrail_external_errors_total{reason}` (checks by an external guardrail that failed:
`timeout`, `connect`, `status`, `too_large`, `invalid`, `buffer_full`, `busy`, `other`),
`uf_circuit_open{provider,model}`,
`uf_log_records_dropped_total`, `uf_log_write_failures_total`,
`uf_otel_spans_exported_total`, `uf_otel_spans_dropped_total`,
`uf_otel_export_failures_total`, `uf_alert_deliveries_total{result}`
(`ok`, `failed`, `dropped`). No label names a
key, user, team or prompt. `GET /health` answers `{"status":"ok"}`.

**Tracing.** Set `UF_OTEL_ENDPOINT` and every `/v1` call (chat, messages,
responses, embeddings, images, audio) and every playground call becomes a trace, exported in the
background over OTLP/HTTP with JSON bodies:

```bash
UF_OTEL_ENDPOINT=http://localhost:4318 \
UF_OTEL_HEADERS='authorization=Bearer ...' \
UF_OTEL_SAMPLE_RATIO=0.25 ultrafast serve
```

- *Format.* Only OTLP JSON over HTTP is spoken: no protobuf, no gRPC, and
  traces only (no OTLP metrics or logs). For a backend that takes only
  protobuf or gRPC, put an OpenTelemetry Collector in front, with an OTLP/HTTP
  receiver (port 4318).
- *Context.* A valid W3C `traceparent` header on a `/v1` call is honored: the
  trace continues under the caller's trace id and parent span, and a sampled
  flag of 0 means the call is not exported. Without one, the gateway starts a
  trace and `UF_OTEL_SAMPLE_RATIO` decides. A call that carries a sampled
  `traceparent` is exported at any ratio.
- *Spans.* One server span per call, named `uf.<endpoint>` (`uf.chat`,
  `uf.messages`, `uf.responses`, `uf.embeddings`, `uf.images`,
  `uf.transcriptions`, `uf.translations`, `uf.speech` or `uf.playground`), with `uf.endpoint`, `uf.requested` (the
  model or route the caller asked for, cut at 256 bytes), `http.response.status_code`, `uf.stream`, `uf.cached`,
  `uf.estimated`, `uf.key_id` / `uf.user_id` / `uf.team_id` when known,
  `gen_ai.usage.input_tokens` / `gen_ai.usage.output_tokens` when counted,
  `uf.guardrail.action` (`blocked`, `redacted` or `flagged`, the worst the
  guardrails did to the call; absent when they found nothing),
  `uf.prompt_template` (`name@version`, when the call used a prompt template;
  the template's text is never in a span), and
  the call's tags as `uf.tags.<name>`. Status is an error for 5xx. Under it, one
  client span per attempt that reached a provider, named
  `uf.attempt <provider>`, with `uf.provider`, `gen_ai.request.model`,
  `gen_ai.system` and `gen_ai.provider.name` (the provider kind: `openai`,
  `anthropic`, ...; left out when unknown), `http.response.status_code` when
  there was an answer, and `uf.outcome` (`ok`, `retryable`, `fatal`). Targets
  skipped or refused by an open circuit are events on the server span
  (`uf.skipped`, `uf.circuit_open`). A cache hit is a server span alone. No
  prompt, answer, header or credential is ever in a span.
- *Best effort.* The request path only hands the finished call to a bounded
  queue (4096 calls); it never waits for the collector. When the queue is full,
  or a batch cannot be delivered (batches go out every 2 s or when large, 10 s
  timeout, no retry), those spans are dropped and counted
  (`uf_otel_spans_dropped_total`, `uf_otel_export_failures_total`; sent spans
  in `uf_otel_spans_exported_total`). Spans a collector rejects in a partial
  success count as dropped. Calls not chosen by the sample ratio are not
  counted as dropped. On shutdown the exporter gets 5 s in all to send what is
  left; the rest is lost.

**Alerts.** Admins (only admins; leads and members do not see the page) set up
alerts in the console under Alerts, or with `/api/alerts/*`: channels say where
to send, rules say when. Each rule has one or more channels. When a rule starts
firing, and when it resolves, an event is stored (visible in the Alerts
history) and delivered to its channels.

*Channels.* A `webhook` channel receives JSON; a `slack` channel receives
`{"text": "Alert firing (Rule name): summary"}`, which Slack-compatible
incoming webhooks (Slack, Mattermost, Rocket.Chat) accept. The URL is
`http://` or `https://` and is stored encrypted; it is never shown again (the
console shows the host only). Creating a channel returns its signing secret
(`whsec_...`) once; rotating it shows the new one once. A **Test** button sends
a sample event and reports the result. It is stored in History as a `test`
event and is not counted in `uf_alert_deliveries_total`.

A webhook body:

```json
{ "version": 1, "id": 42, "state": "firing",
  "rule": { "id": 3, "name": "Team budget", "kind": "budget" },
  "subject": "budget:12:2026-10-01",
  "summary": "Budget '...' passed 80% (...)",
  "details": { },
  "at": "2026-10-08T10:20:30Z", "gateway": "ultrafast 2.0.0" }
```

`state` is `firing` or `resolved` (`test` for a Send test). The body holds
metadata only: no prompt, answer or key. Budget summaries name the budget's
subject (a team name, a user's email, a key's name), and that text goes to the
channel's third party.

*Signature.* Every delivery (slack channels too) carries
`x-uf-signature: t=<unix seconds>,v1=<hex>`, where `v1` is the lower-case hex
HMAC-SHA256, keyed by the whole secret string (`whsec_...` included), of
`<t>.<raw body>`. Check it over the raw bytes, compare in constant time, and
reject a `t` more than 5 minutes old. Python:

```python
import hmac, hashlib, time
t, v1 = [part.split("=", 1)[1] for part in header.split(",")]
signed = t.encode() + b"." + raw_body
expected = hmac.new(secret.encode(), signed, hashlib.sha256).hexdigest()
if not hmac.compare_digest(expected, v1): reject()
if abs(time.time() - int(t)) > 300: reject()
```

Node:

```js
const crypto = require("node:crypto");
const [t, v1] = header.split(",").map((part) => part.split("=")[1]);
const hmac = crypto.createHmac("sha256", secret).update(`${t}.${rawBody}`);
const expected = hmac.digest("hex");
if (expected.length !== v1.length || !crypto.timingSafeEqual(Buffer.from(expected), Buffer.from(v1))) reject();
if (Math.abs(Date.now() / 1000 - Number(t)) > 300) reject();
```

*Delivery.* A delivery is a POST with a 10 s timeout and no redirects; any
answer other than 2xx, a timeout or a connection error counts as a failure.
Up to three tries: now, 5 s later, 30 s after that. Delivery runs on a
background task behind a bounded queue and never touches `/v1`. Each channel
has at most 4 deliveries in progress (retries included) and 256 waiting; more
are dropped. A full queue drops too. Every drop (a full queue, or a channel's
backlog) is recorded on the event and counted in
`uf_alert_deliveries_total{result="dropped"}` (`ok` and `failed`
are the other results). A delivery still waiting or retrying when the gateway
shuts down (5 s allowed) is cut off and not retried at the next start; its
event keeps no outcome. The outcome of each channel (tries, last error as a
fixed phrase, never the URL) is on the event.

*Rules.*

- `budget` (`budget_id`, or none for every budget, key budgets included; `percent`
  1 to 100): fires once per budget period when the spend passes that share of
  the limit. It is evaluated when spend is recorded (about every 5 s), so a
  budget already past the percent when the rule is created fires on its next
  spend. What already fired survives a restart (no second alert in the same
  period); a new period can fire again. A budget rule sends no `resolved`
  notice. A rule on a key's budget works but is not part of a configuration
  export.
- `error_rate` (`scope` `gateway`, `route`, `provider` or `key`; `subject` the
  route or provider name or key id, none for gateway, or to watch each route,
  provider or key on its own; `percent`;
  `window_minutes` 5 to 60, default 5; `min_requests` 1 to 100 000, default 20):
  fires when the share of failed calls in the window reaches `percent` and the
  window holds at least `min_requests` calls. A failure is a 5xx answer,
  or a 429 the provider gave; the gateway's own 429s (rate limit,
  budget), other 4xx and 499 (caller left) are not failures. A route subject
  counts only configured routes (a call to `provider/model` counts under its
  provider, key and the gateway, not under a route), and a call that never
  resolved to a model or route (404, 403) is not counted. Playground calls are
  counted like any other. It resolves after the
  rate has stayed under `percent` for one full window; because old failures
  must also leave the window, that is up to about two windows after the last
  failure.
- `circuit_open` (`provider`, `model`, each optional: left out means any):
  fires when a matching target's circuit breaker opens. It resolves only after
  the breaker has stayed closed for a quiet period of 5 minutes (checked every
  30 s), or when the target is removed from the catalog. A breaker that opens
  again inside the quiet period continues the same episode and says nothing, so
  a provider that flaps is one `firing` and one `resolved`, not a pair for
  every flap.

Disabling a rule forgets what it was firing for, silently: no `resolved`
notice is sent, and enabling it again starts fresh. Changing a rule's params
does the same. A rule's `kind` cannot be changed.

*Configuration export and import* carry alert channels (name and kind only, no
URL or secret) and rules (with their channel names, and budget rule targets by
name). An imported channel is created disabled, with a new secret and no URL:
an admin sets the URL in the console (Alerts), rotates the secret if needed and
enables it. `ultrafast config import` cannot create a channel (it has no access
to the encryption key); use the console for the first import of a file that
holds new channels. Error windows are not exported.

**Reverse proxy and Cloudflare.** Start with `--trusted-proxy CIDR` (or
`UF_TRUSTED_PROXIES`) so sign-in limiting counts the client's address, not the
proxy's. The proxy must set or overwrite `CF-Connecting-IP` and
`X-Forwarded-For` itself and never pass on what the client sent; never list a
network clients can reach directly. Without it, 20 failed sign-ins from anyone
behind a proxy block sign-in for everyone for 15 minutes.

**HTTPS and cookies.** The session cookie is `Secure`, so the console needs
HTTPS except on localhost; over plain HTTP elsewhere the browser drops the
cookie and the sign-in page says so. Terminate TLS at the proxy, or on a
trusted network use `--insecure-cookies`. `/v1` with a key works over HTTP.

## Clients

Rust, Python and TypeScript clients for the gateway (or a provider directly),
built on one Rust core, `ultrafast-translate`, and tested with shared fixtures
(`clients/fixtures/`). They do not retry, route or cache; an error says
whether to retry and when. Wheels and an npm package are not published yet,
and the crates are not on crates.io: build from source as each README says.

Rust ([`crates/client`](crates/client/README.md)):

```rust
use ultrafast_client::{ChatRequest, Client, Target};

let client = Client::new(Target::gateway("http://127.0.0.1:3000", key));
let reply = client.chat(ChatRequest::new("anthropic/claude-sonnet-5").user("Say hi.")).await?;
println!("{}", reply.content);
```

Python ([`crates/client-py`](crates/client-py/README.md)):

```python
import ultrafast

client = ultrafast.Client(ultrafast.gateway("http://127.0.0.1:3000", key))
reply = client.chat("anthropic/claude-sonnet-5", [{"role": "user", "content": "Say hi."}])
print(reply.content)
```

TypeScript ([`clients/ts`](clients/ts/README.md)):

```ts
import { Client, gateway } from "@ultrafast/client";

const client = new Client(gateway({ baseUrl: "http://127.0.0.1:3000", key }));
const reply = await client.chat({ model: "anthropic/claude-sonnet-5", messages: [{ role: "user", content: "Say hi." }] });
console.log(reply.content);
```

All three also stream and make embeddings, and send `tags` as `x-uf-tags`.
They also take tools and images: Rust through `ChatRequest` (tools, tool choice,
image and tool-result messages), Python with flat tool dicts
(`{"name", "description", "parameters"}`) and a `ToolCall` you can pass straight
back in the next assistant message, TypeScript with `tools`, `toolChoice` and
`toolCalls` / `toolCallId`; see each README. Streams yield tool-call events. All three take `response_format` (Rust
`ChatRequest::response_format`, Python `response_format=` with the OpenAI
shape, TypeScript `responseFormat` with `jsonSchema` in camelCase). They do not
yet call `/v1/responses`, images, audio or prompt templates; use any OpenAI
SDK against the gateway for those.

### Admin SDKs

Two more clients manage the gateway instead of calling models: they wrap the
admin API (`/api`: providers, models, routes, keys, users, teams, limits,
budgets, alerts, guardrails, prompts, settings, usage, logs, audit, backup and
configuration). Both are generated from `openapi/admin.json`, authenticate with an
access token (Account, Access tokens in the console), and raise a typed error
with the gateway's `status`, `code` and field messages. They are not published
to npm or PyPI: build from source. CI regenerates both and fails when the
committed copy differs.

TypeScript ([`clients/admin-ts`](clients/admin-ts/README.md)):

```sh
pnpm --dir clients/admin-ts install && pnpm --dir clients/admin-ts build
# then depend on the folder: "@ultrafast/admin": "file:../ultrafast-ai-gateway/clients/admin-ts"
```

```ts
import { createAdminClient } from "@ultrafast/admin";

const api = createAdminClient({ baseUrl: "http://127.0.0.1:3000", token: process.env.UF_ADMIN_TOKEN! });
const { providers } = await api.call(api.raw.GET("/api/providers"));
console.log(providers.map((p) => p.name));
const { channel } = await api.call(
  api.raw.POST("/api/alerts/channels", { body: { name: "ops", kind: "webhook", url: "https://example.com/hook" } }),
);
console.log("created channel", channel.id);
```

Python 3.11 or newer ([`clients/admin-py`](clients/admin-py/README.md)):

```sh
pip install ./clients/admin-py
```

```python
import os
from ultrafast_admin import AdminClient
from ultrafast_admin.api.providers import providers_list
from ultrafast_admin.api.alerts import alerts_channels_create
from ultrafast_admin.models import CreateChannelRequest

with AdminClient("http://127.0.0.1:3000", os.environ["UF_ADMIN_TOKEN"]) as api:
    print([p.name for p in api.call(providers_list.sync_detailed(client=api.client)).providers])
    body = CreateChannelRequest(name="ops", kind="webhook", url="https://example.com/hook")
    print("created channel", api.call(alerts_channels_create.sync_detailed(client=api.client, body=body)).channel.id)
```

## Known limits

- SQLite: one gateway per database. Postgres: several processes may share one
  database, with per-process rate limits, response cache, single-flight,
  routing and breaker health, setup code and alert error windows (a limit of
  N is N on each process). Budgets are shared and converge about every 5 s, so
  a `block` budget can be overshot across processes by one interval's spend.
  A change made on one process reaches the others within about 30 s,
  revocations of keys, users and grants included: `/v1` on another process
  may accept a revoked key for up to that long (console sessions end at
  once). Sign-in attempt limits are per process too.
  On either database these start empty after a restart (budgets are rebuilt
  from the logs).
- A `block` budget can be overshot: spend is counted when the log writer
  prices a call (batches of about a second), and a long call is charged when
  it ends.
- A stream whose caller left, or that failed midway, is charged an estimate
  (marked Estimated in the logs).
- Creating or revoking a key, or changing teams, users, routes, providers,
  models or grants, clears the whole response cache.
- Single-flight is per process, and a caller waiting on another's call keeps
  its concurrency slot while it waits.
- A logged `requested` name (the model or route a caller asked for) is cut at
  256 bytes.
- Request logs keep metadata only. Logs of a deleted user or team stay, with no
  owner.
- Members see their own usage and budgets, team leads their teams', admins
  all; only admins set limits and budgets. A budget with the `alert` action
  writes an audit entry; to be notified, add a budget alert rule (Alerts).
- Traces are best-effort: spans are dropped, and counted, when the collector is
  slow or down, and what is queued at shutdown beyond 5 s is lost. OTLP JSON
  over HTTP, traces only; no protobuf, gRPC, OTLP metrics or logs.
- Alerts are evaluated in one process: error windows are in memory and start
  empty after a restart (an error-rate alert needs `min_requests` calls again).
  Webhooks only (generic and Slack-compatible): no email, no PagerDuty format.
  Delivery is at most three tries and a delivery cut off at shutdown is not
  retried. Webhook URLs are not restricted to public addresses (admins already
  set provider URLs). Only admins see alerts; leads cannot. A circuit alert
  resolves only after its breaker has stayed closed for 5 minutes. Alert
  history (events) is deleted with the request logs, after the log retention
  period.
- Guardrails: images and audio themselves are not inspected (the text of an
  image prompt, of speech input and of a transcript is), and tool definitions and
  stop sequences are not scanned. There are no built-in classifiers (toxicity,
  prompt injection): use an external guardrail. Regular-expression matches
  longer than 256 characters may be missed in streams, and a stream lags by
  the hold-back (256 characters, up to 2 x 256 + 128). Rules read the text as
  it is: no Unicode normalisation, JSON escapes count as word edges, and the
  PII detectors have known misses (a bare 10-digit phone number, an IBAN not
  checked against a length table, IPv6 without a digit or three colons).
  A guardrail on a route can be stepped around with a direct model call, and
  one on a single key with another key (see Which guardrails apply).
  A private-key block in a stream swallows everything up to its `END` line;
  an address glued to an `END` line may show its domain. External output
  checks hold the whole stream (and a stream over 32 MiB is not checked when
  the guardrail fails open); external guardrails are called with a timeout
  and at most 64 at a time per guardrail and process; their URLs are not
  restricted to public addresses (admins already set provider URLs). Admins
  only.
- Single sign-on: one OIDC provider; no SAML, SCIM, group-to-team sync,
  sign-in-only-with-SSO enforcement, provider-initiated sign-in or back-channel
  logout. Signing out of the gateway does not sign out of the identity
  provider, and a role changed at the provider takes effect at the user's next
  sign-in. There is no unlink. The attempt's cookie is stateless (encrypted,
  10 minutes): replaying a callback relies on the provider accepting an
  authorization code only once, as the standard requires. The
  provider's groups claim can be missing (Entra ID overage), in which case
  the role stays as it is.
- A backup restore is manual, and a configuration import never deletes. There
  is no online migration between SQLite and Postgres (see Using PostgreSQL).
- On Postgres the console and `ultrafast backup` do not back up: use `pg_dump`.
- The Responses API is stateless: no `store`, `previous_response_id`,
  conversations, background mode, built-in tools or retrieval of a response.
- Images and audio are served by OpenAI, Azure and OpenAI-compatible providers
  only; there are no image edits or variations, no streamed image or
  transcription events, and no realtime audio. An image answer is buffered whole
  (up to 128 MiB, a few hundred MiB of memory while it is converted), and an
  audio upload is held in memory while it arrives and while the call runs (up
  to `UF_MAX_AUDIO_BYTES` each; at most `UF_MAX_CONCURRENT_UPLOADS` are
  received at once, the calls themselves are not bounded by it). A generated image or a transcription
  that was sent and did not answer in time is never repeated, so it fails with
  a 504 rather than being tried on a fallback.
- Structured outputs: `strict`, `name` and `description` of a JSON schema reach
  OpenAI and Azure only (Anthropic and Gemini enforce the schema without them),
  and `json_object` on Anthropic is the schema `{"type":"object"}`, which the
  model may satisfy with `{}`. A schema a provider does not accept is that
  provider's 400.
- A feature a provider lacks (`reasoning_effort`, `tool_choice` on a model that
  has no tools, ...) is a 400 when the first target a route tries lacks it: the
  gateway does not move on to a fallback that has it.
- Prompt templates: a version never changes, and there is no API to delete one
  version (the database does not block a raw delete of a row, and the
  numbering is checked in code); a template is deleted with all its versions.
  Reading an older version the first time takes a database read (then it is
  kept, 256 at a time). Any caller may use any template by name.
- Gemini thought signatures are not carried: no other format has them. Every
  earlier tool call sent to Gemini carries Google's documented placeholder
  signature (`skip_thought_signature_validator`), which Gemini 3 models need
  to accept the history.
- A tool result's `is_error` flag (Anthropic) is not carried; its text is kept.
- `function.strict` reaches OpenAI and Azure only.
- The image is linux/amd64; the crates are not on crates.io.

## Development

Building from source needs Rust 1.94+, Node 22 and pnpm 11. Release builds
embed `ui/dist`, so build the console first:

```bash
pnpm --dir ui install --frozen-lockfile
pnpm --dir ui build && cargo build --release -p ultrafast-gateway   # target/release/ultrafast
docker build -t ultrafast .                                         # or the image
```

A plain `cargo build` works without Node: `/` then says the console was not
built, while `/api`, `/v1`, `/health` and `/metrics` work.

```bash
cargo run -p ultrafast-gateway -- serve --port 3001 --insecure-cookies   # your own port
pnpm --dir ui dev                                                        # UF_DEV_GATEWAY=http://127.0.0.1:3002 for another port
```

Tests:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
pnpm --dir ui lint && pnpm --dir ui typecheck && pnpm --dir ui test
pnpm --dir ui exec playwright install chromium
pnpm --dir ui build && cargo build --release -p ultrafast-gateway && pnpm --dir ui test:e2e
```

CI (`.github/workflows/`): `ci.yml` (lint, tests, OpenAPI is current, image, secret scan),
`clients.yml`, and `release.yml` (a tag `v*` that names the version in
`Cargo.toml` makes a draft GitHub release with binaries for Linux x86_64 and
aarch64, macOS x86_64 and arm64 and Windows x64, and pushes
`ghcr.io/techgopal/ultrafast-ai-gateway`).

Layout:

| Path | Contents |
| --- | --- |
| `crates/gateway` | The binary: API, `/v1` proxy, store, migrations |
| `crates/translate` | Request and response translation shared with the clients |
| `crates/client`, `crates/client-py`, `crates/client-wasm`, `clients/ts` | Rust, Python, WebAssembly core and TypeScript clients |
| `ui/` | React console |
| `openapi/admin.json` | Generated admin API description |
| `docs/` | [`CONVENTIONS.md`](docs/CONVENTIONS.md), design spec, plans, brand |

Contributors: read [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`docs/CONVENTIONS.md`](docs/CONVENTIONS.md) first. After
changing an admin route, regenerate the spec with
`cargo run -p ultrafast-gateway -- openapi > openapi/admin.json`. Security
reports: see [`SECURITY.md`](SECURITY.md).

## License

MIT, see [`LICENSE`](LICENSE).
