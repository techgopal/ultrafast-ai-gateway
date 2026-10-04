<p align="center"><img src="docs/brand/banner.png" alt="ultrafast — one small binary between your apps and every model" width="100%"></p>

# Ultrafast Gateway 🚀

> This is Ultrafast v2. The v1 code is tagged `v1-final`.
> Design: `docs/superpowers/specs/2026-09-28-gateway-v2-design.md`.

## v2 quickstart

```bash
cargo build --release -p ultrafast-gateway
export UF_DATA_DIR=./data

# Add a provider. Use kind "openai" for any OpenAI-compatible API.
UF_PROVIDER_API_KEY=sk-... ./target/release/ultrafast provider add \
  --name openai --kind openai --base-url https://api.openai.com/v1
UF_PROVIDER_API_KEY=sk-ant-... ./target/release/ultrafast provider add \
  --name anthropic --kind anthropic --base-url https://api.anthropic.com

# Put models in the catalog, enable them and let everyone call them.
# Without this a provider's models answer 404/403: nothing is callable until
# it is enabled and granted.
./target/release/ultrafast model add --provider openai --model gpt-4o --enable --everyone

# Create a key for your app. It is printed once.
./target/release/ultrafast key create --name my-app

./target/release/ultrafast serve
```

Then call it with the key (see below). Models can also be synced from a
provider, enabled and granted to teams or users in the console (Models page).

**Upgrading from alpha.1.** Existing providers keep working only after an
admin syncs, enables and grants their models: from the console (Models page)
or with `ultrafast model add --provider NAME --model ID --enable --everyone`.
Until then calls to `NAME/MODEL` are refused.

Changes made through the admin API under `/api` apply at once. Changes made
with the CLI reach a running gateway within 30 seconds.

Call it with any OpenAI SDK by setting the base URL to
`http://127.0.0.1:3000/v1` and the model to `provider/model`, for example
`anthropic/claude-sonnet-5`, or to the name of a route.

### Install a release

A tag `v*` makes a **draft** GitHub release (a person publishes it) with the
binary of each of these targets, the console compiled in: Linux x86_64 and
aarch64 (static, musl), macOS x86_64 and arm64, Windows x64. Each archive has
a checksum, and `SHA256SUMS` lists them all:

```bash
curl -LO https://github.com/<owner>/<repo>/releases/download/<tag>/ultrafast-<tag>-x86_64-unknown-linux-musl.tar.gz
curl -LO https://github.com/<owner>/<repo>/releases/download/<tag>/SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS
tar -xzf ultrafast-<tag>-x86_64-unknown-linux-musl.tar.gz
./ultrafast-<tag>-x86_64-unknown-linux-musl/ultrafast serve
```

The image is on `ghcr.io/<owner>/ultrafast-ai-gateway:<version>` (and `latest`
for a version that is not a pre-release), for linux/amd64:

```bash
docker run -p 3000:3000 -v ultrafast-data:/var/lib/ultrafast ghcr.io/<owner>/ultrafast-ai-gateway:<version>
```

The image is public as soon as the tag is pushed, while the GitHub release is
still a draft: `ghcr.io/<owner>/ultrafast-ai-gateway:<version>` can be pulled
before a person has reviewed and published the release.

The release workflow (`.github/workflows/release.yml`) builds the console
first and the binaries after it, publishes nothing without a tag, and does not
publish the crates to crates.io yet. Run it by hand (Actions, Release, Run
workflow) to build the archives as artifacts of the run, with nothing
published. The tag must name the version in `Cargo.toml` (`v2.0.0-alpha.2` for
`2.0.0-alpha.2`), or the run stops in its first job, before anything is built.

### Console

The web console is served by the same binary at `/`: a static app compiled
into the binary, with no Node process and no request to any other host at
runtime.

What is in it: setting up the first admin, signing in, accepting an invite;
an overview with a getting-started guide and the last 30 days of requests,
errors, tokens and spend; request logs (a list with filters, tags included, and a detail of
each call, with the targets tried; no prompt or answer is stored); providers
(OpenAI-compatible, Anthropic, Gemini and Azure OpenAI: add, edit, sync
models, delete); a playground; models (enable, who may call each, add by name, set prices);
routing (routes with fallbacks, a response cache, who may use each, and the
health of their targets); virtual keys (create, shown once, limit to chosen
models and routes, tags, revoke, filter); users (invite, role, status, teams, new
invite link, delete); teams (create, rename, delete, members added by email,
and leads); budgets and limits (admins set them; everyone sees what applies
to them); your account (name, password, access tokens); the audit log, for
admins, under Settings; and a Settings page for admins (retention, sign-in
settings, backup, configuration export and import, the audit log).
What a user sees depends on their role, and the API decides. Light and dark
themes, following the device until one is chosen, and a layout for phones.

Not yet: guardrails and MCP tools (shown as coming in the navigation).

#### Playground

Observe, Playground: chat with a model or a route from the console, as the
signed-in user. The picker offers what the gateway lists for you (models you
may call, routes you may use); there is a system prompt, max tokens,
temperature, top P and stop sequences, and the answer streams in, with Stop.
It shows the tokens and the cost of the call (from the price of the model; a
route is priced by the model that answered), and **Copy as curl** gives the
same call for `/v1` with `Authorization: Bearer <your key>` to fill in.

The playground is not a side door. The call goes through the same pipeline as
`/v1/chat/completions` (access, rate limits, budgets, the response cache,
routing, logging) as a key owned by you, with no team and no allowlist would:
a model you may not call is refused as it would be for your key, and the call
counts against your limits and budgets and those of your teams. It is logged
like any call, to you, with no key and the endpoint `playground`. Nothing of
the conversation is saved: it is in the memory of the page. The API is
`POST /api/playground/chat` (a signed-in user; it answers as
`/v1/chat/completions` does).

#### Tags on keys and calls

Tags are names and values that tell calls apart in the logs and the usage
reports: a team, a job, an environment.

- **On a call.** Send `x-uf-tags` on a `/v1` call (chat, messages or
  embeddings): a compact JSON object of strings, for example
  `x-uf-tags: {"job":"nightly","env":"dev"}`. The gateway-owned clients send
  it for you (`tags` option). The header is never forwarded to a provider.
- **On a key.** Set `tags` when creating a key (`POST /api/keys`); whoever
  creates the key may. Only an admin changes them afterwards, with
  `PATCH /api/keys/{id}` and `{"tags": {...}}` (`{}` removes them): a key's
  tags win over a call's, so they are the admin's labels, and an owner or a
  team lead cannot take them off. The console has a Tags editor on Create key
  and an Edit tags action on each key, for admins.
- **Precedence.** A call is recorded with its own tags overlaid by its key's,
  and the key wins on the same name: a caller cannot relabel what an admin
  fixed. A change to a key's tags applies to calls made after it.
- **Limits.** At most 20 tags; a name of `A-Z a-z 0-9 _ . -` (no colon); names and
  values of 1 to 64 characters; the header at most 1 KiB (1024 bytes). A
  header that breaks a rule is refused with 400 `invalid_request_error`, "The
  x-uf-tags header is not valid: ...", in the shape of the endpoint, and the
  call is not made. A key's tags and a call's can merge to 40 stored entries:
  the limit of 20 applies to each, not to the union.
- **Reading them.** `GET /api/logs` and `GET /api/logs/{id}` show `tags`
  (an object, empty when none). `GET /api/logs?tag=env:prod` keeps the calls
  with that tag; repeat `tag` to require several (all must match). The name
  ends at the first colon, which a name cannot contain, so `tag=a:b:c` is the
  name `a` with the value `b:c`. The console's Tag filter takes one tag; the
  API takes several. `GET /api/usage?group=tag:team` sums by the value
  of the tag `team`; calls without it are `(none)`. Both follow the same scope
  as the rest of the logs and usage: you see your own calls, a team lead their
  team's, an admin all. The filter is applied to the rows in that scope and
  needs no index of its own.

#### Prices, cache, limits, budgets and retention

All of these are set by an admin in the console, or with the admin API under
`/api` (`openapi/admin.json` lists every route).

- **Prices.** Models page, Price on a model: dollars per one million input
  tokens and per one million output tokens. A model without a price is
  logged without a cost (and shown as Unpriced), so spend and budgets are a
  lower bound while any model in use has none. Prices are public (not secret).
- **Route cache.** Routing page, on a route: Cache answers, how long an
  answer is kept (TTL, 1 to 86 400 seconds) and whose calls share an answer
  (team, key or user). Streams and requests with a temperature above 0.5 are
  never cached. A cached answer costs nothing and calls no provider.
- **Limits.** Budgets and limits page, Set limit: requests per minute, tokens
  per minute and concurrent requests, for the gateway, a team, a user or a
  key. The strictest applies; a refused call gets 429 with `Retry-After`.
- **Budgets.** Same page, Set budget: an amount per UTC day, week (from
  Monday) or month for the gateway, a team, a user or a key. `Block` refuses
  calls once the amount is spent (429, `budget_exceeded`); `Alert` allows them
  and writes one audit entry per period.
- **Who is counted.** A call counts against the limits and budgets of its key,
  the key's owner, the gateway and the team of the key; a key without a team
  counts against all of its owner's teams.
- **Retention.** Settings page: request logs older than the number of days
  (1 to 3 650, 30 at first) are deleted.

#### Settings: sign-in, configuration export and import

Settings, Sign-in: how long a session lives (1 to 720 hours, 12 at first; it
applies to sign-ins from then on, and sessions that exist keep theirs). The
trusted proxies (the `--trusted-proxy` flag) and the sign-in limits (5 failed
attempts for one email and 20 for one address in 15 minutes) are shown and
cannot be changed there.

Settings, Configuration: **export** writes the setup as one JSON file
(`{"format": "ultrafast-config", "version": 1, ...}`): providers (name, kind,
base URL, API version, **no credential**), models (enabled, prices, who may
call them by team name and user email), routes (all settings, targets as
`provider/model`, teams by name), teams (names), limits and budgets for the
gateway, teams and users (those of keys are left out) and the settings. It
holds no key, token, password, session, log or audit row; users are not in it.
**Import** reads such a file: choose it, read the report of what it would do
(a dry run, nothing is written), then Apply. A file that names a team, user,
provider or model that does not exist, or a value that is not valid, is
refused whole, with where and why, and writes nothing. What is missing is
created and what exists, by name, is updated; **nothing is deleted** (a prune
is for later). Providers it creates have no credential until an admin sets
one, and the report says so. The import is one transaction, is audited, and
reaches `/v1` at once. From the command line, with no master key:

```bash
ultrafast --data-dir ./data config export ./config.json
ultrafast --data-dir ./other config import ./config.json --dry-run
ultrafast --data-dir ./other config import ./config.json
```

The API is `GET /api/config/export` and `POST /api/config/import?dry_run=true`
(a dry run unless `dry_run=false`; 422 with the report when the file has
errors), for admins.

#### Backup and restore

A backup is a consistent copy of the whole database, of one moment, taken
while the gateway runs (SQLite's online backup; it blocks no request). Take
one from the console (Settings, Backup), from the admin API
(`GET /api/backup`, admins only, audited), or from the command line:

```bash
ultrafast --data-dir ./data backup ./backups/ultrafast-2026-10-04.db
```

The file holds everything in the database (users and their password hashes,
sessions, request logs, the audit log, provider credentials as they are
stored) **except the master key**. The credentials are encrypted with that
key, so a backup is useless without it: keep the master key (`master.key` in
the data directory, or the `UF_MASTER_KEY` you set) safe, apart from the
backups. The backup file is readable by its owner only; treat it like the
database.

To restore, stop the gateway cleanly (so that it has checkpointed its
write-ahead log) and put the backup in its place. There is no
API for this, on purpose:

```bash
# 1. stop the gateway
# 2. keep what is there, in case: the database together with its -wal and
#    -shm files (they belong to it; after a clean stop they may be absent)
mv data/gateway.db data/gateway.db.before
[ -e data/gateway.db-wal ] && mv data/gateway.db-wal data/gateway.db.before-wal
[ -e data/gateway.db-shm ] && mv data/gateway.db-shm data/gateway.db.before-shm
# 3. put the backup in its place; the master key stays as it is
cp backups/ultrafast-2026-10-04.db data/gateway.db
chmod 600 data/gateway.db
# 4. start the gateway: it runs the migrations it needs and serves
```

The gateway must have the master key of the gateway the backup came from. To
move only the setup (providers without credentials, models, routes, limits)
and not the data, use the configuration export and import instead.

### Metrics

`GET /metrics` serves Prometheus metrics (text format 0.0.4). It exists only
when a token is set: start the gateway with `UF_METRICS_TOKEN` (or
`--metrics-token`; the variable is safer, a flag shows in the process list)
and scrape with `Authorization: Bearer <token>`. Without a token `/metrics`
is not served (the path is answered like any other the console does not know);
a missing or wrong token answers 401.

```yaml
scrape_configs:
  - job_name: ultrafast
    authorization: { credentials_file: /etc/prometheus/ultrafast-token }
    static_configs: [{ targets: ["127.0.0.1:3000"] }]
```

Metrics: `uf_requests_total{endpoint,status_class}` (endpoint `chat`,
`messages`, `embeddings`, `playground`; class `2xx`, `4xx`, `5xx`, `499` for a caller that
went away, `other`), `uf_tokens_total{direction}` (answers from the cache are
not counted), `uf_cost_micros_total`, `uf_upstream_duration_seconds{provider}`
(histogram), `uf_log_records_dropped_total`, `uf_log_write_failures_total`,
`uf_cache_hits_total`, `uf_cache_misses_total`,
`uf_rate_limited_total{limit}` (`requests_per_minute`, `tokens_per_minute`,
`concurrent`), `uf_budget_blocked_total` and `uf_circuit_open{provider,model}`
(1 while a breaker is open). `uf_requests_total` counts authenticated chat, messages and embeddings calls
only (not `/v1/models`, not calls refused before authentication);
`uf_cost_micros_total` is the cost as priced by the log writer. Counters start at zero when the gateway starts.
No label names a key, user, team or prompt.

Build: the console is compiled into the binary from `ui/dist`, so build the
console first, then the gateway:

```bash
pnpm --dir ui install --frozen-lockfile && pnpm --dir ui build && cargo build --release -p ultrafast-gateway
```

This needs Node 22 and pnpm. `cargo build` alone never runs Node and works
without it: the binary then serves a page at `/` that says the console was not
built, and `/api`, `/v1`, `/health` and `/metrics` work as usual. The Docker image builds
both.

Develop: run a gateway of your own on a port that nothing else uses (here
3001; never the port of a gateway that is in use) with `--insecure-cookies`
(plain HTTP on your own machine only), and the Vite dev server next to it. The
dev server passes `/api`, `/v1` and `/health` on to `http://127.0.0.1:3001`,
or to the address in `UF_DEV_GATEWAY` when you use another port:

```bash
cargo run -p ultrafast-gateway -- serve --port 3001 --insecure-cookies
pnpm --dir ui dev
# another port: UF_DEV_GATEWAY=http://127.0.0.1:3002 pnpm --dir ui dev
```

Test: `pnpm --dir ui test` runs the unit and component tests. The browser
tests run the release binary built as above (or the one named by
`UF_E2E_BINARY`), each test with its own gateway on a free port and a new
temporary data directory, in Chromium at desktop and phone sizes:

```bash
pnpm --dir ui exec playwright install chromium
pnpm --dir ui test:e2e
```

Known limits:

- The console needs HTTPS, except on localhost: the session cookie is
  `Secure`, so over plain HTTP at any other address the browser drops it and
  the sign-in page says so. On a trusted network, start the gateway with
  `--insecure-cookies` instead.
- Rate limits, budgets, the response cache and the health of routing targets
  live in the memory of one gateway process: they start empty when it starts
  (budgets are counted again from the request logs) and are not shared
  between processes.
- A `block` budget can be overshot: spend is counted when the log writer has
  priced a call (batches of about a second), so calls already running, and
  concurrent ones, are not stopped, and a long call is charged when it ends.
- A stream whose caller left, or that failed after content was sent, is
  charged an estimate (the input at four characters to a token, and the
  streamed characters / 4 for the output), marked Estimated in the logs.
- Any key created or revoked, and any change to what a cached answer depends
  on (teams and users created or deleted, routes with their targets and cache
  settings, providers, models, grants), clears the whole response cache.
  There is no single-flight: calls that miss together all go to the provider.
- Request logs keep metadata only: the prompt and the answer are not stored.
- Only admins set limits and budgets. Team leads see those of their team, and
  members see the gateway's and their teams' without what was spent (their
  own user's and keys' with it); nobody sees another person's spend.
- A budget alert is an audit entry, not an email or a webhook.
- Logs of a deleted user or team stay, with no owner; the id of a deleted
  user or team may be given out again and does not inherit them.
- Behind a reverse proxy, start the gateway with `--trusted-proxy CIDR` (or
  `UF_TRUSTED_PROXIES`) so sign-in limiting counts the client's address, not
  the proxy's. The proxy must set or overwrite `CF-Connecting-IP` and
  `X-Forwarded-For` itself and never pass on what the client sent; the
  gateway believes those headers from the listed networks. Never list a
  network that clients can reach directly. Without the flag, 20 failed
  sign-ins from anyone behind a proxy block sign-in for everyone for 15
  minutes.

What works today: `/v1/chat/completions`, `/v1/messages` (Anthropic format),
`/v1/embeddings` and `/v1/models`, streaming, OpenAI-compatible, Anthropic,
Gemini and Azure OpenAI providers, a model catalog with grants and prices,
routes with fallbacks, circuit breakers and a response cache, request logs
with retention, usage and spend reports, rate limits and budgets, Prometheus
metrics, and the console with its playground. Not yet: tools, images and
guardrails.

[![Rust](https://img.shields.io/badge/Rust-1.94+-orange.svg)](LICENSE)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build Status](https://img.shields.io/github/actions/workflow/status/techgopal/ultrafast-ai-gateway/ci.yml?branch=main)](https://github.com/techgopal/ultrafast-ai-gateway/actions)


## Clients

Rust, Python and TypeScript clients for the gateway (or for a provider
directly). All three share one Rust core, `ultrafast-translate`: request
building and response parsing are the code the gateway itself uses, and error
classification and stream decoding are shared by the three clients (the
gateway does not use those two). The TypeScript client runs the core as
WebAssembly. One set of fixtures (`clients/fixtures/`) is run by all three
test suites, so the behaviour those fixtures cover is checked to be the same
in all three. They do not retry, route, cache or break circuits: an error says
whether trying again could help (`retryable`) and how long to wait
(`retry_after`). Every example below uses a key made with `ultrafast key
create` and a model written `provider/model`, or the name of a route.

Rust ([`crates/client`](crates/client/README.md)):

```rust
use ultrafast_client::{ChatRequest, Client, Target};

let client = Client::new(Target::gateway("http://127.0.0.1:3000", key));
let reply = client
    .chat(ChatRequest::new("anthropic/claude-sonnet-5").user("Say hi."))
    .await?;
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
const reply = await client.chat({
  model: "anthropic/claude-sonnet-5",
  messages: [{ role: "user", content: "Say hi." }],
});
console.log(reply.content);
```

Each client also streams (`chat_stream` / `chatStream`) and makes
embeddings; see its README. Wheels and an npm package are configured but not
published yet: build from source as each README says. Text content only, as
in the gateway; the optional `tags` are sent as `x-uf-tags`, which the gateway
records (see Tags on keys and calls).

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.


---

**Made with ❤️ by the Ultrafast AI Team** 