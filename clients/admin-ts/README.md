# @ultrafast/admin

TypeScript SDK for the Ultrafast gateway's admin API (`/api`): manage providers,
models, routes, keys, users, teams, limits, budgets, alerts and guardrails from
code and CI. The types and the typed client are generated from
`openapi/admin.json`; this package adds a base URL and token, a timeout and a
typed error. It is not published to npm: build it from source.

```sh
pnpm --dir clients/admin-ts install && pnpm --dir clients/admin-ts build
# then depend on the folder, e.g. "@ultrafast/admin": "file:../ultrafast-ai-gateway/clients/admin-ts"
```

Create an access token under Account, Access tokens (it starts with `uf-at-`).
A token acts as its user and needs no CSRF header. Keep it out of logs and source.

```ts
import { createAdminClient, AdminApiError } from "@ultrafast/admin";

const api = createAdminClient({
  baseUrl: "https://gateway.example.com",
  token: process.env.UF_ADMIN_TOKEN!,
  // timeoutMs: 30000, fetch: customFetch   (both optional)
});
```

`api.raw` is an [openapi-fetch](https://openapi-ts.dev/openapi-fetch/) client typed
over every operation: paths, parameters, bodies and answers are checked. It
resolves to `{ data, error, response }`. `api.call(...)` wraps such a call: it
returns `data`, or throws `AdminApiError { status, code, message, fields }`.

```ts
// List providers
const { providers } = await api.call(api.raw.GET("/api/providers"));

// Add a provider
const provider = await api.call(
  api.raw.POST("/api/providers", {
    body: { name: "openai", kind: "openai", base_url: "https://api.openai.com/v1", api_key: process.env.OPENAI_KEY },
  }),
);

// Add a model (disabled and granted to nobody until you change that)
const model = await api.call(
  api.raw.POST("/api/models", { body: { provider_id: provider.id, name: "gpt-4o-mini" } }),
);
await api.call(
  api.raw.PATCH("/api/models/{id}", { params: { path: { id: model.id } }, body: { enabled: true } }),
);

// Create a key: the secret is in this answer only
const created = await api.call(api.raw.POST("/api/keys", { body: { name: "ci" } }));
console.log(created.secret);

// Set a budget: 50 dollars a month for the whole gateway, alert only
await api.call(
  api.raw.PUT("/api/budgets", {
    body: { scope: "gateway", amount_micros: 50_000_000, period: "monthly", action: "alert" },
  }),
);

// Create an alert rule that tells a channel when 80 % of a budget is spent
const { channel } = await api.call(
  api.raw.POST("/api/alerts/channels", {
    body: { name: "ops", kind: "slack", url: "https://hooks.slack.com/services/..." },
  }),
);
await api.call(
  api.raw.POST("/api/alerts/rules", {
    body: { name: "budget-80", kind: "budget", params: { budget_id: null, percent: 80 }, channel_ids: [channel.id] },
  }),
);
```

`params` of an alert rule is an open object; see the description of
`CreateRuleRequest` for its shape by `kind`.

## Errors

```ts
try {
  await api.call(api.raw.POST("/api/providers", { body: { name: "x", kind: "nope", base_url: "http://h" } }));
} catch (error) {
  if (error instanceof AdminApiError) {
    error.status; // 422
    error.code;   // "validation_failed"
    error.fields; // { kind: "kind must be openai, anthropic, gemini or azure" }
  }
}
```

| Status | Code (examples) | Meaning |
| --- | --- | --- |
| 401 | `unauthenticated` | no token, or one that is unknown, revoked or expired |
| 403 | `forbidden` | the token's user may not do this |
| 404 | `not_found` | no such object |
| 409 | `provider_exists`, ... | a name is taken or the state forbids it |
| 422 | `validation_failed` | `fields` holds a message for each field |
| 0 | `timeout`, `network_error` | no answer came |

An answer that is not the gateway's error shape (a proxy's 502, say) is
`code: "http_<status>"` with its text as the message. The token is never put in
an error.

## Downloads

Two operations do not answer JSON objects:

```ts
const file = await api.exportConfig();      // the configuration file, parsed
const sqlite = await api.downloadBackup();  // Uint8Array: a SQLite file
// or by hand: api.raw.GET("/api/backup", { parseAs: "arrayBuffer" })
```

A call times out after `timeoutMs` (30 s) from the request until its body is read,
and throws `AdminApiError` with code `timeout`. A large backup can need longer:
`api.downloadBackup({ timeoutMs: 600_000 })`; `exportConfig` takes the same
option. A signal you give to `raw` aborts the call, and what you aborted with is
rethrown as given. (Used on `raw` alone, a body that stalls after its headers
rejects with the platform's `TimeoutError`; `call` and the helpers map it.)

## Regenerating

`src/schema.d.ts` is generated and committed (openapi-typescript 7.13.0, the
version the console uses):

```sh
pnpm --dir clients/admin-ts gen          # regenerate
pnpm --dir clients/admin-ts check:gen    # fails when it differs from openapi/admin.json
```

## Tests

`pnpm test` starts the real gateway on a free port with a temporary data
directory under `~/.cache`: `UF_E2E_BINARY` if set, else it runs
`cargo build -p ultrafast-gateway` and starts `debug/ultrafast` in the target
directory `cargo metadata` reports, so `CARGO_TARGET_DIR` is honoured. Tests
that need no gateway never build. It signs in once to mint a token, then uses
only the token.
