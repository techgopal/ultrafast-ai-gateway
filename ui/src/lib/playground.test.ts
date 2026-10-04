import { describe, expect, test } from "vitest";
import {
  checkParams,
  chunkOf,
  costMicros,
  curlOf,
  requestBody,
  retryText,
  SseReader,
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
    [90, " Try again in 2 minutes."],
    [3600, " Try again in 60 minutes."],
  ])("%j", (seconds, text) => {
    expect(retryText(seconds)).toBe(text);
  });
});
