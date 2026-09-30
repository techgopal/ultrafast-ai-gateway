// @vitest-environment node
import { describe, expect, test } from "vitest";
import { idOf } from "./id";

describe("the id a text stands for", () => {
  test.each([
    ["1", 1],
    ["7", 7],
    ["42", 42],
    ["1000", 1000],
    ["9007199254740991", Number.MAX_SAFE_INTEGER],
  ])("%j is the id %d", (text, id) => {
    expect(idOf(text)).toBe(id);
  });

  test.each([
    ["nothing", ""],
    ["zero", "0"],
    ["a negative number", "-1"],
    ["a sign", "+1"],
    ["a fraction", "1.5"],
    ["a word", "abc"],
    ["a leading zero", "01"],
    ["an exponent", "1e3"],
    ["a hexadecimal number", "0x10"],
    ["a space", " "],
    ["a space before", " 1"],
    ["a space after", "1 "],
    ["an encoded space", "%20"],
    ["a number and more", "12abc"],
    ["a digit of another script", "１"],
    ["more than a safe integer", "9007199254740992"],
    ["more digits than an id has", "99999999999999999999"],
    ["a path", "1/2"],
  ])("%s is no id: %j", (_, text) => {
    expect(idOf(text)).toBeNull();
  });
});
