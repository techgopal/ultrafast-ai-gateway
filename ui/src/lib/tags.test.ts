import { describe, expect, test } from "vitest";
import { ConsoleRefusal } from "@/api/errors";
import {
  BAD_TAG_FILTER,
  chipsOf,
  MAX_TAGS,
  parseTagFilter,
  rowsOf,
  tagProblem,
  tagsOf,
  TAG_PROBLEMS,
} from "@/lib/tags";

const row = (name: string, value: string) => ({ name, value });

describe("the rules of a tag, as the gateway has them", () => {
  test("a good set has no problem; rows left empty are not there", () => {
    expect(tagProblem([])).toBeNull();
    expect(tagProblem([row("team", "platform"), row("Az09_.:-", "any value é")])).toBeNull();
    expect(tagProblem([row("", ""), row("a", "b"), row("  ", "")])).toBeNull();
    const edge = [row("n".repeat(64), "v".repeat(64))];
    expect(tagProblem(edge)).toBeNull();
    const twenty = Array.from({ length: MAX_TAGS }, (_, i) => row(`k${i}`, "v"));
    expect(tagProblem(twenty)).toBeNull();
  });

  test.each([
    ["more than 20", Array.from({ length: 21 }, (_, i) => row(`k${i}`, "v")), TAG_PROBLEMS.count],
    ["a name without a value", [row("a", "")], TAG_PROBLEMS.empty],
    ["a value without a name", [row("", "v")], TAG_PROBLEMS.empty],
    ["a long name", [row("n".repeat(65), "v")], TAG_PROBLEMS.long],
    ["a long value", [row("a", "v".repeat(65))], TAG_PROBLEMS.long],
    ["a space in a name", [row("a b", "v")], TAG_PROBLEMS.charset],
    ["a letter outside ASCII in a name", [row("é", "v")], TAG_PROBLEMS.charset],
    ["a name twice", [row("a", "1"), row("a", "2")], TAG_PROBLEMS.twice],
  ])("%s is refused", (_, rows, problem) => {
    expect(tagProblem(rows)).toBe(problem);
  });

  test("64 characters count as characters, not as bytes", () => {
    expect(tagProblem([row("a", "é".repeat(64))])).toBeNull();
    expect(tagProblem([row("a", "😀".repeat(64))])).toBeNull();
    expect(tagProblem([row("a", "😀".repeat(65))])).toBe(TAG_PROBLEMS.long);
  });

  test("tagsOf gives the object to send, and refuses on the field", () => {
    expect(tagsOf([row("b", "2"), row("", ""), row("a", "1")])).toEqual({ a: "1", b: "2" });
    expect(tagsOf([])).toEqual({});
    try {
      tagsOf([row("a b", "v")]);
      expect.unreachable();
    } catch (error) {
      expect(error).toBeInstanceOf(ConsoleRefusal);
      expect((error as ConsoleRefusal).field).toBe("tags");
      expect((error as ConsoleRefusal).message).toBe(TAG_PROBLEMS.charset);
    }
  });

  test("rows are the tags sorted by name, and chips say name:value", () => {
    expect(rowsOf({ b: "2", a: "1" })).toEqual([row("a", "1"), row("b", "2")]);
    expect(rowsOf({})).toEqual([]);
    expect(chipsOf({ team: "platform", env: "prod" })).toEqual(["env:prod", "team:platform"]);
    expect(chipsOf({})).toEqual([]);
  });
});

describe("the filter of the logs", () => {
  test("name:value, the name ends at the first colon", () => {
    expect(parseTagFilter("env:prod")).toEqual({ tag: "env:prod" });
    expect(parseTagFilter("  env:prod  ")).toEqual({ tag: "env:prod" });
    expect(parseTagFilter("url:http://x:80")).toEqual({ tag: "url:http://x:80" });
    expect(parseTagFilter("a:b c")).toEqual({ tag: "a:b c" });
  });

  test.each(["env", ":v", "env:", "a b:v", "é:v", `${"n".repeat(65)}:v`, `a:${"v".repeat(65)}`, ""])(
    "%j is no tag",
    (text) => {
      expect(parseTagFilter(text)).toEqual({ problem: BAD_TAG_FILTER });
    },
  );
});
