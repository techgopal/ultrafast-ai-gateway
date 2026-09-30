// @vitest-environment node
import { describe, expect, test } from "vitest";
import { ConsoleRefusal } from "@/api/errors";
import { apiKeyOf } from "./providers";

describe("the API key that is sent", () => {
  test("is the one typed, without the spaces around it, which nobody means", () => {
    expect(apiKeyOf("sk-1")).toBe("sk-1");
    expect(apiKeyOf("  sk-1 \n")).toBe("sk-1");
    expect(apiKeyOf("\tsk 1\t")).toBe("sk 1");
  });

  test("nothing typed is no key", () => {
    expect(apiKeyOf("")).toBe("");
  });

  test.each([" ", "   ", "\t", " \n "])(
    "a key of spaces only (%j) is refused by the console, on its field",
    (typed) => {
      expect(() => apiKeyOf(typed)).toThrow(ConsoleRefusal);
      try {
        apiKeyOf(typed);
      } catch (error) {
        expect(error).toBeInstanceOf(ConsoleRefusal);
        expect(error).toMatchObject({ message: "Enter an API key.", field: "api_key" });
      }
    },
  );
});
