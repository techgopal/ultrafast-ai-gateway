import { describe, expect, test } from "vitest";
import type { Message } from "@/lib/playground";
import { nameOf } from "./PlaygroundThread";

const call = (id: string, name: string) => ({ id, type: "function" as const, function: { name, arguments: "{}" } });

describe("nameOf", () => {
  const thread: Message[] = [
    { role: "user", content: "x" },
    { role: "assistant", content: null, tool_calls: [call("call_0", "first")] },
    { role: "tool", content: "a", tool_call_id: "call_0" },
    { role: "assistant", content: null, tool_calls: [call("call_0", "second")] },
    { role: "tool", content: "b", tool_call_id: "call_0" },
  ];

  test("a result is named after the latest call with its id before it", () => {
    expect(nameOf(thread, "call_0", 2)).toBe("first");
    expect(nameOf(thread, "call_0", 4)).toBe("second");
    expect(nameOf(thread, "call_0")).toBe("second");
  });

  test("a result no call answers has no name", () => {
    expect(nameOf(thread, "other", 4)).toBeUndefined();
    expect(nameOf(thread, "call_0", 1)).toBeUndefined();
  });
});
