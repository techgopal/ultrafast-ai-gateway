import { describe, expect, it } from "vitest";
import { Client, anthropic, azure, gateway, gemini, openai, openaiCompatible } from "../src/index.js";
import { KEY, MSGS, OPENAI_CHAT, fakeFetch, json } from "./helpers.js";

describe("chat", () => {
  it("gateway: OpenAI format under /v1, bearer key, tags header, parsed response", async () => {
    const { fetch, seen } = fakeFetch(json(200, OPENAI_CHAT));
    const c = new Client(gateway({ baseUrl: "http://gw.test:3000", key: KEY }), { fetch });
    const r = await c.chat({ model: "gpt-4o", messages: MSGS, maxTokens: 10, temperature: 0.5, topP: 0.5, stop: "x", tags: { team: "a" } });
    expect(r).toEqual({ id: "c1", model: "gpt-4o", content: "hello", toolCalls: [], finishReason: "stop", usage: { inputTokens: 3, outputTokens: 2 } });
    expect(seen).toHaveLength(1);
    const s = seen[0]!;
    expect(s.url).toBe("http://gw.test:3000/v1/chat/completions");
    expect(s.method).toBe("POST");
    expect(s.headers["authorization"]).toBe(`Bearer ${KEY}`);
    expect(s.headers["x-uf-tags"]).toBe('{"team":"a"}');
    expect(s.redirect).toBe("manual");
    const body = JSON.parse(s.body);
    expect(body).toMatchObject({ model: "gpt-4o", max_tokens: 10, temperature: 0.5, top_p: 0.5, stop: ["x"] });
    expect(body.stream).not.toBe(true);
  });

  it("openaiCompatible works without a key (keyless Ollama): no Authorization header", async () => {
    for (const mk of [() => openaiCompatible({ baseUrl: "http://ollama.test/v1", key: "" }), () => openaiCompatible({ baseUrl: "http://ollama.test/v1" })]) {
      const { fetch, seen } = fakeFetch(json(200, OPENAI_CHAT));
      const r = await new Client(mk(), { fetch }).chat({ model: "m", messages: MSGS });
      expect(r.content).toBe("hello");
      expect(seen[0]!.headers["authorization"]).toBeUndefined();
    }
  });

  it("the gateway and the other providers still need a key", () => {
    expect(() => gateway({ baseUrl: "http://gw.test", key: "" })).toThrow(TypeError);
    expect(() => openai({ key: "" })).toThrow(TypeError);
  });

  it("a trailing /v1 on the gateway address is accepted", async () => {
    const { fetch, seen } = fakeFetch(json(200, OPENAI_CHAT));
    await new Client(gateway({ baseUrl: "http://gw.test/v1/", key: KEY }), { fetch }).chat({ model: "m", messages: MSGS });
    expect(seen[0]!.url).toBe("http://gw.test/v1/chat/completions");
  });

  const cases: Array<[string, () => ReturnType<typeof openai>, string, string, Record<string, unknown>]> = [
    ["openai", () => openai({ key: KEY }), "https://api.openai.com/v1/chat/completions", "authorization", OPENAI_CHAT],
    ["openai base", () => openai({ key: KEY, baseUrl: "http://o.test/v1" }), "http://o.test/v1/chat/completions", "authorization", OPENAI_CHAT],
    ["openaiCompatible", () => openaiCompatible({ baseUrl: "http://groq.test/v1", key: KEY }), "http://groq.test/v1/chat/completions", "authorization", OPENAI_CHAT],
    [
      "anthropic",
      () => anthropic({ key: KEY }),
      "https://api.anthropic.com/v1/messages",
      "x-api-key",
      { id: "a1", model: "claude", content: [{ type: "text", text: "hello" }], stop_reason: "end_turn", usage: { input_tokens: 3, output_tokens: 2 } },
    ],
    [
      "gemini",
      () => gemini({ key: KEY }),
      "https://generativelanguage.googleapis.com/v1beta/models/m:generateContent",
      "x-goog-api-key",
      { candidates: [{ content: { parts: [{ text: "hello" }] }, finishReason: "STOP" }], usageMetadata: { promptTokenCount: 3, candidatesTokenCount: 2 } },
    ],
    [
      "azure",
      () => azure({ endpoint: "http://az.test", key: KEY, apiVersion: "2024-06-01" }),
      "http://az.test/openai/deployments/m/chat/completions?api-version=2024-06-01",
      "api-key",
      OPENAI_CHAT,
    ],
  ];
  it.each(cases)("%s target builds its provider's request and parses its answer", async (_n, mk, url, keyHeader, answer) => {
    const { fetch, seen } = fakeFetch(json(200, answer));
    const r = await new Client(mk(), { fetch }).chat({ model: "m", messages: MSGS });
    expect(r.content).toBe("hello");
    expect(seen[0]!.url).toBe(url);
    expect(seen[0]!.headers[keyHeader]).toContain(KEY);
  });

  it("tags go to a gateway and never to a provider", async () => {
    for (const [target, answer] of [
      [openai({ key: KEY }), OPENAI_CHAT],
      [anthropic({ key: KEY }), { id: "a", model: "m", content: [{ type: "text", text: "x" }], stop_reason: "end_turn", usage: { input_tokens: 1, output_tokens: 1 } }],
      [azure({ endpoint: "http://az.test", key: KEY }), OPENAI_CHAT],
    ] as const) {
      const { fetch, seen } = fakeFetch(json(200, answer));
      await new Client(target, { fetch }).chat({ model: "m", messages: MSGS, tags: { team: "a" } });
      expect(seen[0]!.headers).not.toHaveProperty("x-uf-tags");
      expect(seen[0]!.body).not.toContain("team");
    }
  });

  it("tags over 1 KiB are refused before anything is sent", async () => {
    const { fetch, seen } = fakeFetch(json(200, OPENAI_CHAT));
    const c = new Client(gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch });
    await expect(c.chat({ model: "m", messages: MSGS, tags: { a: "x".repeat(2000) } })).rejects.toMatchObject({ kind: "invalid_request", retryable: false });
    expect(seen).toHaveLength(0);
  });

  it("refuses a bad role or non-text content before sending", async () => {
    const { fetch, seen } = fakeFetch(json(200, OPENAI_CHAT));
    const c = new Client(openai({ key: KEY }), { fetch });
    // a tool message without toolCallId (the type allows it; the check refuses it)
    await expect(c.chat({ model: "m", messages: [{ role: "tool", content: "x" }] })).rejects.toMatchObject({ kind: "invalid_request" });
    // @ts-expect-error deliberately wrong
    await expect(c.chat({ model: "m", messages: [{ role: "user", content: [1] }] })).rejects.toMatchObject({ kind: "invalid_request" });
    expect(seen).toHaveLength(0);
  });

  it("always sends a stream:false chat even if the caller passed stream", async () => {
    const { fetch, seen } = fakeFetch(json(200, OPENAI_CHAT));
    // @ts-expect-error not part of the request type
    await new Client(openai({ key: KEY }), { fetch }).chat({ model: "m", messages: MSGS, stream: true });
    expect(JSON.parse(seen[0]!.body).stream).not.toBe(true);
  });
});

describe("embed", () => {
  const answer = { object: "list", data: [{ index: 0, embedding: [0.1, 0.2] }, { index: 1, embedding: [0.3, 0.4] }], model: "te", usage: { prompt_tokens: 4 } };
  it("gateway embeds one or many inputs", async () => {
    const { fetch, seen } = fakeFetch(json(200, answer));
    const c = new Client(gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch });
    const r = await c.embed({ model: "te", input: ["a", "b"], dimensions: 2, tags: { t: "1" } });
    expect(r.model).toBe("te");
    expect(r.vectors).toHaveLength(2);
    expect(r.vectors[0]![0]).toBeCloseTo(0.1);
    expect(r.promptTokens).toBe(4);
    expect(seen[0]!.url).toBe("http://gw.test/v1/embeddings");
    expect(seen[0]!.headers["x-uf-tags"]).toBe('{"t":"1"}');
    expect(JSON.parse(seen[0]!.body)).toMatchObject({ model: "te", input: ["a", "b"], dimensions: 2 });
    await c.embed({ model: "te", input: "single" });
    expect(JSON.parse(seen[1]!.body).input).toEqual(["single"]);
  });
  it("no tags to a provider", async () => {
    const { fetch, seen } = fakeFetch(json(200, answer));
    await new Client(openai({ key: KEY }), { fetch }).embed({ model: "te", input: ["a"], tags: { t: "1" } });
    expect(seen[0]!.headers).not.toHaveProperty("x-uf-tags");
  });
  it("an error answer is typed", async () => {
    const { fetch } = fakeFetch(json(401, { error: { message: "no" } }));
    await expect(new Client(openai({ key: KEY }), { fetch }).embed({ model: "te", input: ["a"] })).rejects.toMatchObject({ kind: "auth", status: 401, retryable: false });
  });
});
