# @ultrafast/client (TypeScript)

Client for the Ultrafast gateway, or for a provider directly. Request building,
response parsing and stream decoding are the same Rust code the gateway and the
Rust and Python clients use, compiled to WebAssembly (`crates/client-wasm`); the
network is the runtime's own `fetch`. No runtime dependencies, no Node-specific
APIs in `src/`: it runs in Node 20+, Bun, Deno, browsers and edge runtimes.

```ts
import { Client, gateway } from "@ultrafast/client";

const client = new Client(gateway({ baseUrl: "http://localhost:3000", key }));

const reply = await client.chat({ model: "gpt-4o", messages: [{ role: "user", content: "hi" }], tags: { team: "a" } });
console.log(reply.content, reply.usage);

for await (const event of client.chatStream({ model: "gpt-4o", messages: [{ role: "user", content: "hi" }] })) {
  if (event.type === "delta") process.stdout.write(event.text);
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
| `openaiCompatible({ baseUrl, key })` | Groq, Mistral, OpenRouter, Ollama; `baseUrl` includes `/v1`. |

## Options

`new Client(target, { fetch?, timeoutMs?, maxResponseBytes? })`

- `fetch`: your own (tests, proxies, edge runtimes). Requests are sent with `redirect: "manual"`; a redirect is an error and is never followed (the credential would go with it). If you supply a `fetch`, do not make it follow redirects with credentials.
- `timeoutMs` (default 120000): for the whole of `chat`/`embed`, for the answer to start a stream, and for each silent stretch inside a stream. The client enforces it itself (an abort plus a race), so a `fetch` that ignores `AbortSignal` is still bounded.
- `maxResponseBytes` (default 32 MiB): the most a `chat`/`embed` answer or any error body may hold; a larger one is a `malformed` error. Streams are not capped in total.

## Errors

Every failure is an `UltrafastError` with `kind` (`auth | permission | not_found | invalid_request | rate_limited | upstream | network | timeout | malformed`), `status` (when there was an HTTP answer), `retryable` and `retryAfter` (seconds, when the server sent `Retry-After`). There are subclasses per kind (`RateLimitError`, `RequestTimeoutError`, ...). The client does not retry, route, cache or break circuits. The key is removed from every message and is not on `Target`, `Client` or the error when printed or serialised.

A stream yields its events in order, then at most one error: a provider error event, a broken connection, a silent stretch, or a close before the end (`malformed`, "the stream ended before it was complete") arrives after the text already received, never as a silent end. `chatStream` starts the request on the first `next()`; leaving the loop early cancels the request.

## Build and test

Needs Rust with the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` 0.2.129 (user-level installs), and pnpm.

```
pnpm install
pnpm build:wasm   # writes src/wasm/ (git-ignored): the wasm-bindgen glue and the .wasm as base64
pnpm lint && pnpm typecheck && pnpm test
pnpm build        # build:wasm + tsc into dist/
```

The `.wasm` is embedded as base64 in the package and loaded with `WebAssembly.instantiate`, so loading needs no file, URL or `fetch` access, and works the same everywhere. Tests use an injected fake `fetch`, plus one smoke test with the real `fetch` against a local HTTP server.
