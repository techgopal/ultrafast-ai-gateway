import { describe, expect, it } from "vitest";
import { Client, UltrafastError, anthropic, gateway, type ChatRequest, type StreamEvent } from "../src/index.js";
import { KEY, fakeFetch, json, sse, streamResponse } from "./helpers.js";

const gw = (fetch: typeof globalThis.fetch) => new Client(gateway({ baseUrl: "http://gw.test", key: KEY }), { fetch });

const TOOL_ANSWER = {
  id: "c2",
  model: "gpt-4o",
  choices: [
    {
      message: {
        role: "assistant",
        content: null,
        tool_calls: [{ id: "call_1", type: "function", function: { name: "weather", arguments: '{"city":"Paris"}' } }],
      },
      finish_reason: "tool_calls",
    },
  ],
  usage: { prompt_tokens: 9, completion_tokens: 4 },
};

const weather = { name: "weather", description: "Current weather", parameters: { type: "object", properties: { city: { type: "string" } } } };

const full: ChatRequest = {
  model: "gpt-4o",
  messages: [
    {
      role: "user",
      content: [
        { type: "text", text: "weather where this was taken?" },
        { type: "image", url: "data:image/png;base64,AAAA" },
      ],
    },
    { role: "assistant", content: null, toolCalls: [{ id: "call_1", name: "weather", arguments: '{"city":"Paris"}' }] },
    { role: "tool", content: "sunny", toolCallId: "call_1" },
  ],
  tools: [weather],
  toolChoice: { name: "weather" },
  parallelToolCalls: false,
};

describe("tools and images", () => {
  it("chat sends tools, images, tool calls and results, and returns tool calls", async () => {
    const { fetch, seen } = fakeFetch(json(200, TOOL_ANSWER));
    const r = await gw(fetch).chat(full);
    expect(r.toolCalls).toEqual([{ id: "call_1", name: "weather", arguments: '{"city":"Paris"}' }]);
    expect(r.finishReason).toBe("tool_calls");
    expect(r.content).toBe("");
    const body = JSON.parse(seen[0]!.body);
    expect(body.messages[0].content[1]).toEqual({ type: "image_url", image_url: { url: "data:image/png;base64,AAAA" } });
    expect(body.messages[1].tool_calls[0].function).toEqual({ name: "weather", arguments: '{"city":"Paris"}' });
    expect(body.messages[2]).toMatchObject({ role: "tool", tool_call_id: "call_1", content: "sunny" });
    expect(body.tools[0].function.name).toBe("weather");
    expect(body.tool_choice).toEqual({ type: "function", function: { name: "weather" } });
    expect(body.parallel_tool_calls).toBe(false);
  });

  it("strict reaches the wire when set and only then", async () => {
    const { fetch, seen } = fakeFetch(json(200, TOOL_ANSWER));
    await gw(fetch).chat({ model: "m", messages: [{ role: "user", content: "x" }], tools: [{ ...weather, strict: true }, { ...weather, name: "other" }] });
    const body = JSON.parse(seen[0]!.body);
    expect(body.tools[0].function.strict).toBe(true);
    expect(body.tools[1].function).not.toHaveProperty("strict");
  });

  it("a plain answer has no tool calls", async () => {
    const { fetch } = fakeFetch(json(200, { id: "c", model: "m", choices: [{ message: { role: "assistant", content: "hi" }, finish_reason: "stop" }] }));
    expect((await gw(fetch).chat({ model: "m", messages: [{ role: "user", content: "x" }] })).toolCalls).toEqual([]);
  });

  it("a direct provider target gets tools and images too", async () => {
    const { fetch, seen } = fakeFetch(
      json(200, { id: "m1", model: "claude", content: [{ type: "text", text: "ok" }], stop_reason: "end_turn", usage: { input_tokens: 1, output_tokens: 1 } }),
    );
    await new Client(anthropic({ key: KEY }), { fetch }).chat({
      model: "claude",
      messages: [{ role: "user", content: [{ type: "image", url: "data:image/png;base64,AAAA" }] }],
      tools: [weather],
    });
    const body = JSON.parse(seen[0]!.body);
    expect(body.tools[0].name).toBe("weather");
    expect(body.messages[0].content[0].type).toBe("image");
  });

  it("chatStream yields tool call events in order", async () => {
    const text = [
      sse({ choices: [{ delta: { tool_calls: [{ index: 0, id: "call_1", function: { name: "weather", arguments: "" } }] } }] }),
      sse({ choices: [{ delta: { tool_calls: [{ index: 0, function: { arguments: '{"city":' } }] } }] }),
      sse({ choices: [{ delta: { tool_calls: [{ index: 0, function: { arguments: '"Paris"}' } }] } }] }),
      sse({ choices: [{ delta: {}, finish_reason: "tool_calls" }], usage: { prompt_tokens: 9, completion_tokens: 4 } }),
      "data: [DONE]\n\n",
    ].join("");
    for (const split of ["whole", "byte"] as const) {
      const { fetch } = fakeFetch(streamResponse(text, split));
      const events: StreamEvent[] = [];
      for await (const e of gw(fetch).chatStream(full)) events.push(e);
      expect(events).toEqual([
        { type: "tool_call_start", index: 0, id: "call_1", name: "weather" },
        { type: "tool_call_delta", index: 0, arguments: '{"city":' },
        { type: "tool_call_delta", index: 0, arguments: '"Paris"}' },
        { type: "done", finishReason: "tool_calls", usage: { inputTokens: 9, outputTokens: 4 } },
      ]);
    }
  });
});

describe("message checks", () => {
  const send = async (messages: unknown, extra: Record<string, unknown> = {}) => {
    const { fetch, seen } = fakeFetch(json(200, TOOL_ANSWER));
    try {
      await gw(fetch).chat({ model: "m", messages, ...extra } as unknown as ChatRequest);
    } catch (e) {
      return { error: e as UltrafastError, sent: seen.length };
    }
    return { error: undefined, sent: seen.length };
  };

  it("a tool message without toolCallId is refused before sending", async () => {
    const { error, sent } = await send([{ role: "tool", content: "sunny" }]);
    expect(error).toBeInstanceOf(UltrafastError);
    expect(error!.kind).toBe("invalid_request");
    expect(error!.message).toMatch(/toolCallId/);
    expect(sent).toBe(0);
  });

  it("other bad shapes are invalid_request and never sent", async () => {
    for (const messages of [
      [{ role: "robot", content: "x" }],
      [{ role: "user", content: 5 }],
      [{ role: "user", content: [{ type: "audio", url: "x" }] }],
      [{ role: "user", content: [{ type: "image", url: 5 }] }],
      [{ role: "user", content: [{ type: "text" }] }],
      [{ role: "user", content: null }],
      [{ role: "assistant", content: null, toolCalls: [{ id: "a", name: "n" }] }],
      [{ role: "assistant", content: null, toolCalls: "x" }],
      [{ role: "user", content: "x", toolCallId: "c" }],
      [{ role: "user", content: [{ type: "image", url: "ftp://x/a.png" }] }],
    ]) {
      const { error, sent } = await send(messages);
      expect(error?.kind, JSON.stringify(messages)).toBe("invalid_request");
      expect(sent).toBe(0);
    }
  });

  it("a bad toolChoice is invalid_request", async () => {
    const { error } = await send([{ role: "user", content: "x" }], { tools: [weather], toolChoice: "sometimes" });
    expect(error?.kind).toBe("invalid_request");
  });

  it("the three toolChoice words and a tool name are accepted", async () => {
    for (const toolChoice of ["auto", "none", "required", { name: "weather" }]) {
      const { error } = await send([{ role: "user", content: "x" }], { tools: [weather], toolChoice });
      expect(error, JSON.stringify(toolChoice)).toBeUndefined();
    }
  });
});
