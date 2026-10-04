import { describe, expect, test } from "vitest";
import { ApiError } from "@/api/errors";
import * as fixtures from "@/test/fixtures";
import {
  check,
  DEFAULTS,
  emptyForm,
  formOf,
  gatewayFields,
  hasProblems,
  healthFor,
  inFormWords,
  healthText,
  moved,
  requestOf,
  targetText,
  teamsText,
  type RouteForm,
} from "./routes";

const { support, research, legacy } = fixtures.routes;
const offered = { models: [1, 2, 3, 4], teams: [1, 2, 3] };

function valid(change: Partial<RouteForm> = {}): RouteForm {
  return {
    ...emptyForm(),
    name: "chat",
    primaries: [{ model: "1", weight: "1" }],
    ...change,
  };
}

describe("the form of a route", () => {
  test("a new route starts with the defaults, no target and admins only", () => {
    expect(emptyForm()).toEqual({
      name: "",
      primaries: [],
      fallbacks: [],
      audience: "admins",
      team_ids: [],
      retries: "2",
      first_token_s: "30",
      total_s: "300",
      breaker_failures: "5",
      breaker_window_s: "60",
      breaker_open_s: "30",
      cache_enabled: false,
      cache_ttl_s: "300",
      cache_scope: "team",
    });
    expect(DEFAULTS).toEqual({
      retries: "2",
      first_token_s: "30",
      total_s: "300",
      breaker_failures: "5",
      breaker_window_s: "60",
      breaker_open_s: "30",
      cache_ttl_s: "300",
    });
  });

  test("a route is the form it is edited in, and back to the same request", () => {
    expect(formOf(support)).toEqual({
      name: "support-chat",
      primaries: [
        { model: "1", weight: "3" },
        { model: "2", weight: "1" },
      ],
      fallbacks: ["4"],
      audience: "all",
      team_ids: [],
      ...DEFAULTS,
      cache_enabled: false,
      cache_scope: "team",
    });
    expect(formOf(research)).toMatchObject({
      audience: "chosen",
      team_ids: ["1", "2"],
      retries: "0",
      first_token_s: "10",
      total_s: "120",
      breaker_failures: "3",
      breaker_window_s: "30",
      breaker_open_s: "15",
      cache_enabled: true,
      cache_ttl_s: "600",
      cache_scope: "user",
    });
    expect(formOf(legacy).audience).toBe("admins");
    for (const route of fixtures.routeList) {
      expect(requestOf(formOf(route), offered)).toEqual({
        name: route.name,
        everyone: route.everyone,
        primaries: route.primaries.map((p) => ({ model_id: p.model_id, weight: p.weight })),
        fallbacks: route.fallbacks.map((f) => f.model_id),
        retries: route.retries,
        first_token_timeout_ms: route.first_token_timeout_ms,
        total_timeout_ms: route.total_timeout_ms,
        breaker_failures: route.breaker_failures,
        breaker_window_s: route.breaker_window_s,
        breaker_open_s: route.breaker_open_s,
        cache_enabled: route.cache_enabled,
        cache_ttl_s: route.cache_ttl_s,
        cache_scope: route.cache_scope,
        team_ids: route.team_ids,
      });
    }
  });

  test("seconds may have milliseconds", () => {
    const form = valid({ first_token_s: "1.5", total_s: "2.25" });
    expect(requestOf(form, offered)).toMatchObject({
      first_token_timeout_ms: 1500,
      total_timeout_ms: 2250,
    });
    expect(formOf({ ...support, first_token_timeout_ms: 1500 }).first_token_s).toBe("1.5");
  });

  test("everyone is sent with no team; chosen teams without everyone; a team that is gone is not sent", () => {
    expect(requestOf(valid({ audience: "all", team_ids: ["1"] }), offered)).toMatchObject({
      everyone: true,
      team_ids: [],
    });
    expect(requestOf(valid({ audience: "admins", team_ids: ["1"] }), offered)).toMatchObject({
      everyone: false,
      team_ids: [],
    });
    expect(
      requestOf(valid({ audience: "chosen", team_ids: ["3", "99", "1"] }), offered),
    ).toMatchObject({ everyone: false, team_ids: [3, 1] });
  });

  test("the cache is sent as the form has it; a scope that is not offered is not sent", () => {
    expect(
      requestOf(valid({ cache_enabled: true, cache_ttl_s: "86400", cache_scope: "key" }), offered),
    ).toMatchObject({ cache_enabled: true, cache_ttl_s: 86_400, cache_scope: "key" });
    expect(requestOf(valid({ cache_scope: "everyone" }), offered).cache_scope).toBe("team");
  });

  test.each(["0", "86401", "1.5", "", "abc", "-1"])("a cache TTL of %j is refused", (ttl) => {
    expect(check(valid({ cache_ttl_s: ttl }), offered).fields.cache_ttl_s).toBe(
      "Enter a whole number from 1 to 86400.",
    );
  });

  test.each(["1", "86400", "300"])("a cache TTL of %j is fine, also with the cache off", (ttl) => {
    expect(check(valid({ cache_ttl_s: ttl, cache_enabled: false }), offered).fields.cache_ttl_s).toBeUndefined();
  });

  test("the gateway's words for the TTL are the form's", () => {
    expect(gatewayFields({ cache_ttl_s: "must be from 1 to 86400", cache_scope: "must be team, key or user" })).toEqual({
      cache_ttl_s: "Enter a whole number from 1 to 86400.",
      cache_scope: "must be team, key or user",
    });
  });

  test("the name is sent without the spaces around it", () => {
    expect(requestOf(valid({ name: " chat " }), offered).name).toBe("chat");
  });
});

describe("check", () => {
  test("a valid form has no problem", () => {
    expect(hasProblems(check(valid(), offered))).toBe(false);
    expect(hasProblems(check(valid({ audience: "chosen", team_ids: ["1"] }), offered))).toBe(false);
  });

  test.each(["", "Chat", "a/b", "-a", "a b", "a".repeat(65)])("the name %j is refused", (name) => {
    expect(check(valid({ name }), offered).fields.name).toBe(
      "Use 1 to 64 characters: a-z, 0-9, '.', '_' and '-', starting with a letter or a digit.",
    );
  });

  test.each(["a", "0", "a.b_c-d", "a".repeat(64)])("the name %j is fine", (name) => {
    expect(check(valid({ name }), offered).fields.name).toBeUndefined();
  });

  test("a route needs a primary target", () => {
    expect(check(valid({ primaries: [] }), offered).fields.primaries).toBe(
      "Add at least one primary target.",
    );
  });

  test("a row says what is wrong with it", () => {
    const problems = check(
      valid({
        primaries: [
          { model: "", weight: "1" },
          { model: "1", weight: "0" },
          { model: "1", weight: "1" },
          { model: "2", weight: "1001" },
          { model: "3", weight: "1.5" },
          { model: "99", weight: "5" },
          { model: "2", weight: "" },
        ],
        fallbacks: ["", "1", "4", "4", "77"],
      }),
      offered,
    );
    expect(problems.primaries).toEqual([
      { model: "Choose a model." },
      { weight: "Enter a whole number from 1 to 1000." },
      { model: "This model is already in the route." },
      { weight: "Enter a whole number from 1 to 1000." },
      { weight: "Enter a whole number from 1 to 1000." },
      { model: "Choose a model." },
      { model: "This model is already in the route.", weight: "Enter a whole number from 1 to 1000." },
    ]);
    expect(problems.fallbacks).toEqual([
      "Choose a model.",
      "This model is already in the route.",
      undefined,
      "This model is already in the route.",
      "Choose a model.",
    ]);
    expect(hasProblems(problems)).toBe(true);
  });

  test("the settings are within the limits of the gateway", () => {
    const at = (change: Partial<RouteForm>) => check(valid(change), offered).fields;
    expect(at({ retries: "6" }).retries).toBe("Enter a whole number from 0 to 5.");
    expect(at({ retries: "-1" }).retries).toBe("Enter a whole number from 0 to 5.");
    expect(at({ retries: "" }).retries).toBe("Enter a whole number from 0 to 5.");
    expect(at({ retries: "0" }).retries).toBeUndefined();
    expect(at({ first_token_s: "0" }).first_token_s).toBe("Enter seconds from 1 to 300.");
    expect(at({ first_token_s: "301" }).first_token_s).toBe("Enter seconds from 1 to 300.");
    expect(at({ first_token_s: "1.2345" }).first_token_s).toBe("Enter seconds from 1 to 300.");
    expect(at({ first_token_s: "300" }).first_token_s).toBeUndefined();
    expect(at({ total_s: "3601" }).total_s).toBe("Enter seconds from 1 to 3600.");
    expect(at({ breaker_failures: "0" }).breaker_failures).toBe("Enter a whole number from 1 to 100.");
    expect(at({ breaker_failures: "101" }).breaker_failures).toBe("Enter a whole number from 1 to 100.");
    expect(at({ breaker_window_s: "4" }).breaker_window_s).toBe("Enter seconds from 5 to 3600.");
    expect(at({ breaker_open_s: "3601" }).breaker_open_s).toBe("Enter seconds from 5 to 3600.");
    expect(at({ breaker_open_s: "5" }).breaker_open_s).toBeUndefined();
  });

  test("the total timeout is not below the first token timeout", () => {
    const fields = check(valid({ first_token_s: "60", total_s: "30" }), offered).fields;
    expect(fields.total_s).toBe("Must not be below the first token timeout.");
    expect(fields.first_token_s).toBeUndefined();
    // When one of them is out of range that is said, and nothing else.
    expect(check(valid({ first_token_s: "500", total_s: "30" }), offered).fields.total_s).toBeUndefined();
    expect(check(valid({ first_token_s: "30", total_s: "30" }), offered).fields.total_s).toBeUndefined();
  });

  test("chosen teams need at least one team that is offered", () => {
    const text = "Choose at least one team, or choose Admins only.";
    expect(check(valid({ audience: "chosen" }), offered).fields.team_ids).toBe(text);
    expect(check(valid({ audience: "chosen", team_ids: ["99"] }), offered).fields.team_ids).toBe(
      text,
    );
    expect(check(valid({ audience: "all" }), offered).fields.team_ids).toBeUndefined();
    expect(check(valid({ audience: "admins" }), offered).fields.team_ids).toBeUndefined();
  });
});

describe("gateway fields", () => {
  test("the names of the form are the names of the form, in the words of the form", () => {
    expect(
      gatewayFields({
        name: "x",
        primaries: "a model does not exist",
        first_token_timeout_ms: "must be 1000 to 300000",
        total_timeout_ms: "must be 1000 to 3600000",
        everyone: "must not be combined with teams or users",
        breaker_open_s: "must be 5 to 3600",
        retries: "must be 0 to 5",
        team_ids: "a team does not exist",
        other: "kept",
      }),
    ).toEqual({
      name: "x",
      primaries: "a model does not exist",
      first_token_s: "Enter seconds from 1 to 300.",
      total_s: "Enter seconds from 1 to 3600.",
      audience: "must not be combined with teams or users",
      breaker_open_s: "Enter seconds from 5 to 3600.",
      retries: "Enter a whole number from 0 to 5.",
      team_ids: "a team does not exist",
      other: "kept",
    });
  });

  test("the total below the first token keeps the gateway's words", () => {
    expect(gatewayFields({ total_timeout_ms: "must not be below the first token timeout" })).toEqual({
      total_s: "Must not be below the first token timeout.",
    });
  });
});

describe("inFormWords", () => {
  test("an answer of the gateway keeps its status, code and message, and gets the words of the form", () => {
    const said = new ApiError(422, "validation_failed", "Some fields are not valid.", {
      first_token_timeout_ms: "must be 1000 to 300000",
    });
    const made = inFormWords(said);
    expect(made).toBeInstanceOf(ApiError);
    expect(made).toMatchObject({
      status: 422,
      code: "validation_failed",
      message: "Some fields are not valid.",
      fields: { first_token_s: "Enter seconds from 1 to 300." },
    });
  });

  test("an answer with no fields, and any other error, stay as they are", () => {
    const plain = new ApiError(409, "route_exists", "A route of this name already exists.");
    expect(inFormWords(plain)).toBe(plain);
    const other = new Error("x");
    expect(inFormWords(other)).toBe(other);
  });
});

describe("words", () => {
  test("a target, the teams of a route", () => {
    expect(targetText(fixtures.routes.support.primaries[0] ?? { model: "", weight: 0 })).toBe("openai/gpt-4o-mini ×3");
    expect(teamsText(support)).toBe("All teams");
    expect(teamsText(research)).toBe("2 teams");
    expect(teamsText({ ...research, team_ids: [1] })).toBe("1 team");
    expect(teamsText(legacy)).toBe("Admins only");
  });

  test("the state of a target", () => {
    expect(healthText("closed")).toBe("Healthy");
    expect(healthText("open")).toBe("Failing");
    expect(healthText("half_open")).toBe("Testing");
  });

  test("the health of the targets of one route", () => {
    expect(healthFor(support, fixtures.healthList).map((t) => `${t.provider}/${t.model}`)).toEqual([
      "openai/gpt-4o-mini",
      "openai/gpt-4o",
      "local-llm/llama3.1:8b",
    ]);
    expect(healthFor(legacy, fixtures.healthList).map((t) => t.model)).toEqual(["o3-mini"]);
  });

  test("a fallback moves up and down, and stays where it is at the ends", () => {
    expect(moved(["a", "b", "c"], 1, -1)).toEqual(["b", "a", "c"]);
    expect(moved(["a", "b", "c"], 1, 1)).toEqual(["a", "c", "b"]);
    expect(moved(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]);
    expect(moved(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);
    const list = ["a", "b"];
    moved(list, 0, 1);
    expect(list).toEqual(["a", "b"]);
  });
});
