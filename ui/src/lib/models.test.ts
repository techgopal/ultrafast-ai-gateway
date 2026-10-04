import { describe, expect, test } from "vitest";
import { ConsoleRefusal } from "@/api/errors";
import * as fixtures from "@/test/fixtures";
import {
  ANY,
  accessSummary,
  chosen,
  grantsOf,
  hasNoAccess,
  matches,
  PRICE_RULE,
  priceFormOf,
  pricesRequestOf,
  priceText,
  providerChoices,
  refOf,
  sortModels,
  syncText,
} from "./models";

const { openaiMini, openaiFull, openaiDisabled, localLlama } = fixtures.models;
const none = { everyone: false, team_ids: [], user_ids: [] };

describe("accessSummary", () => {
  test.each([
    [{ everyone: true, team_ids: [], user_ids: [] }, "Everyone"],
    [none, "No one"],
    [{ everyone: false, team_ids: [1], user_ids: [] }, "1 team"],
    [{ everyone: false, team_ids: [], user_ids: [4] }, "1 user"],
    [{ everyone: false, team_ids: [1, 2], user_ids: [5] }, "2 teams, 1 user"],
    [{ everyone: false, team_ids: [1], user_ids: [5, 6] }, "1 team, 2 users"],
  ])("%j is %s", (grants, text) => {
    expect(accessSummary(grants)).toBe(text);
  });
});

test("nobody has access when no grant is made", () => {
  expect(hasNoAccess(none)).toBe(true);
  expect(hasNoAccess({ everyone: true, team_ids: [], user_ids: [] })).toBe(false);
  expect(hasNoAccess({ everyone: false, team_ids: [1], user_ids: [] })).toBe(false);
  expect(hasNoAccess({ everyone: false, team_ids: [], user_ids: [3] })).toBe(false);
});

test("a model is called as provider/model", () => {
  expect(refOf(openaiMini)).toBe("openai/gpt-4o-mini");
  expect(refOf(localLlama)).toBe("local-llm/llama3.1:8b");
});

describe("sortModels", () => {
  test("by provider, then by name, without changing the list", () => {
    const list = [localLlama, openaiMini, openaiDisabled, openaiFull];
    const copy = [...list];
    expect(sortModels(list).map((m) => m.name)).toEqual([
      "llama3.1:8b",
      "gpt-4o",
      "gpt-4o-mini",
      "o3-mini",
    ]);
    expect(list).toEqual(copy);
  });
});

describe("matches", () => {
  const all = { search: "", provider: ANY, status: ANY };
  test("nothing filtered leaves every model", () => {
    expect(fixtures.modelList.every((m) => matches(m, all))).toBe(true);
  });
  test("the text is looked for in the name and the provider, without regard to case", () => {
    expect(matches(openaiMini, { ...all, search: " MINI " })).toBe(true);
    expect(matches(openaiFull, { ...all, search: "mini" })).toBe(false);
    expect(matches(localLlama, { ...all, search: "local" })).toBe(true);
    expect(matches(openaiFull, { ...all, search: "local" })).toBe(false);
  });
  test("the provider and the status", () => {
    expect(matches(openaiMini, { ...all, provider: String(openaiMini.provider_id) })).toBe(true);
    expect(matches(localLlama, { ...all, provider: String(openaiMini.provider_id) })).toBe(false);
    expect(matches(openaiMini, { ...all, status: "enabled" })).toBe(true);
    expect(matches(openaiMini, { ...all, status: "disabled" })).toBe(false);
    expect(matches(openaiDisabled, { ...all, status: "disabled" })).toBe(true);
  });
});

test("the providers to choose from are those of the models, by name", () => {
  expect(providerChoices(fixtures.modelList)).toEqual([
    { value: ANY, label: "All providers" },
    { value: "2", label: "local-llm" },
    { value: "1", label: "openai" },
  ]);
});

test("a choice that is gone filters nothing", () => {
  const choices = providerChoices(fixtures.modelList);
  expect(chosen("1", choices)).toBe("1");
  expect(chosen("9", choices)).toBe(ANY);
});

describe("syncText", () => {
  test.each([
    [{ added: ["a", "b"], existing: 0 }, "Added 2 models. They start disabled."],
    [{ added: ["a"], existing: 3 }, "Added 1 model. It starts disabled."],
    [{ added: [], existing: 5 }, "No new models."],
  ])("%j", (result, text) => {
    expect(syncText(result)).toBe(text);
  });
});

describe("grantsOf", () => {
  const teams = [1, 2];
  const users = [3, 5];
  test("everyone sends no team and no user", () => {
    expect(grantsOf({ everyone: true, team_ids: ["1"], user_ids: ["3"] }, teams, users)).toEqual({
      everyone: true,
      team_ids: [],
      user_ids: [],
    });
  });
  test("a team or a user that is no longer offered is not sent", () => {
    expect(
      grantsOf({ everyone: false, team_ids: ["1", "9"], user_ids: ["5", "8"] }, teams, users),
    ).toEqual({ everyone: false, team_ids: [1], user_ids: [5] });
  });
});

describe("prices", () => {
  test("a price is said in dollars, and unknown says so", () => {
    expect([priceText(2_500_000), priceText(0), priceText(1), priceText(null)]).toEqual([
      "$2.50",
      "$0.00",
      "$0.000001",
      "Not set",
    ]);
  });

  test("the form starts from the model, an unknown price empty", () => {
    expect(priceFormOf({ input_price_micros: 2_500_000, output_price_micros: null })).toEqual({
      input_price_micros: "2.5",
      output_price_micros: "",
    });
  });

  test("the request has both prices in micros; empty is null; 0 is a price", () => {
    expect(pricesRequestOf({ input_price_micros: " 0 ", output_price_micros: "" })).toEqual({
      input_price_micros: 0,
      output_price_micros: null,
    });
    expect(pricesRequestOf({ input_price_micros: "0.15", output_price_micros: "12.345678" })).toEqual({
      input_price_micros: 150_000,
      output_price_micros: 12_345_678,
    });
  });

  test.each(["-1", "abc", "1.2345678", "1e3"])("%j is refused on its field", (text) => {
    try {
      pricesRequestOf({ input_price_micros: "1", output_price_micros: text });
    } catch (error) {
      expect(error).toBeInstanceOf(ConsoleRefusal);
      expect(error).toMatchObject({ message: PRICE_RULE, field: "output_price_micros" });
      return;
    }
    throw new Error("nothing was refused");
  });
});
