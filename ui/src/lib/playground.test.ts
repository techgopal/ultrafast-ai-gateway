import { describe, expect, test } from "vitest";
import {
  checkImageCount,
  checkParams,
  checkResponseFormat,
  checkTools,
  chunkOf,
  costMicros,
  curlOf,
  IMAGE_COUNT_INVALID,
  imageCurlOf,
  imageRequestBody,
  imageUrlsOf,
  requestBody,
  retryText,
  SseReader,
  ToolCallAssembler,
  type Params,
} from "@/lib/playground";

const empty: Params = { maxTokens: "", temperature: "", topP: "", stop: "" };

describe("checkParams", () => {
  test("nothing typed sends nothing", () => {
    expect(checkParams(empty)).toEqual({ values: {}, errors: {} });
  });

  test("numbers and stop sequences are read", () => {
    expect(
      checkParams({ maxTokens: " 256 ", temperature: "0.7", topP: "1", stop: "END, ###,, " }),
    ).toEqual({
      values: { max_tokens: 256, temperature: 0.7, top_p: 1, stop: ["END", "###"] },
      errors: {},
    });
  });

  test.each([
    ["maxTokens", "0"],
    ["maxTokens", "-3"],
    ["maxTokens", "1.5"],
    ["maxTokens", "abc"],
    ["maxTokens", "99999999999"],
    ["temperature", "-0.1"],
    ["temperature", "2.1"],
    ["temperature", "hot"],
    ["topP", "1.1"],
    ["topP", "-1"],
    ["topP", "x"],
  ] as const)("%s %j is refused on its field", (field, text) => {
    const checked = checkParams({ ...empty, [field]: text });
    expect(Object.keys(checked.errors)).toEqual([field]);
    expect(checked.values).toEqual({});
  });

  test("at most four stop sequences, as the providers allow", () => {
    expect(checkParams({ ...empty, stop: "a,b,c,d" }).errors).toEqual({});
    expect(checkParams({ ...empty, stop: "a,b,c,d,e" }).errors.stop).toBeDefined();
  });

  test("the bounds are allowed", () => {
    expect(checkParams({ ...empty, temperature: "0", topP: "0", maxTokens: "1" }).errors).toEqual(
      {},
    );
    expect(checkParams({ ...empty, temperature: "2", topP: "1" }).errors).toEqual({});
  });
});

describe("requestBody", () => {
  test("a system prompt goes first, and the stream is asked for", () => {
    expect(
      requestBody(
        "openai/gpt-4o-mini",
        "  Be brief.  ",
        [
          { role: "user", content: "hi" },
          { role: "assistant", content: "hello" },
          { role: "user", content: "and?" },
        ],
        { max_tokens: 10 },
      ),
    ).toEqual({
      model: "openai/gpt-4o-mini",
      stream: true,
      max_tokens: 10,
      messages: [
        { role: "system", content: "Be brief." },
        { role: "user", content: "hi" },
        { role: "assistant", content: "hello" },
        { role: "user", content: "and?" },
      ],
    });
  });

  test("a blank system prompt is left out", () => {
    const body = requestBody("route", "   ", [{ role: "user", content: "hi" }], {});
    expect(body.messages).toEqual([{ role: "user", content: "hi" }]);
  });
});

describe("SseReader", () => {
  test("events come out whole, whatever the chunks cut", () => {
    const reader = new SseReader();
    expect(reader.feed('data: {"a":1}\n\nda')).toEqual(['{"a":1}']);
    expect(reader.feed('ta: {"b":')).toEqual([]);
    expect(reader.feed('2}\n\ndata: [DONE]\n\n')).toEqual(['{"b":2}', "[DONE]"]);
  });

  test("CRLF line ends, comments and other fields are handled", () => {
    const reader = new SseReader();
    expect(reader.feed(": ping\r\n\r\nevent: x\r\ndata: one\r\n\r\n")).toEqual(["one"]);
  });

  test("an event of several data lines is joined with a newline", () => {
    expect(new SseReader().feed("data: a\ndata: b\n\n")).toEqual(["a\nb"]);
  });
});

describe("chunkOf", () => {
  test("a delta", () => {
    expect(chunkOf(JSON.stringify({ choices: [{ delta: { content: "hel" } }] }))).toEqual({
      text: "hel",
    });
  });

  test("the usage", () => {
    expect(
      chunkOf(
        JSON.stringify({
          model: "gpt-4o-mini",
          choices: [],
          usage: { prompt_tokens: 2, completion_tokens: 3, total_tokens: 5 },
        }),
      ),
    ).toEqual({ usage: { input: 2, output: 3 }, model: "gpt-4o-mini" });
  });

  test("done, an error, and what is not understood", () => {
    expect(chunkOf("[DONE]")).toEqual({ done: true });
    // A guardrail ended the answer.
    expect(chunkOf(JSON.stringify({ choices: [{ delta: {}, finish_reason: "content_filter" }] }))).toEqual({
      blocked: true,
    });
    expect(chunkOf(JSON.stringify({ choices: [{ delta: {}, finish_reason: "stop" }] }))).toEqual({});
    expect(chunkOf(JSON.stringify({ error: { message: "upstream failed", type: "upstream_error" } }))).toEqual({
      error: "upstream failed",
    });
    expect(chunkOf("not json")).toEqual({});
    expect(chunkOf(JSON.stringify({ choices: [{ delta: {} }] }))).toEqual({});
    expect(chunkOf(JSON.stringify({ error: 5 }))).toEqual({ error: "The answer broke off." });
  });
});

describe("costMicros", () => {
  test("tokens times the price per million, rounded to a micro", () => {
    expect(
      costMicros({ input: 1000, output: 500 }, { input_price_micros: 150_000, output_price_micros: 600_000 }),
    ).toBe(450);
    expect(costMicros({ input: 1, output: 1 }, { input_price_micros: 150_000, output_price_micros: 600_000 })).toBe(1);
  });

  test("a price that is not known makes the cost not known", () => {
    expect(costMicros({ input: 1, output: 1 }, { input_price_micros: null, output_price_micros: 5 })).toBeNull();
    expect(costMicros({ input: 1, output: 1 }, null)).toBeNull();
  });
});

describe("curlOf", () => {
  test("the gateway's URL, a placeholder for the key, the same parameters", () => {
    const text = curlOf("https://gw.example.com", {
      model: "openai/gpt-4o-mini",
      stream: true,
      max_tokens: 10,
      messages: [{ role: "user", content: "it's" }],
    });
    expect(text).toContain("curl https://gw.example.com/v1/chat/completions");
    expect(text).toContain("-H 'Authorization: Bearer <your key>'");
    expect(text).toContain("-H 'Content-Type: application/json'");
    // A quote inside the body is closed, escaped and opened again.
    expect(text).toContain(`'"'"'`);
    expect(text).toContain('"model":"openai/gpt-4o-mini"');
    expect(text).not.toMatch(/uf-key|cookie|csrf/i);
  });
});

describe("retryText", () => {
  test.each([
    [null, ""],
    [1, " Try again in 1 second."],
    [30, " Try again in 30 seconds."],
    [59, " Try again in 59 seconds."],
    [60, " Try again in 1 minute."],
    [90, " Try again in 2 minutes."],
    [3600, " Try again in 60 minutes."],
  ])("%j", (seconds, text) => {
    expect(retryText(seconds)).toBe(text);
  });
});

const delta = (calls: object[]) =>
  chunkOf(JSON.stringify({ choices: [{ index: 0, delta: { tool_calls: calls } }] }));

describe("tool calls in a stream", () => {
  test("a chunk with tool call deltas is read, the text stays absent", () => {
    expect(delta([{ index: 0, id: "c1", type: "function", function: { name: "f", arguments: "" } }])).toEqual({
      toolCalls: [{ index: 0, id: "c1", name: "f", arguments: "" }],
    });
  });

  test("two calls whose pieces are interleaved are put together by index", () => {
    const assembler = new ToolCallAssembler();
    const feed = (calls: object[]) => {
      assembler.add(delta(calls).toolCalls ?? []);
    };
    feed([{ index: 0, id: "call_a", type: "function", function: { name: "weather", arguments: "" } }]);
    feed([{ index: 0, function: { arguments: '{"city":' } }]);
    feed([{ index: 1, id: "call_b", type: "function", function: { name: "time", arguments: '{"tz"' } }]);
    feed([{ index: 0, function: { arguments: '"Oslo"}' } }]);
    feed([{ index: 1, function: { arguments: ':"CET"}' } }]);
    expect(assembler.calls()).toEqual([
      { id: "call_a", type: "function", function: { name: "weather", arguments: '{"city":"Oslo"}' } },
      { id: "call_b", type: "function", function: { name: "time", arguments: '{"tz":"CET"}' } },
    ]);
  });

  test("two calls whose pieces come in one chunk are both kept", () => {
    const assembler = new ToolCallAssembler();
    assembler.add(
      delta([
        { index: 0, id: "a", type: "function", function: { name: "f", arguments: "{" } },
        { index: 1, id: "b", type: "function", function: { name: "g", arguments: "[" } },
      ]).toolCalls ?? [],
    );
    assembler.add(delta([{ index: 0, function: { arguments: "}" } }, { index: 1, function: { arguments: "]" } }]).toolCalls ?? []);
    expect(assembler.calls().map((call) => [call.id, call.function.arguments])).toEqual([
      ["a", "{}"],
      ["b", "[]"],
    ]);
  });

  test("no deltas, no calls; a call that never got an id is not made up", () => {
    const assembler = new ToolCallAssembler();
    expect(assembler.calls()).toEqual([]);
    assembler.add([{ index: 0, arguments: "{}" }]);
    expect(assembler.calls()).toEqual([]);
  });
});

describe("checkTools", () => {
  const tool = { type: "function", function: { name: "weather", parameters: { type: "object" } } };

  test("nothing typed is no tools", () => {
    expect(checkTools("  ")).toEqual({ tools: undefined, names: [], error: undefined });
    expect(checkTools("[]")).toEqual({ tools: undefined, names: [], error: undefined });
  });

  test("an array of functions is read, with the names", () => {
    const checked = checkTools(JSON.stringify([tool, { ...tool, function: { name: "time" } }]));
    expect(checked.error).toBeUndefined();
    expect(checked.names).toEqual(["weather", "time"]);
    expect(checked.tools).toHaveLength(2);
  });

  test.each([
    ["not json", "{"],
    ["an object", '{"type":"function"}'],
    ["a number in the array", "[1]"],
    ["no function", '[{"type":"function"}]'],
    ["another type", '[{"type":"retrieval","function":{"name":"a"}}]'],
    ["no name", '[{"type":"function","function":{}}]'],
    ["an empty name", '[{"type":"function","function":{"name":""}}]'],
  ])("%s is refused", (_why, text) => {
    const checked = checkTools(text);
    expect(checked.error).toBe("Tools must be a JSON array of functions, each with a type of function and a name.");
    expect(checked.tools).toBeUndefined();
  });
});

describe("the request with tools and images", () => {
  test("an image is sent as content parts, the text first", () => {
    const body = requestBody(
      "m",
      "",
      [{ role: "user", content: "what is this", images: [{ name: "a.png", url: "data:image/png;base64,AAAA" }] }],
      {},
    );
    expect(body.messages).toEqual([
      {
        role: "user",
        content: [
          { type: "text", text: "what is this" },
          { type: "image_url", image_url: { url: "data:image/png;base64,AAAA" } },
        ],
      },
    ]);
  });

  test("tool calls and results go as the OpenAI shape, null content with calls", () => {
    const call = { id: "c1", type: "function" as const, function: { name: "f", arguments: "{}" } };
    const body = requestBody(
      "m",
      "",
      [
        { role: "user", content: "go" },
        { role: "assistant", content: null, tool_calls: [call] },
        { role: "tool", content: "42", tool_call_id: "c1" },
      ],
      { tool_choice: "required" },
    );
    expect(body.tool_choice).toBe("required");
    expect(body.messages).toEqual([
      { role: "user", content: "go" },
      { role: "assistant", content: null, tool_calls: [call] },
      { role: "tool", content: "42", tool_call_id: "c1" },
    ]);
  });

  test("curl abbreviates the image data and says so", () => {
    const text = curlOf("https://gw", {
      messages: [
        {
          role: "user",
          content: [{ type: "image_url", image_url: { url: "data:image/png;base64,AAAAAAAA" } }],
        },
      ],
      tools: [{ type: "function", function: { name: "f" } }],
    });
    expect(text).toContain("data:image/png;base64,…");
    expect(text).not.toContain("AAAAAAAA");
    expect(text).toContain('"tools":[{"type":"function","function":{"name":"f"}}]');
    expect(text).toContain("# image data omitted");
  });

  test("without an image there is no note", () => {
    expect(curlOf("https://gw", { messages: [{ role: "user", content: "x" }] })).not.toContain("omitted");
  });
});

describe("checkResponseFormat", () => {
  test("text sends nothing, JSON sends json_object", () => {
    expect(checkResponseFormat("text", "ignored")).toEqual({ format: undefined, error: undefined });
    expect(checkResponseFormat("json_object", "ignored")).toEqual({
      format: { type: "json_object" },
      error: undefined,
    });
  });

  test("a JSON schema is read from its text", () => {
    const schema = { type: "object", properties: { a: { type: "string" } } };
    expect(checkResponseFormat("json_schema", JSON.stringify(schema))).toEqual({
      format: { type: "json_schema", json_schema: { name: "response", schema } },
      error: undefined,
    });
  });

  test.each([["empty", "  "], ["not json", "{"], ["an array", "[]"], ["a string", '"x"'], ["null", "null"]])(
    "a schema that is %s is refused",
    (_why, text) => {
      expect(checkResponseFormat("json_schema", text)).toEqual({
        format: undefined,
        error: "The schema must be a JSON object, such as {\"type\":\"object\"}.",
      });
    },
  );
});

describe("images mode", () => {
  test("the number of images is a whole number from 1 to 4", () => {
    expect(checkImageCount(" 3 ")).toEqual({ n: 3 });
    for (const bad of ["", "0", "5", "1.5", "-1", "x", "1e1"]) {
      expect(checkImageCount(bad).error).toBe(IMAGE_COUNT_INVALID);
    }
  });

  test("the body leaves out the size when the provider decides", () => {
    expect(imageRequestBody("p/m", "a fox", 2, "1536x1024")).toEqual({ model: "p/m", prompt: "a fox", n: 2, size: "1536x1024" });
    expect(imageRequestBody("p/m", "a fox", 1, "default")).toEqual({ model: "p/m", prompt: "a fox", n: 1 });
  });

  test("only base64 images become data URLs, in the format the answer names", () => {
    expect(
      imageUrlsOf({ data: [{ b64_json: "AAAA" }, { url: "https://x.example/a.png" }, { b64_json: "<script>" }], output_format: "webp" }),
    ).toEqual(["data:image/webp;base64,AAAA"]);
    expect(imageUrlsOf({ data: [{ b64_json: "AA" }], output_format: "svg+xml" })).toEqual(["data:image/png;base64,AA"]);
  });

  test("the curl command quotes the prompt", () => {
    const command = imageCurlOf("https://gw.example", { model: "m", prompt: "it's" });
    expect(command).toContain("curl https://gw.example/v1/images/generations");
    expect(command).toContain(`-d '{"model":"m","prompt":"it'"'"'s"}'`);
  });
});
