import { describe, expect, test } from "vitest";
import {
  ARGUMENTS_NOT_AN_OBJECT,
  ARGUMENTS_NOT_JSON,
  argumentsOf,
  blocksOf,
  callOf,
  hintsOf,
  rpcErrorOf,
  skeletonOf,
} from "@/lib/playgroundMcp";

const tool = {
  input_schema: {
    type: "object",
    properties: {
      repo: { type: "string", description: "owner/name" },
      title: { type: "string" },
      draft: { type: "boolean" },
      labels: { type: ["array", "null"] },
    },
    required: ["repo", "draft"],
  },
};

describe("argumentsOf", () => {
  test("empty is none, an object is itself", () => {
    expect(argumentsOf("  ")).toEqual({ value: {} });
    expect(argumentsOf('{"a": 1}')).toEqual({ value: { a: 1 } });
  });

  test("anything else says why", () => {
    expect(argumentsOf("{")).toEqual({ error: ARGUMENTS_NOT_JSON });
    expect(argumentsOf("[1]")).toEqual({ error: ARGUMENTS_NOT_AN_OBJECT });
    expect(argumentsOf("3")).toEqual({ error: ARGUMENTS_NOT_AN_OBJECT });
    expect(argumentsOf("null")).toEqual({ error: ARGUMENTS_NOT_AN_OBJECT });
  });
});

describe("the hints of a schema", () => {
  test("one line for each argument, with what the schema says", () => {
    expect(hintsOf(tool)).toEqual([
      "repo (string, required): owner/name",
      "title (string)",
      "draft (boolean, required)",
      "labels (array or null)",
    ]);
    expect(hintsOf({ input_schema: {} })).toEqual([]);
    expect(hintsOf({ input_schema: { properties: { x: {} } } })).toEqual(["x"]);
  });

  test("the skeleton holds each required argument, empty", () => {
    expect(JSON.parse(skeletonOf(tool))).toEqual({ repo: "", draft: false });
    expect(skeletonOf({ input_schema: {} })).toBe("{}");
  });
});

describe("a result", () => {
  test("text is shown, anything else is named", () => {
    expect(
      blocksOf({
        content: [
          { type: "text", text: "hi" },
          { type: "image", data: "xx" },
          { type: "resource", resource: {} },
          { type: "audio" },
        ],
      }),
    ).toEqual([
      { kind: "text", text: "hi" },
      { kind: "other", label: "An image, not shown here." },
      { kind: "other", label: "An embedded resource, not shown here." },
      { kind: "other", label: "A block of an unknown kind." },
    ]);
    expect(blocksOf(undefined)).toEqual([]);
    expect(blocksOf({ content: "no" })).toEqual([]);
  });

  test("an error of the reply is its message", () => {
    expect(rpcErrorOf({ error: { code: -32602, message: "Unknown tool" } })).toBe("Unknown tool");
    expect(rpcErrorOf({ error: {} })).toBe("The call failed.");
    expect(rpcErrorOf({ result: {} })).toBeNull();
  });

  test("the call names the tool and its arguments", () => {
    expect(callOf("echo", { q: "hi" })).toEqual({
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: { name: "echo", arguments: { q: "hi" } },
    });
  });
});
