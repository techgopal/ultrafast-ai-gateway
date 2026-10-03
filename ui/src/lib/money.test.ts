import { describe, expect, test } from "vitest";
import { dollarsToMicros, formatDollars, microsToDollars } from "@/lib/money";

describe("dollarsToMicros", () => {
  test.each([
    ["0", 0],
    ["2", 2_000_000],
    ["2.5", 2_500_000],
    ["2.50", 2_500_000],
    ["0.15", 150_000],
    ["0.000001", 1],
    ["12.345678", 12_345_678],
    ["0.1", 100_000],
    ["1234567.1", 1_234_567_100_000],
  ])("%s is %i micros", (text, micros) => {
    expect(dollarsToMicros(text)).toBe(micros);
  });

  test("a float never decides: 0.29 and 1.005 convert exactly", () => {
    expect(dollarsToMicros("0.29")).toBe(290_000);
    expect(dollarsToMicros("1.005")).toBe(1_005_000);
    expect(dollarsToMicros("4.35")).toBe(4_350_000);
  });

  test.each(["", " ", "-1", "1.", ".5", "1.2345678", "abc", "1e3", "1,5", "$2", "2 .5", "0x10", "+1"])(
    "%j is not dollars",
    (text) => {
      expect(dollarsToMicros(text)).toBeNull();
    },
  );

  test("surrounding spaces are ignored", () => {
    expect(dollarsToMicros(" 2.5 ")).toBe(2_500_000);
  });

  test("what is beyond a safe integer of micros is refused", () => {
    expect(dollarsToMicros("9007199254")).toBe(9_007_199_254_000_000);
    expect(dollarsToMicros("9007199255")).toBeNull();
  });
});

describe("microsToDollars", () => {
  test.each([
    [0, "0"],
    [2_000_000, "2"],
    [2_500_000, "2.5"],
    [150_000, "0.15"],
    [1, "0.000001"],
    [12_345_678, "12.345678"],
  ])("%i micros is %s dollars", (micros, text) => {
    expect(microsToDollars(micros)).toBe(text);
  });

  test("it undoes dollarsToMicros", () => {
    for (const text of ["0", "2.5", "0.000001", "99.99", "1234.5"]) {
      expect(microsToDollars(dollarsToMicros(text) ?? -1)).toBe(text);
    }
  });
});

describe("formatDollars", () => {
  test.each([
    [0, "$0.00"],
    [2_500_000, "$2.50"],
    [2_000_000, "$2.00"],
    [150_000, "$0.15"],
    [1, "$0.000001"],
    [12_345_678, "$12.345678"],
    [1_234_567_000_000, "$1,234,567.00"],
    [500, "$0.0005"],
  ])("%i micros reads %s", (micros, text) => {
    expect(formatDollars(micros)).toBe(text);
  });
});
