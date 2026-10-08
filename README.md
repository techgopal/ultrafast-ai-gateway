<p align="center"><img src="docs/brand/banner.png" alt="ultrafast — one small binary between your apps and every model" width="100%"></p>

# Ultrafast Gateway

[![CI](https://img.shields.io/github/actions/workflow/status/techgopal/ultrafast-ai-gateway/ci.yml?branch=main)](https://github.com/techgopal/ultrafast-ai-gateway/actions)
[![Rust](https://img.shields.io/badge/Rust-1.94+-orange.svg)](rust-toolchain.toml)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Ultrafast is one small Rust binary that sits between your apps and your LLM
providers. It speaks the OpenAI and Anthropic APIs, adds an admin API, and
serves a built-in web console, with SQLite for storage and nothing else to
run. It is for small teams who host it themselves. This is v2; the v1 code is
tagged `v1-final`.

## Screenshots

The console is built into the binary. Overview, routing and the playground;
more (dark theme, logs, models) are in [`docs/images/`](docs/images/).

![The Overview page: counts, requests, errors, tokens and spend, top models and keys](docs/images/console-overview.png)

![The routing editor: weighted primary targets and an ordered fallback](docs/images/console-routing.png)

![The playground: a chat with a route, with tokens and cost](docs/images/console-playground.png)

## Features

- **Endpoints.** `/v1/chat/completions`, `/v1/messages` (Anthropic format),
  `/v1/embeddings`, `/v1/models`; streaming on both chat formats, with tool
  calling and image input (vision) on both, over any provider kind.
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
- **Console.** Overview, logs, playground, providers, models, routing, keys,
  users, teams, limits, settings; light and dark themes, phone layout. Compiled
  into the binary; no Node at runtime.
- **Metrics.** Prometheus at `/metrics` (opt in, token protected).
- **Tracing.** Every `/v1` call as an OpenTelemetry trace over OTLP/HTTP (JSON),
  with a span per provider attempt; opt in.
- **Alerts.** Budget, error-rate and circuit-breaker rules, delivered as signed
  webhooks (generic or Slack-compatible); managed by admins in the console.
- **Operations.** Online backup, configuration export and import, a CLI for
  setup, an OpenAPI description of the admin API.
- **Clients.** Rust, Python and TypeScript, sharing one Rust core.

Not yet (phase 2): guardrails, MCP tools, single sign-on, alerts by email, an
admin SDK, Postgres, the Responses API,
image or audio output, and `response_format` / structured outputs.

## Quickstart

### 1. Get it

**Docker** (linux/amd64; listens on 3000, keeps its data in `/var/lib/ultrafast`):

```bash
# The first admin, in a file of its own: not on the command line, where the
# password would be in the shell history and in `docker inspect`.
printf 'UF_ADMIN_EMAIL=you@example.com\nUF_ADMIN_PASSWORD=a long password\n' > admin.env
chmod 600 admin.env
docker run -d --name ultrafast -p 3000:3000 -v ultrafast-data:/var/lib/ultrafast \
  --env-file admin.env ghcr.io/techgopal/ultrafast-ai-gateway:2.0.0-beta.2
```

**Or a binary** from the [latest release](https://github.com/techgopal/ultrafast-ai-gateway/releases)
(Linux x86_64/aarch64, macOS Intel/Apple Silicon, Windows x64). For Linux x86_64:

```bash
V=2.0.0-beta.2; T=x86_64-unknown-linux-musl
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
| `UF_DATA_DIR` | `--data-dir` | `./data` (`/var/lib/ultrafast` in the image) | Directory of the database (`gateway.db`) and `master.key`. Any subcommand. |
| `UF_MASTER_KEY` | `--master-key` | generated into `master.key` | 64 hex characters; encrypts provider credentials. Any subcommand. |
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
| `UF_PROVIDER_API_KEY` | `--api-key` | unset | `provider add` only: the provider's API key. |
| `RUST_LOG` | none | `info` | Log filter. A gateway that starts with no user logs its one-time setup code at `info` under the target `ultrafast::setup`: when you lower the level, keep it, as in `RUST_LOG=warn,ultrafast::setup=info`. |

For the dev tooling only: `UF_DEV_GATEWAY` (where `pnpm --dir ui dev` proxies
to) and `UF_E2E_BINARY` (the binary the browser tests run).

Subcommands: `serve`, `provider add`, `model add`, `key create`,
`backup <path>`, `config export|import <file>`, `openapi`.

## Using the gateway

- **Models.** Call `provider/model` (for example `anthropic/claude-sonnet-5`)
  or the name of a route. `GET /v1/models` lists what the key may call.
- **Access.** A model is callable when it is enabled, granted to the caller
  (everyone, or a team or user they belong to), and, if the key has an
  allowlist, on that list. Routes follow the same idea. A call that fails these
  rules is refused.
- **Tags.** Send `x-uf-tags: {"job":"nightly","env":"dev"}` on chat, messages
  or embeddings calls (at most 20 tags; names `A-Z a-z 0-9 _ . -`; names and
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

## Console

Served at `/`. What each role sees is decided by the API.

| Page | Admin | Team lead | Member |
| --- | --- | --- | --- |
| Overview, Logs | all | their teams' and own | own |
| Playground | yes | yes | yes |
| Providers | manage | view | view |
| Models, Routing | manage | see what they may use | see what they may use |
| Virtual keys | any user's | own and their teams' members' | own |
| Users | invite, role, status, delete | their teams' members | themselves |
| Teams | create, delete, leads | rename, add members | see own teams |
| Budgets and limits | set | read their teams' | read what applies to them |
| Settings (retention, sign-in, backup, config, audit log) | yes | no | no |
| Account (name, password, access tokens) | yes | yes | yes |

The playground sends images (5 MB each, 9 MiB per request including the
history), tools as JSON with a tool choice, and shows the model's tool calls and
the results you send back.

Guardrails and MCP tools appear in the navigation as coming.

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

## Operations

**Backup.** A consistent copy of the database while it runs: Settings, Backup;
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
`uf_budget_blocked_total`, `uf_circuit_open{provider,model}`,
`uf_log_records_dropped_total`, `uf_log_write_failures_total`,
`uf_otel_spans_exported_total`, `uf_otel_spans_dropped_total`,
`uf_otel_export_failures_total`, `uf_alert_deliveries_total{result}`
(`ok`, `failed`, `dropped`). No label names a
key, user, team or prompt. `GET /health` answers `{"status":"ok"}`.

**Tracing.** Set `UF_OTEL_ENDPOINT` and every `/v1` call (chat, messages,
embeddings) and every playground call becomes a trace, exported in the
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
- *Spans.* One server span per call, named `uf.chat`, `uf.messages`,
  `uf.embeddings` or `uf.playground`, with `uf.endpoint`, `uf.requested` (the
  model or route the caller asked for, cut at 256 bytes), `http.response.status_code`, `uf.stream`, `uf.cached`,
  `uf.estimated`, `uf.key_id` / `uf.user_id` / `uf.team_id` when known,
  `gen_ai.usage.input_tokens` / `gen_ai.usage.output_tokens` when counted, and
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
`toolCalls` / `toolCallId`; see each README. Streams yield tool-call events.

## Known limits

- Rate limits, budgets, the response cache and routing health live in the
  memory of one process: they start empty (budgets are rebuilt from the logs)
  and are not shared between processes. One gateway per database.
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
  history (events) is kept until the database is deleted; it is not pruned by
  log retention.
- Single sign-on: one OIDC provider; no SAML, SCIM, group-to-team sync,
  sign-in-only-with-SSO enforcement, provider-initiated sign-in or back-channel
  logout. Signing out of the gateway does not sign out of the identity
  provider, and a role changed at the provider takes effect at the user's next
  sign-in. There is no unlink. The attempt's cookie is stateless (encrypted,
  10 minutes): replaying a callback relies on the provider accepting an
  authorization code only once, as the standard requires. The
  provider's groups claim can be missing (Entra ID overage), in which case
  the role stays as it is.
- A backup restore is manual, and a configuration import never deletes.
- No Responses API, image or audio output, or `response_format` / structured
  outputs yet (phase 2). SQLite only.
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
