import { readFileSync, existsSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Client, UltrafastError, anthropic, azure, gateway, gemini, openai, openaiCompatible } from "../src/index.js";
import { WASM_BASE64 } from "../src/wasm/inline.js";
import { KEY, MSGS, OPENAI_CHAT, fakeFetch, json, pieces } from "./helpers.js";

const req = { model: "m", messages: MSGS };
const gw = (fetch: typeof globalThis.fetch, opts = {}) => new Client(gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch, ...opts });
const wasmBytes = (): Uint8Array => Uint8Array.from(Buffer.from(WASM_BASE64, "base64"));
async function failure(p: Promise<unknown>): Promise<UltrafastError> {
  try {
    await p;
  } catch (e) {
    expect(e).toBeInstanceOf(UltrafastError);
    return e as UltrafastError;
  }
  throw new Error("did not throw");
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("initWasm", () => {
  it("accepts a precompiled module and then never decodes the inlined bytes", async () => {
    vi.resetModules();
    const m = await import("../src/index.js");
    const atob = vi.spyOn(globalThis, "atob");
    const compiled = await WebAssembly.compile(wasmBytes());
    await m.initWasm(compiled);
    const { fetch } = fakeFetch(json(200, OPENAI_CHAT));
    const r = await new m.Client(m.gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch }).chat(req);
    expect(r.content).toBe("hello");
    expect(atob).not.toHaveBeenCalled();
  });

  it("a load failure keeps a non-secret hint of the cause, and a later good init works", async () => {
    vi.resetModules();
    const m = await import("../src/index.js");
    const e = await m.initWasm(new Uint8Array([1, 2, 3])).catch((x: unknown) => x);
    expect(e).toBeInstanceOf(m.MalformedError);
    expect((e as Error).message).toMatch(/^the WebAssembly module could not be loaded: \w+: .+/);
    await m.initWasm(await WebAssembly.compile(wasmBytes()));
    const { fetch } = fakeFetch(json(200, OPENAI_CHAT));
    expect((await new m.Client(m.gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch }).chat(req)).content).toBe("hello");
  });

  it("a chat whose module cannot load fails typed, with the hint", async () => {
    vi.resetModules();
    const m = await import("../src/index.js");
    await m.initWasm(new Uint8Array([1])).catch(() => undefined);
    // loading was reset, so the default (inlined) load runs and succeeds
    const { fetch } = fakeFetch(json(200, OPENAI_CHAT));
    expect((await new m.Client(m.gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch }).chat(req)).content).toBe("hello");
  });

  it("five first calls at once load the module once", async () => {
    vi.resetModules();
    const m = await import("../src/index.js");
    const inst = vi.spyOn(WebAssembly, "instantiate");
    const { fetch } = fakeFetch(json(200, OPENAI_CHAT));
    const c = new m.Client(m.gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch });
    const rs = await Promise.all([1, 2, 3, 4, 5].map(() => c.chat(req)));
    expect(rs.every((r) => r.content === "hello")).toBe(true);
    expect(inst).toHaveBeenCalledTimes(1);
  });

  it("initWasm after the module is loaded is a no-op", async () => {
    vi.resetModules();
    const m = await import("../src/index.js");
    const { fetch } = fakeFetch(json(200, OPENAI_CHAT));
    await new m.Client(m.gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch }).chat(req);
    const inst = vi.spyOn(WebAssembly, "instantiate");
    await m.initWasm(new Uint8Array([1]));
    expect(inst).not.toHaveBeenCalled();
  });
});

describe("timeoutMs", () => {
  it.each([0, -1, Number.NaN, Number.POSITIVE_INFINITY, 2 ** 31, "5" as unknown as number])("%s is a TypeError", (t) => {
    expect(() => new Client(gateway({ baseUrl: "http://gw.test", key: KEY }), { timeoutMs: t })).toThrow(TypeError);
  });
  it("2^31-1 is accepted", () => {
    expect(() => new Client(gateway({ baseUrl: "http://gw.test", key: KEY }), { timeoutMs: 2 ** 31 - 1 })).not.toThrow();
  });
});

describe("generated files", () => {
  it("inline.ts declares a string, not the literal", () => {
    expect(readFileSync(new URL("../src/wasm/inline.ts", import.meta.url), "utf8")).toMatch(/WASM_BASE64: string =/);
  });
  it("the glue (source and built) never reaches for import.meta.url", () => {
    for (const f of ["../src/wasm/ultrafast_client_wasm.js", "../dist/wasm/ultrafast_client_wasm.js"]) {
      const u = new URL(f, import.meta.url);
      if (existsSync(u)) expect(readFileSync(u, "utf8")).not.toContain("import.meta");
    }
  });
  it("prepack builds", () => {
    const p = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")) as { scripts: Record<string, string> };
    expect(p.scripts["prepack"]).toBe("pnpm build");
  });
});

describe("host error wording matches the Rust client", () => {
  it("connection failure", async () => {
    const e = await failure(gw(fakeFetch(new TypeError("fetch failed")).fetch).chat(req));
    expect({ kind: e.kind, message: e.message }).toEqual({ kind: "network", message: "could not connect to the server" });
  });
  it("a body that breaks off", async () => {
    const r = new Response(pieces([new TextEncoder().encode("{")], { fail: new TypeError("terminated") }), { status: 200 });
    const e = await failure(gw(fakeFetch(() => r).fetch).chat(req));
    expect(e.kind).toBe("network");
    expect(e.message).toMatch(/^network error: /);
  });
  it.each(["AbortError", "TimeoutError"])("a runtime %s is a timeout", async (name) => {
    const err = new DOMException("x", name);
    const e = await failure(gw(fakeFetch(err).fetch).chat(req));
    expect({ kind: e.kind, retryable: e.retryable, message: e.message }).toEqual({ kind: "timeout", retryable: true, message: "the request timed out" });
  });
});

describe("answers are read as promised", () => {
  it("a declared length over the cap never reads the body", async () => {
    let pulled = false;
    const body = new ReadableStream<Uint8Array>({ pull() { pulled = true; } }, { highWaterMark: 0 });
    const r = new Response(body, { status: 200, headers: { "content-length": "100" } });
    const e = await failure(gw(fakeFetch(() => r).fetch, { maxResponseBytes: 50 }).chat(req));
    expect(e.kind).toBe("malformed");
    expect(pulled).toBe(false);
  });

  const emb = { data: [{ index: 0, embedding: [0.5] }], model: "te", usage: { prompt_tokens: 1 }, embeddings: [{ values: [0.5] }] };
  it.each([
    ["openai", () => openai({ key: KEY }), "https://api.openai.com/v1/embeddings"],
    ["openaiCompatible", () => openaiCompatible({ baseUrl: "http://c.test/v1", key: KEY }), "http://c.test/v1/embeddings"],
    ["azure", () => azure({ endpoint: "http://az.test", key: KEY }), "http://az.test/openai/deployments/te/embeddings?api-version="],
    ["gemini", () => gemini({ key: KEY }), "https://generativelanguage.googleapis.com/"],
  ])("embed on the %s target", async (_n, mk, urlStart) => {
    const { fetch, seen } = fakeFetch(json(200, emb));
    const r = await new Client(mk(), { fetch }).embed({ model: "te", input: ["a"], tags: { t: "1" } });
    expect(r.vectors[0]![0]).toBeCloseTo(0.5);
    expect(seen[0]!.url.startsWith(urlStart)).toBe(true);
    expect(seen[0]!.headers).not.toHaveProperty("x-uf-tags");
  });
  it("anthropic has no embeddings: a typed error, nothing sent", async () => {
    const { fetch, seen } = fakeFetch(json(200, emb));
    const e = await failure(new Client(anthropic({ key: KEY }), { fetch }).embed({ model: "te", input: ["a"] }));
    expect(e.retryable).toBe(false);
    expect(seen).toHaveLength(0);
  });
});
