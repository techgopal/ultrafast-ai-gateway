# @ultrafast/client (TypeScript)

Client for the Ultrafast gateway, or for a provider directly. Request building,
response parsing and stream decoding are the Rust code the Rust and Python
clients use (the translation part is the gateway's own), compiled to
WebAssembly (`crates/client-wasm`); the
network is the runtime's own `fetch`. No runtime dependencies, no Node-specific
APIs in `src/`: it runs in Node 20+, Bun, Deno and browsers with no setup, and on
edge runtimes that forbid compiling WebAssembly at run time with one extra call
(see below).

```ts
import { Client, gateway } from "@ultrafast/client";

const client = new Client(gateway({ baseUrl: "http://localhost:3000", key }));

const reply = await client.chat({ model: "gpt-4o", messages: [{ role: "user", content: "hi" }], tags: { team: "a" } });
console.log(reply.content, reply.usage);

for await (const event of client.chatStream({ model: "gpt-4o", messages: [{ role: "user", content: "hi" }] })) {
  if (event.type === "delta") console.log(event.text);
}

const { vectors } = await client.embed({ model: "text-embedding-3-small", input: ["a", "b"] });
```

## Targets

| Constructor | Notes |
| --- | --- |
| `gateway({ baseUrl, key })` | OpenAI format under `/v1`; a trailing `/v1` is accepted. The only target that is sent `tags` (`x-uf-tags`, at most 1 KiB of JSON). |
| `openai({ key, baseUrl? })` | `baseUrl` includes the version segment (default `https://api.openai.com/v1`). |
| `anthropic({ key, baseUrl? })` | |
| `gemini({ key, baseUrl? })` | |
| `azure({ endpoint, key, apiVersion? })` | The request's `model` is the deployment name. |
| `openaiCompatible({ baseUrl, key? })` | Groq, Mistral, OpenRouter, Ollama; `baseUrl` includes `/v1`. The key may be empty or left out (keyless Ollama): no Authorization header is sent. |

## Tools and images

```ts
const reply = await client.chat({
  model: "gpt-4o",
  messages: [
    { role: "user", content: [
      { type: "text", text: "What is in this picture?" },
      { type: "image", url: "data:image/png;base64,..." }, // or an http(s) URL
    ] },
  ],
  tools: [{ name: "weather", description: "Current weather", parameters: { type: "object" } }], // `strict?: boolean` goes to OpenAI and Azure only
  toolChoice: "auto", // "auto" | "none" | "required" | { name: "weather" }
  parallelToolCalls: true,
});
for (const call of reply.toolCalls) { /* call.id, call.name, call.arguments (JSON text) */ }
// the next turn: { role: "assistant", content: null, toolCalls: reply.toolCalls },
//                { role: "tool", toolCallId: reply.toolCalls[0].id, content: "..." }
```

`chatStream` yields `delta`, `tool_call_start` (`index`, `id`, `name`),
`tool_call_delta` (`index`, `arguments`) and `done`. A `tool` message without
`toolCallId`, or any other malformed message, throws `InvalidRequestError`
before a request is sent. A direct provider target takes the same request.

## Options

`new Client(target, { fetch?, timeoutMs?, maxResponseBytes? })`

- `fetch`: your own (tests, proxies, edge runtimes). Requests are sent with `redirect: "manual"`; a redirect is an error and is never followed (the credential would go with it). If you supply a `fetch`, do not make it follow redirects with credentials.
- `timeoutMs` (default 120000): for the whole of `chat`/`embed`, for the answer to start a stream, and for each silent stretch inside a stream. The client enforces it itself (an abort plus a race), so a `fetch` that ignores `AbortSignal` is still bounded.
- `maxResponseBytes` (default 32 MiB): the most a `chat`/`embed` answer or any error body may hold; a larger one is a `malformed` error. Streams are not capped in total.

## Browsers

The gateway sends no CORS headers, so a page can call it only from the same
origin or through a proxy you run. A key in browser code is public to anyone
who opens the page: use a key meant to be exposed (tight allowlist and limits),
or keep the key in your proxy and send none from the page.

## Errors

Every failure is an `UltrafastError` with `kind` (`auth | permission | not_found | invalid_request | rate_limited | upstream | network | timeout | malformed`), `status` (when there was an HTTP answer), `retryable` and `retryAfter` (seconds, when the server sent `Retry-After`). There are subclasses per kind (`RateLimitError`, `RequestTimeoutError`, ...). The client does not retry, route, cache or break circuits. The key is removed from every message and is not on `Target`, `Client` or the error when printed or serialised.

A stream yields its events in order, then at most one error: a provider error event, a broken connection, a silent stretch, or a close before the end (`malformed`, "the stream ended before it was complete") arrives after the text already received, never as a silent end. `chatStream` starts the request on the first `next()`; leaving the loop early cancels the request.

## Edge runtimes (Cloudflare Workers, Vercel Edge, strict CSP)

By default the first call compiles the WebAssembly module embedded in the
package with `WebAssembly.instantiate(bytes)`. Runtimes that forbid compiling
at run time (Cloudflare Workers, Vercel Edge) and pages whose
Content-Security-Policy lacks `wasm-unsafe-eval` refuse that. There, call
`initWasm` once, before the first use, with a precompiled `WebAssembly.Module`
that the platform or your bundler provides:

```ts
// Cloudflare Workers: wrangler turns a .wasm import into a compiled module.
import wasmModule from "@ultrafast/client/wasm/ultrafast_client_wasm_bg.wasm";
import { Client, gateway, initWasm } from "@ultrafast/client";

await initWasm(wasmModule);
const client = new Client(gateway({ baseUrl, key }));
```

`initWasm` also accepts bytes, a URL or a `Response`. After the module has
loaded (or a load has started) it does nothing. If loading fails, the error is
a `malformed` `UltrafastError` whose message ends with the cause's name and
message (`the WebAssembly module could not be loaded: CompileError: ...`), and
a later `initWasm` or call tries again.

## Build and test

Needs Rust with the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` 0.2.129 (user-level installs), and pnpm.

```
pnpm install
pnpm build:wasm   # writes src/wasm/ (git-ignored): the wasm-bindgen glue and the .wasm as base64
pnpm lint && pnpm typecheck && pnpm test
pnpm build        # build:wasm + tsc into dist/
```

`tests/fixtures.test.ts` runs the parity fixtures in `../fixtures/*.json`, the same
files the Rust and Python clients run (tests only; `src/` reads no files).

The `.wasm` is embedded as base64 in the package and loaded with `WebAssembly.instantiate`, so loading needs no file, URL or `fetch` access, and works the same everywhere. Tests use an injected fake `fetch`, plus one smoke test with the real `fetch` against a local HTTP server.
