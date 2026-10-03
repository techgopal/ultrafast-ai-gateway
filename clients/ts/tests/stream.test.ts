import { describe, expect, it } from "vitest";
import { Client, UltrafastError, anthropic, gateway, type StreamEvent } from "../src/index.js";
import { KEY, MSGS, OPENAI_STREAM, fakeFetch, json, pieces, sse, streamResponse } from "./helpers.js";

const gw = (fetch: typeof globalThis.fetch, opts = {}) =>
  new Client(gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch, ...opts });
const req = { model: "m", messages: MSGS };

async function collect(c: Client): Promise<{ events: StreamEvent[]; error?: UltrafastError }> {
  const events: StreamEvent[] = [];
  try {
    for await (const e of c.chatStream(req)) events.push(e);
  } catch (e) {
    expect(e).toBeInstanceOf(UltrafastError);
    return { events, error: e as UltrafastError };
  }
  return { events };
}

const EXPECTED: StreamEvent[] = [
  { type: "delta", text: "he" },
  { type: "delta", text: "llo" },
  { type: "done", finishReason: "stop", usage: { inputTokens: 3, outputTokens: 2 } },
];

describe("chatStream", () => {
  it("whole body", async () => {
    const { fetch, seen } = fakeFetch(streamResponse(OPENAI_STREAM));
    const { events, error } = await collect(gw(fetch, { timeoutMs: 1000 }));
    expect(error).toBeUndefined();
    expect(events).toEqual(EXPECTED);
    expect(seen[0]!.headers["accept"]).toBe("text/event-stream");
    expect(JSON.parse(seen[0]!.body).stream).toBe(true);
    expect(seen[0]!.headers["x-uf-tags"]).toBeUndefined();
  });

  it("split at every byte gives the same events", async () => {
    const { events, error } = await collect(gw(fakeFetch(streamResponse(OPENAI_STREAM, "byte")).fetch));
    expect(error).toBeUndefined();
    expect(events).toEqual(EXPECTED);
  });

  it("split at every byte inside a multi-byte character", async () => {
    const text = sse({ choices: [{ delta: { content: "héllo 世界 \u{1F600}" } }] }) + sse({ choices: [{ delta: {}, finish_reason: "stop" }] }) + "data: [DONE]\n\n";
    const { events } = await collect(gw(fakeFetch(streamResponse(text, "byte")).fetch));
    expect(events.filter((e) => e.type === "delta").map((e) => (e as { text: string }).text).join("")).toBe("héllo 世界 \u{1F600}");
    expect(events.at(-1)?.type).toBe("done");
  });

  it("anthropic streams decode too", async () => {
    const text = [
      'event: message_start\ndata: {"type":"message_start","message":{"usage":{"input_tokens":3}}}\n\n',
      'event: content_block_delta\ndata: {"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}\n\n',
      'event: message_delta\ndata: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}\n\n',
      'event: message_stop\ndata: {"type":"message_stop"}\n\n',
    ].join("");
    const c = new Client(anthropic({ key: KEY }), { fetch: fakeFetch(streamResponse(text, "byte")).fetch });
    const { events, error } = await collect(c);
    expect(error).toBeUndefined();
    expect(events[0]).toEqual({ type: "delta", text: "hi" });
    expect(events.at(-1)).toMatchObject({ type: "done", finishReason: "stop" });
  });

  it("text first, then an error event: the text arrives, then a typed error", async () => {
    const text = sse({ choices: [{ delta: { content: "a" } }] }) + `data: ${JSON.stringify({ error: { message: "overloaded", type: "overloaded_error" } })}\n\n`;
    for (const split of ["whole", "byte"] as const) {
      const { events, error } = await collect(gw(fakeFetch(streamResponse(text, split)).fetch));
      expect(events).toEqual([{ type: "delta", text: "a" }]);
      expect(error).toBeDefined();
      expect(error!.message).toContain("overloaded");
    }
  });

  it("a close before Done is a malformed error after the text, not silent truncation", async () => {
    const text = sse({ choices: [{ delta: { content: "a" } }] });
    const { events, error } = await collect(gw(fakeFetch(streamResponse(text)).fetch));
    expect(events).toEqual([{ type: "delta", text: "a" }]);
    expect(error).toMatchObject({ kind: "malformed", retryable: false, message: "the stream ended before it was complete" });
  });

  it("an empty body is malformed", async () => {
    const { error } = await collect(gw(fakeFetch(streamResponse("")).fetch));
    expect(error?.kind).toBe("malformed");
  });

  it("a broken connection mid-stream is a network error after the text", async () => {
    const text = sse({ choices: [{ delta: { content: "a" } }] });
    const { events, error } = await collect(gw(fakeFetch(streamResponse(text, "whole", { fail: new TypeError("terminated") })).fetch));
    expect(events).toEqual([{ type: "delta", text: "a" }]);
    expect(error).toMatchObject({ kind: "network", retryable: true });
  });

  it("a stream that goes quiet is a timeout after the text (idle timeout)", async () => {
    const text = sse({ choices: [{ delta: { content: "a" } }] });
    const { events, error } = await collect(gw(fakeFetch(streamResponse(text, "whole", { stall: true })).fetch, { timeoutMs: 40 }));
    expect(events).toEqual([{ type: "delta", text: "a" }]);
    expect(error).toMatchObject({ kind: "timeout", retryable: true });
  });

  it("the idle timeout restarts on every chunk, so a long stream is not cut by a total limit", async () => {
    const enc = new TextEncoder();
    const parts = OPENAI_STREAM.split("\n\n").filter(Boolean).map((p) => enc.encode(p + "\n\n"));
    const slow = new ReadableStream<Uint8Array>({
      async pull(c) {
        const next = parts.shift();
        if (!next) return c.close();
        await new Promise((r) => setTimeout(r, 30));
        c.enqueue(next);
        return undefined;
      },
    });
    const { fetch } = fakeFetch(new Response(slow, { status: 200 }));
    const { events, error } = await collect(gw(fetch, { timeoutMs: 80 }));
    expect(error).toBeUndefined();
    expect(events).toEqual(EXPECTED);
  });

  it("a server that never answers is a timeout before any event", async () => {
    const { fetch } = fakeFetch(() => new Promise<Response>(() => {}));
    const { events, error } = await collect(gw(fetch, { timeoutMs: 30 }));
    expect(events).toEqual([]);
    expect(error?.kind).toBe("timeout");
  });

  it("an error status is thrown typed (429 with Retry-After) before any event", async () => {
    const { fetch } = fakeFetch(json(429, { error: { message: "slow" } }, { "retry-after": "3" }));
    const { events, error } = await collect(gw(fetch));
    expect(events).toEqual([]);
    expect(error).toMatchObject({ kind: "rate_limited", retryable: true, retryAfter: 3, status: 429 });
  });

  it("an error body that stalls times out", async () => {
    const r = new Response(pieces([new TextEncoder().encode("{")], { stall: true }), { status: 500 });
    const { error } = await collect(gw(fakeFetch(r).fetch, { timeoutMs: 30 }));
    expect(error?.kind).toBe("timeout");
  });

  it("stops reading and aborts the request when the caller stops early", async () => {
    let signal: AbortSignal | null | undefined;
    let cancelled = false;
    const enc = new TextEncoder();
    const body = new ReadableStream<Uint8Array>({
      pull(c) { c.enqueue(enc.encode(sse({ choices: [{ delta: { content: "a" } }] }))); },
      cancel() { cancelled = true; },
    });
    const { fetch } = fakeFetch((_s, init) => { signal = init.signal; return new Response(body, { status: 200 }); });
    for await (const e of gw(fetch).chatStream(req)) {
      expect(e.type).toBe("delta");
      break;
    }
    expect(cancelled).toBe(true);
    expect(signal?.aborted).toBe(true);
  });

  it("stops after Done and cancels the rest of the body", async () => {
    let cancelled = false;
    let sent = false;
    const body = new ReadableStream<Uint8Array>(
      {
        pull(c) {
          if (!sent) {
            sent = true;
            c.enqueue(new TextEncoder().encode(OPENAI_STREAM));
            return undefined;
          }
          return new Promise(() => {}); // the server keeps the connection open
        },
        cancel() {
          cancelled = true;
        },
      },
      { highWaterMark: 0 },
    );
    const { events, error } = await collect(gw(fakeFetch(() => new Response(body, { status: 200 })).fetch, { timeoutMs: 500 }));
    await new Promise((r) => setTimeout(r, 0));
    expect(error).toBeUndefined();
    expect(events).toEqual(EXPECTED);
    expect(cancelled).toBe(true);
  });
});
