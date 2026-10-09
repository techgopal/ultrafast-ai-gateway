import { describe, expect, test } from "vitest";
import {
  check,
  diffLines,
  diffMessages,
  draftOf,
  emptyDraft,
  hasProblems,
  requestOf,
  versionRequestOf,
  variablesIn,
  type Draft,
} from "./prompts";

const filled = (over: Partial<Draft> = {}): Draft => ({
  ...emptyDraft(),
  name: "summarize",
  messages: [{ key: 1, role: "user", content: "Summarize {{text}}" }],
  ...over,
});

describe("variablesIn", () => {
  test("only exact braces make a variable, as in the gateway", () => {
    expect(variablesIn(["{{a}} {{ b }} {{}} {{1}} {a} {{{c}}} {{d} {{e}}"])).toEqual(["a", "c", "e"]);
  });

  test("names are collected once from every message, sorted", () => {
    expect(variablesIn(["{{b}} {{a}}", "{{a}} {{_c1}}"])).toEqual(["_c1", "a", "b"]);
  });

  test("a name is 1 to 64 characters", () => {
    expect(variablesIn([`{{${"a".repeat(64)}}}`])).toEqual(["a".repeat(64)]);
    expect(variablesIn([`{{${"a".repeat(65)}}}`])).toEqual([]);
  });

  test("a name is case sensitive and stays as it was written", () => {
    expect(variablesIn(["{{Name}} {{name}}"])).toEqual(["Name", "name"]);
  });
});

describe("diffLines", () => {
  test("equal texts are all the same", () => {
    expect(diffLines(["a", "b"], ["a", "b"])).toEqual([
      { kind: "same", text: "a" },
      { kind: "same", text: "b" },
    ]);
  });

  test("an added, a removed and a changed line", () => {
    expect(diffLines(["a", "b", "c"], ["a", "x", "c", "d"])).toEqual([
      { kind: "same", text: "a" },
      { kind: "remove", text: "b" },
      { kind: "add", text: "x" },
      { kind: "same", text: "c" },
      { kind: "add", text: "d" },
    ]);
  });

  test("from nothing and to nothing", () => {
    expect(diffLines([], ["a"])).toEqual([{ kind: "add", text: "a" }]);
    expect(diffLines(["a"], [])).toEqual([{ kind: "remove", text: "a" }]);
  });

  test("a large text is compared without a stall", () => {
    const before = Array.from({ length: 3000 }, (_, i) => `line ${String(i)}`);
    const after = [...before.slice(0, 1500), "changed", ...before.slice(1501)];
    const out = diffLines(before, after);
    expect(out.filter((line) => line.kind !== "same")).toEqual([
      { kind: "remove", text: "line 1500" },
      { kind: "add", text: "changed" },
    ]);
  });
});

describe("diffMessages", () => {
  test("a message is its role and then its lines", () => {
    const before = [{ role: "system", content: "Be brief." }, { role: "user", content: "Hi {{name}}" }];
    const after = [{ role: "system", content: "Be brief.\nBe kind." }, { role: "user", content: "Hi {{name}}" }];
    expect(diffMessages(before, after)).toEqual([
      { kind: "same", text: "system:" },
      { kind: "same", text: "Be brief." },
      { kind: "add", text: "Be kind." },
      { kind: "same", text: "user:" },
      { kind: "same", text: "Hi {{name}}" },
    ]);
  });

  test("a changed role shows as a removed and an added role line", () => {
    const out = diffMessages([{ role: "user", content: "x" }], [{ role: "assistant", content: "x" }]);
    expect(out).toEqual([
      { kind: "remove", text: "user:" },
      { kind: "add", text: "assistant:" },
      { kind: "same", text: "x" },
    ]);
  });
});

describe("check", () => {
  test("a good draft has no problems", () => {
    expect(hasProblems(check(filled()))).toBe(false);
  });

  test("the name is needed, trimmed, 100 characters, without @", () => {
    expect(check(filled({ name: "  " })).fields.name).toBe("Enter a name.");
    expect(check(filled({ name: "a".repeat(101) })).fields.name).toBe("The name is at most 100 characters.");
    expect(check(filled({ name: "a@b" })).fields.name).toBe(
      "The name cannot contain @: the logs write name@version.",
    );
    expect(check(filled({ name: "a".repeat(100) })).fields.name).toBeUndefined();
  });

  test("the description is at most 500 characters", () => {
    expect(check(filled({ description: "x".repeat(501) })).fields.description).toBe(
      "The description is at most 500 characters.",
    );
  });

  test("a message needs text, and the first problem of each row is named", () => {
    const out = check(
      filled({
        messages: [
          { key: 1, role: "user", content: "ok" },
          { key: 2, role: "user", content: "  " },
        ],
      }),
    );
    expect(out.rows).toEqual({ 2: "Write the message." });
  });

  test("at least one and at most 64 messages", () => {
    expect(check(filled({ messages: [] })).fields.messages).toBe("Add at least one message.");
    const many = Array.from({ length: 65 }, (_, key) => ({ key, role: "user" as const, content: "x" }));
    expect(check(filled({ messages: many })).fields.messages).toBe("At most 64 messages.");
  });

  test("a message is at most 64 KiB and all together 256 KiB, counted in bytes", () => {
    const big = "é".repeat(32 * 1024 + 1);
    expect(check(filled({ messages: [{ key: 1, role: "user", content: big }] })).rows[1]).toBe(
      "A message is at most 64 KiB.",
    );
    const four = Array.from({ length: 5 }, (_, key) => ({ key, role: "user" as const, content: "x".repeat(60 * 1024) }));
    expect(check(filled({ messages: four })).fields.messages).toBe("The messages together are at most 256 KiB.");
  });

  test("at most 64 variables", () => {
    const text = Array.from({ length: 65 }, (_, i) => `{{v${String(i)}}}`).join(" ");
    expect(check(filled({ messages: [{ key: 1, role: "user", content: text }] })).fields.messages).toBe(
      "At most 64 variables.",
    );
  });

  test("the model, when given, is 1 to 200 characters", () => {
    expect(check(filled({ model: "m".repeat(201) })).fields.model).toBe("The model is at most 200 characters.");
    expect(check(filled({ model: "openai/gpt-4o" })).fields.model).toBeUndefined();
  });

  test("temperature 0 to 2, top P 0 to 1, max tokens a whole number from 1", () => {
    const out = check(filled({ temperature: "3", topP: "1.5", maxTokens: "0" }));
    expect(out.fields.temperature).toBe("From 0 to 2.");
    expect(out.fields.topP).toBe("From 0 to 1.");
    expect(out.fields.maxTokens).toBe("A whole number from 1.");
    expect(check(filled({ maxTokens: "1.5" })).fields.maxTokens).toBe("A whole number from 1.");
    expect(check(filled({ temperature: "x" })).fields.temperature).toBe("From 0 to 2.");
    expect(hasProblems(check(filled({ temperature: "0", topP: "1", maxTokens: "1" })))).toBe(false);
  });
});

describe("requestOf", () => {
  test("a bare draft sends the name, the messages and nothing else", () => {
    expect(requestOf(filled())).toEqual({
      name: "summarize",
      messages: [{ role: "user", content: "Summarize {{text}}" }],
    });
  });

  test("the trimmed name, description, model and every setting", () => {
    expect(
      requestOf(
        filled({ name: " s ", description: " d ", model: " p/m ", temperature: "0.5", topP: "0.9", maxTokens: "100" }),
      ),
    ).toEqual({
      name: "s",
      description: "d",
      model: "p/m",
      params: { temperature: 0.5, top_p: 0.9, max_tokens: 100 },
      messages: [{ role: "user", content: "Summarize {{text}}" }],
    });
  });

  test("the response format of the version before is kept, not dropped", () => {
    const format = { type: "json_object" } as unknown as Record<string, never>;
    expect(requestOf(filled({ responseFormat: format })).params).toEqual({ response_format: format });
  });
});

describe("versionRequestOf", () => {
  test("a version has no name or description", () => {
    expect(versionRequestOf(filled({ name: "x", description: "d", model: "p/m" }))).toEqual({
      model: "p/m",
      messages: [{ role: "user", content: "Summarize {{text}}" }],
    });
  });
});

describe("draftOf", () => {
  test("a version is the start of the next one; its response format is kept", () => {
    const format = { type: "json_object" } as unknown as Record<string, never>;
    const draft = draftOf(
      { name: "s", description: "d" },
      {
        version: 2,
        messages: [{ role: "system", content: "Hi {{x}}" }],
        variables: ["x"],
        model: "p/m",
        params: { temperature: 0.2, max_tokens: 10, response_format: format },
        created_by: 1,
        created_at: "2026-09-30 12:00:00",
      },
    );
    expect(draft).toMatchObject({
      name: "s",
      description: "d",
      model: "p/m",
      temperature: "0.2",
      maxTokens: "10",
      topP: "",
      responseFormat: format,
      messages: [{ role: "system", content: "Hi {{x}}" }],
    });
  });
});
