import { inspect } from "node:util";
import { describe, expect, it } from "vitest";
import {
  AuthenticationError, Client, MalformedError, NetworkError, RateLimitError, RequestTimeoutError,
  UltrafastError, UpstreamError, anthropic, gateway, openai,
} from "../src/index.js";
import { KEY, MSGS, OPENAI_CHAT, OPENAI_STREAM, fakeFetch, json, pieces, streamResponse } from "./helpers.js";

const gw = (fetch: typeof globalThis.fetch, opts = {}) =>
  new Client(gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch, ...opts });
const req = { model: "m", messages: MSGS };

async function failure(p: Promise<unknown>): Promise<UltrafastError> {
  try {
    await p;
  } catch (e) {
    expect(e).toBeInstanceOf(UltrafastError);
    return e as UltrafastError;
  }
  throw new Error("did not throw");
}

describe("errors", () => {
  it("429 with Retry-After is rate_limited, retryable, with retryAfter seconds", async () => {
    const e = await failure(gw(fakeFetch(json(429, { error: { message: "slow down" } }, { "retry-after": "7" })).fetch).chat(req));
    expect(e).toBeInstanceOf(RateLimitError);
    expect({ kind: e.kind, status: e.status, retryable: e.retryable, retryAfter: e.retryAfter, message: e.message }).toEqual({
      kind: "rate_limited", status: 429, retryable: true, retryAfter: 7, message: "slow down",
    });
  });

  it("maps statuses to kinds", async () => {
    const table: Array<[number, string, boolean]> = [
      [401, "auth", false], [403, "permission", false], [404, "not_found", false],
      [400, "invalid_request", false], [500, "upstream", true], [503, "upstream", true],
    ];
    for (const [status, kind, retryable] of table) {
      const e = await failure(gw(fakeFetch(json(status, { error: { message: "x" } })).fetch).chat(req));
      expect({ kind: e.kind, retryable, status: e.status }).toEqual({ kind, retryable, status });
      expect(e.retryable).toBe(retryable);
      expect(e.retryAfter).toBeUndefined();
    }
    expect(await failure(gw(fakeFetch(json(401, {})).fetch).chat(req))).toBeInstanceOf(AuthenticationError);
    expect(await failure(gw(fakeFetch(json(502, {})).fetch).chat(req))).toBeInstanceOf(UpstreamError);
  });

  it("a redirect is an error and is not followed", async () => {
    const { fetch, seen } = fakeFetch(new Response("", { status: 302, headers: { location: "http://evil.test/" } }));
    const e = await failure(gw(fetch).chat(req));
    expect(e.kind).toBe("invalid_request");
    expect(e.message).toBe("the server answered with a redirect");
    expect(seen).toHaveLength(1);
  });

  it("an opaque redirect (browser fetch with manual redirects) is the same error", async () => {
    const r = new Response("", { status: 200 });
    Object.defineProperty(r, "type", { value: "opaqueredirect" });
    Object.defineProperty(r, "status", { value: 0 });
    const e = await failure(gw(fakeFetch(() => r).fetch).chat(req));
    expect(e.message).toBe("the server answered with a redirect");
  });

  it("a status outside 0..999 is clamped, not wrapped", async () => {
    const r = new Response("{}", { status: 200 });
    Object.defineProperty(r, "status", { value: 65536 + 200 });
    const e = await failure(gw(fakeFetch(() => r).fetch).chat(req));
    expect(e.status).toBeLessThanOrEqual(999);
    expect(e.status).toBe(999);
    expect(e.kind).toBe("upstream");
  });

  it("an unparseable 200 is malformed", async () => {
    const e = await failure(gw(fakeFetch(new Response("<html>", { status: 200 })).fetch).chat(req));
    expect(e).toBeInstanceOf(MalformedError);
    expect(e.retryable).toBe(false);
  });

  it("a connection failure is network, retryable, and does not carry the key", async () => {
    const e = await failure(gw(fakeFetch(new TypeError(`fetch failed http://gw.test/?k=${KEY}`)).fetch).chat(req));
    expect(e).toBeInstanceOf(NetworkError);
    expect({ kind: e.kind, retryable: e.retryable, status: e.status }).toEqual({ kind: "network", retryable: true, status: undefined });
    expect(JSON.stringify(e) + String(e) + e.message + inspect(e, { depth: 5 })).not.toContain(KEY);
  });

  it("a timeout waiting for the answer is timeout and retryable, even if fetch ignores the signal", async () => {
    const { fetch } = fakeFetch(() => new Promise<Response>(() => {}));
    const e = await failure(gw(fetch, { timeoutMs: 30 }).chat(req));
    expect(e).toBeInstanceOf(RequestTimeoutError);
    expect({ kind: e.kind, retryable: e.retryable }).toEqual({ kind: "timeout", retryable: true });
  });

  it("a body that stalls after the headers times out, and the request is aborted", async () => {
    let signal: AbortSignal | null | undefined;
    const { fetch } = fakeFetch((_s, init) => {
      signal = init.signal;
      return new Response(pieces([new TextEncoder().encode("{")], { stall: true }), { status: 200 });
    });
    const e = await failure(gw(fetch, { timeoutMs: 30 }).chat(req));
    expect(e.kind).toBe("timeout");
    expect(signal?.aborted).toBe(true);
  });

  it("an answer larger than the cap is malformed (declared or streamed), also for error bodies", async () => {
    const big = new TextEncoder().encode("x".repeat(100));
    const declared = new Response(big, { status: 200, headers: { "content-length": "100" } });
    expect((await failure(gw(fakeFetch(declared).fetch, { maxResponseBytes: 50 }).chat(req))).kind).toBe("malformed");
    const undeclared = new Response(pieces([big.slice(0, 40), big.slice(40)]), { status: 200 });
    const e = await failure(gw(fakeFetch(undeclared).fetch, { maxResponseBytes: 50 }).chat(req));
    expect(e.kind).toBe("malformed");
    expect(e.message).toContain("50");
    const errBody = new Response(pieces([big.slice(0, 40), big.slice(40)]), { status: 500 });
    expect((await failure(gw(fakeFetch(errBody).fetch, { maxResponseBytes: 50 }).chat(req))).kind).toBe("malformed");
  });

  it("the default cap is 32 MiB", async () => {
    const declared = new Response("x", { status: 200, headers: { "content-length": String(33 * 1024 * 1024) } });
    expect((await failure(gw(fakeFetch(declared).fetch).chat(req))).kind).toBe("malformed");
  });
});

describe("the key never appears", () => {
  it("a provider that echoes the key in its error is scrubbed (also when JSON escapes it)", async () => {
    const key = 'sk-"quoted"\\key';
    // json() escapes the quotes and the backslash on the wire; the message is matched decoded.
    const { fetch } = fakeFetch(json(401, { error: { message: `bad key ${key}` } }));
    const e = await failure(new Client(openai({ key }), { fetch }).chat(req));
    expect(e.message).toBe("bad key [redacted]");
  });

  it("not in the stream error, the target, the client or the error's inspect output", async () => {
    const t = anthropic({ key: KEY });
    const c = new Client(t, { fetch: fakeFetch(json(500, { error: { message: `boom ${KEY}` } })).fetch });
    const e = await failure(c.chat(req));
    for (const text of [JSON.stringify(t), String(t), inspect(t), JSON.stringify(c), String(c), inspect(c), JSON.stringify(e), String(e), inspect(e, { depth: 8 }), e.stack ?? ""]) {
      expect(text).not.toContain(KEY);
    }
    const s = new Client(t, { fetch: fakeFetch(json(500, { error: { message: `boom ${KEY}` } })).fetch });
    const e2 = await failure((async () => { for await (const _ of s.chatStream(req)) void _; })());
    expect(e2.message).not.toContain(KEY);
  });

  it("a stream that fails mid-way with the key in the error is scrubbed", async () => {
    const text = `data: {"choices":[{"delta":{"content":"a"}}]}\n\ndata: {"error":{"message":"leak ${KEY}"}}\n\n`;
    const c = gw(fakeFetch(streamResponse(text)).fetch);
    const got: unknown[] = [];
    const e = await failure((async () => { for await (const ev of c.chatStream(req)) got.push(ev); })());
    expect(got).toEqual([{ type: "delta", text: "a" }]);
    expect(e.message).not.toContain(KEY);
  });
});

describe("misuse", () => {
  it("OPENAI_STREAM fixture sanity", () => {
    expect(OPENAI_STREAM).toContain("[DONE]");
    expect(OPENAI_CHAT.id).toBe("c1");
  });
});
