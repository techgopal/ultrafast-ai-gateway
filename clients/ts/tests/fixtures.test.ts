// The shared parity fixtures (clients/fixtures/*.json): the Rust and Python
// clients run the same files and must agree with this one.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  Client,
  UltrafastError,
  anthropic,
  azure,
  gateway,
  gemini,
  openai,
  openaiCompatible,
  type ChatRequest,
  type EmbeddingsRequest,
  type StreamEvent,
  type Target,
} from "../src/index.js";
import { fakeFetch, pieces, type Seen } from "./helpers.js";

interface Spec {
  kind: string;
  base_url: string;
  api_version?: string;
}
interface WireError {
  kind: string;
  retryable: boolean;
  status: number | null;
  retry_after: number | null;
  message?: string;
}
interface Case {
  name: string;
  op: string;
  target: Spec;
  request: Record<string, unknown> & { model: string; messages: ChatRequest["messages"]; input: string[] };
  model?: string;
  status: number;
  headers: Record<string, string>;
  body: string;
  chunks: string[];
  expect: {
    sent: boolean;
    method: string;
    path: string;
    auth_header: string;
    headers: Record<string, string>;
    body: unknown;
    ok?: unknown;
    error?: WireError | null;
    events?: unknown[];
  };
}

function load(name: string): { key: string; cases: Case[] } {
  const url = new URL(`../../fixtures/${name}`, import.meta.url);
  return JSON.parse(readFileSync(url, "utf8")) as { key: string; cases: Case[] };
}

const BASE = "http://fx.test";
const { key: KEY, cases: REQUESTS } = load("requests.json");
const RESPONSES = load("responses.json").cases;
const ERRORS = load("errors.json").cases;
const STREAMS = load("streams.json").cases;

function target(t: Spec): Target {
  const baseUrl = t.base_url.replace("{base}", BASE);
  switch (t.kind) {
    case "gateway":
      return gateway({ baseUrl, key: KEY });
    case "openai":
      return openai({ key: KEY, baseUrl });
    case "openai_compatible":
      return openaiCompatible({ baseUrl, key: KEY });
    case "anthropic":
      return anthropic({ key: KEY, baseUrl });
    case "gemini":
      return gemini({ key: KEY, baseUrl });
    case "azure":
      return azure({ endpoint: baseUrl, key: KEY, ...(t.api_version ? { apiVersion: t.api_version } : {}) });
    default:
      throw new Error(`unknown kind ${t.kind}`);
  }
}

function chatRequest(r: Case["request"]): ChatRequest {
  const c: ChatRequest = { model: r.model, messages: r.messages };
  if (r["max_tokens"] !== undefined) c.maxTokens = r["max_tokens"] as number;
  if (r["temperature"] !== undefined) c.temperature = r["temperature"] as number;
  if (r["top_p"] !== undefined) c.topP = r["top_p"] as number;
  if (r["stop"] !== undefined) c.stop = r["stop"] as string[];
  if (r["tags"] !== undefined) c.tags = r["tags"] as Record<string, string>;
  return c;
}

function embedRequest(r: Case["request"]): EmbeddingsRequest {
  const e: EmbeddingsRequest = { model: r.model, input: r.input };
  if (r["dimensions"] !== undefined) e.dimensions = r["dimensions"] as number;
  if (r["tags"] !== undefined) e.tags = r["tags"] as Record<string, string>;
  return e;
}

function errorOf(e: unknown): WireError & { message: string } {
  expect(e).toBeInstanceOf(UltrafastError);
  const u = e as UltrafastError;
  return {
    kind: u.kind,
    retryable: u.retryable,
    status: u.status ?? null,
    retry_after: u.retryAfter ?? null,
    message: u.message,
  };
}

/** A fixture without `message` leaves it out of the comparison. */
function checkError(got: WireError & { message: string }, want: WireError): void {
  expect(got.kind).toBe(want.kind);
  expect(got.retryable).toBe(want.retryable);
  expect(got.status).toBe(want.status);
  expect(got.retry_after).toBe(want.retry_after);
  if (want.message !== undefined) expect(got.message).toBe(want.message);
}

const enc = new TextEncoder();
const hi = [{ role: "user" as const, content: "hi" }];

describe("requests on the wire", () => {
  it.each(REQUESTS.map((c) => [c.name, c] as const))("%s", async (_name, c) => {
    const body = c.op === "chat_stream" ? "data: [DONE]\n\n" : "{}";
    const type = c.op === "chat_stream" ? "text/event-stream" : "application/json";
    const { fetch, seen } = fakeFetch(new Response(body, { status: 200, headers: { "content-type": type } }));
    const client = new Client(target(c.target), { fetch });
    let failure: unknown;
    try {
      if (c.op === "chat") await client.chat(chatRequest(c.request));
      else if (c.op === "embed") await client.embed(embedRequest(c.request));
      else for await (const _e of client.chatStream(chatRequest(c.request))) void _e;
    } catch (e) {
      failure = e;
    }
    const want = c.expect;
    if (!want.sent) {
      expect(seen).toEqual([]);
      checkError(errorOf(failure), want.error as WireError);
      return;
    }
    expect(seen).toHaveLength(1);
    const s = seen[0] as Seen;
    expect(s.method).toBe(want.method);
    expect(s.url).toBe(BASE + want.path);
    for (const [k, v] of Object.entries(want.headers)) expect(s.headers[k], k).toBe(v);
    if (!("x-uf-tags" in want.headers)) expect(s.headers["x-uf-tags"]).toBeUndefined();
    expect(s.headers[want.auth_header]).toContain(KEY);
    expect(JSON.parse(s.body)).toEqual(want.body);
  });
});

function answer(c: Case): Promise<unknown> {
  const { fetch } = fakeFetch(new Response(c.body, { status: c.status, headers: c.headers }));
  const client = new Client(target(c.target), { fetch });
  if (c.op === "embed") {
    return client.embed({ model: c.model ?? "m", input: ["a"] }).then((r) => ({
      model: r.model,
      vectors: r.vectors,
      prompt_tokens: r.promptTokens,
    }));
  }
  return client.chat({ model: "m", messages: hi }).then((r) => ({
    id: r.id ?? "",
    model: r.model ?? "",
    content: r.content,
    finish_reason: r.finishReason,
    usage: r.usage && { input_tokens: r.usage.inputTokens, output_tokens: r.usage.outputTokens },
  }));
}

describe("parsed responses and their errors", () => {
  it.each(RESPONSES.map((c) => [c.name, c] as const))("%s", async (_name, c) => {
    if (c.expect.ok !== undefined) {
      expect(await answer(c)).toEqual(c.expect.ok);
    } else {
      const e = await answer(c).then(
        () => undefined,
        (x: unknown) => x,
      );
      checkError(errorOf(e), c.expect.error as WireError);
    }
  });
});

describe("http errors", () => {
  it.each(ERRORS.map((c) => [c.name, c] as const))("%s", async (_name, c) => {
    const e = await answer(c).then(
      () => undefined,
      (x: unknown) => x,
    );
    checkError(errorOf(e), c.expect.error as WireError);
  });
});

function eventOf(e: StreamEvent): unknown {
  return e.type === "delta"
    ? { type: "delta", text: e.text }
    : {
        type: "done",
        finish_reason: e.finishReason,
        usage: e.usage && { input_tokens: e.usage.inputTokens, output_tokens: e.usage.outputTokens },
      };
}

async function runStream(c: Case, chunks: string[]): Promise<{ events: unknown[]; error: unknown }> {
  const body = pieces(chunks.filter((p) => p !== "").map((p) => enc.encode(p)));
  const { fetch } = fakeFetch(new Response(body, { status: 200, headers: { "content-type": "text/event-stream" } }));
  const client = new Client(target(c.target), { fetch, timeoutMs: 2000 });
  const events: unknown[] = [];
  let error: unknown;
  try {
    for await (const e of client.chatStream({ model: "m", messages: hi })) events.push(eventOf(e));
  } catch (e) {
    error = e;
  }
  return { events, error };
}

describe("streams", () => {
  for (const shape of ["pieces", "whole"] as const) {
    it.each(STREAMS.map((c) => [c.name, c] as const))(`%s (${shape})`, async (_name, c) => {
      const chunks = shape === "pieces" ? c.chunks : [c.chunks.join("")];
      const { events, error } = await runStream(c, chunks);
      expect(events).toEqual(c.expect.events);
      if (c.expect.error === null) expect(error).toBeUndefined();
      else checkError(errorOf(error), c.expect.error as WireError);
    });
  }
});
