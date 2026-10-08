import { describe, expect, test } from "vitest";
import { ApiError, ConsoleRefusal } from "@/api/errors";
import {
  ANY,
  budgetName,
  conditionText,
  deliveriesOf,
  deliveryLine,
  deliverySummary,
  emptyRuleForm,
  offeredOf,
  paramsOf,
  paramsOnFields,
  ruleChangesOf,
  ruleFormOf,
  ruleRequestOf,
  subjectText,
  type Lists,
  type Offered,
  type RuleForm,
} from "@/lib/alerts";
import * as fixtures from "@/test/fixtures";

const lookups = {
  budgets: fixtures.budgetList,
  keys: fixtures.keyList.map((key) => ({ id: key.id, name: key.name })),
};

const offered: Offered = {
  budgets: fixtures.budgetList.map((one) => one.id),
  routes: fixtures.routeList.map((one) => one.name),
  providers: fixtures.providerList.map((one) => one.name),
  keys: fixtures.keyList.map((one) => one.id),
  models: fixtures.modelList.map((one) => ({ provider: one.provider_name, name: one.name })),
  channels: fixtures.alertChannelList.map((one) => one.id),
};

const form = (patch: Partial<RuleForm>): RuleForm => ({ ...emptyRuleForm(), name: "R", ...patch });

describe("paramsOf", () => {
  test("reads the parameters of each kind", () => {
    expect(paramsOf(fixtures.alertRules.budget)).toEqual({
      kind: "budget",
      budget_id: 2,
      percent: 80,
    });
    expect(paramsOf(fixtures.alertRules.errors)).toEqual({
      kind: "error_rate",
      scope: "route",
      subject: "support-chat",
      percent: 10,
      window_minutes: 5,
      min_requests: 20,
    });
    expect(paramsOf(fixtures.alertRules.circuit)).toEqual({
      kind: "circuit_open",
      provider: null,
      model: null,
    });
  });

  test("a kind or a shape it does not know is none", () => {
    expect(paramsOf({ kind: "latency", params: {} })).toBeNull();
    expect(paramsOf({ kind: "budget", params: { percent: "80" } })).toBeNull();
    expect(paramsOf({ kind: "error_rate", params: { scope: "planet", percent: 5 } })).toBeNull();
    expect(paramsOf({ kind: "budget", params: null })).toBeNull();
  });
});

describe("conditionText", () => {
  test("a budget, by its name and the percent", () => {
    expect(conditionText(fixtures.alertRules.budget, lookups)).toBe(
      "Budget 'team Platform weekly' at 80%",
    );
    expect(
      conditionText({ kind: "budget", params: { budget_id: null, percent: 50 } }, lookups),
    ).toBe("Any budget at 50%");
    expect(
      conditionText({ kind: "budget", params: { budget_id: 99, percent: 50 } }, lookups),
    ).toBe("Budget 99 at 50%");
  });

  test("errors, by scope", () => {
    expect(conditionText(fixtures.alertRules.errors, lookups)).toBe(
      "Errors ≥ 10% over 5 min on route support-chat",
    );
    const rate = (params: object) => ({
      kind: "error_rate",
      params: { percent: 25, window_minutes: 15, min_requests: 20, ...params },
    });
    expect(conditionText(rate({ scope: "gateway", subject: null }), lookups)).toBe(
      "Errors ≥ 25% over 15 min on the gateway",
    );
    expect(conditionText(rate({ scope: "provider", subject: null }), lookups)).toBe(
      "Errors ≥ 25% over 15 min on each provider",
    );
    expect(
      conditionText(rate({ scope: "key", subject: String(fixtures.keys.active.id) }), lookups),
    ).toBe(`Errors ≥ 25% over 15 min on key ${fixtures.keys.active.name}`);
    expect(conditionText(rate({ scope: "key", subject: "404" }), lookups)).toBe(
      "Errors ≥ 25% over 15 min on key 404",
    );
  });

  test("a circuit, by what it is limited to", () => {
    const circuit = (params: object) => ({ kind: "circuit_open", params });
    expect(conditionText(circuit({ provider: null, model: null }), lookups)).toBe(
      "Circuit opens on any target",
    );
    expect(conditionText(circuit({ provider: "openai", model: null }), lookups)).toBe(
      "Circuit opens on any model of openai",
    );
    expect(conditionText(circuit({ provider: "openai", model: "gpt-4.1" }), lookups)).toBe(
      "Circuit opens on openai/gpt-4.1",
    );
    expect(conditionText(circuit({ provider: null, model: "gpt-4.1" }), lookups)).toBe(
      "Circuit opens on model gpt-4.1 of any provider",
    );
  });

  test("a condition it cannot read is said so", () => {
    expect(conditionText({ kind: "latency", params: {} }, lookups)).toBe("Unknown condition");
  });
});

test("a budget is named by its label and its period", () => {
  expect(budgetName(fixtures.budgets.gateway)).toBe("gateway monthly");
  expect(budgetName(fixtures.budgets.team)).toBe("team Platform weekly");
});

describe("subjectText", () => {
  test.each([
    ["gateway", "the gateway"],
    ["route:support-chat", "route support-chat"],
    ["provider:openai", "provider openai"],
    [`key:${fixtures.keys.active.id}`, `key ${fixtures.keys.active.name}`],
    ["key:404", "key 404"],
    ["target:openai/gpt-4.1", "openai/gpt-4.1"],
    ["budget:2:2026-09-28", "team Platform weekly from 2026-09-28"],
    ["budget:99:2026-09-28", "budget 99 from 2026-09-28"],
    ["channel:1", "a channel test"],
    ["something:else", "something:else"],
  ])("%s", (subject, expected) => {
    expect(subjectText(subject, lookups)).toBe(expected);
  });
});

describe("deliveries", () => {
  test("are read as the gateway gives them", () => {
    expect(deliveriesOf(fixtures.alertEvents.firing)).toHaveLength(2);
    expect(deliveriesOf({ deliveries: [{ nonsense: true }, 7, null] })).toEqual([]);
  });

  test("are told one by one", () => {
    const [ok, failed] = deliveriesOf(fixtures.alertEvents.firing);
    expect(ok && deliveryLine(ok)).toBe("ops-webhook: delivered (200)");
    expect(failed && deliveryLine(failed)).toBe(
      "team-slack: failed, the receiver answered 500 (3 tries)",
    );
    expect(
      deliveryLine({ channel_id: 1, channel_name: "a", ok: false, status: null, tries: 1, error: null }),
    ).toBe("a: failed (1 try)");
    expect(
      deliveryLine({
        channel_id: 1,
        channel_name: "a",
        ok: false,
        status: null,
        tries: 0,
        error: "dropped: the delivery queue was full",
      }),
    ).toBe("a: failed, dropped: the delivery queue was full");
  });

  test("are summed up in words", () => {
    expect(deliverySummary(deliveriesOf(fixtures.alertEvents.firing))).toBe("1 of 2 delivered");
    expect(deliverySummary(deliveriesOf(fixtures.alertEvents.resolved))).toBe("1 delivered");
    expect(deliverySummary([])).toBe("None yet");
    expect(deliverySummary([], true)).toBe("No channels");
    expect(
      deliverySummary([
        { channel_id: 1, channel_name: "a", ok: false, status: null, tries: 0, error: "x" },
      ]),
    ).toBe("0 of 1 delivered");
  });
});

describe("the form of a rule", () => {
  test("starts as a budget at 80%, for every budget", () => {
    expect(emptyRuleForm()).toMatchObject({
      kind: "budget",
      budget_id: ANY,
      percent: "80",
      channel_ids: [],
    });
  });

  test("is made from a rule, and makes its parameters again", () => {
    for (const rule of fixtures.alertRuleList) {
      const made = ruleFormOf(rule);
      expect(made.name).toBe(rule.name);
      expect(ruleRequestOf(made, offered).params).toEqual(rule.params);
      expect(ruleRequestOf(made, offered).channel_ids).toEqual(rule.channels.map((one) => one.id));
    }
  });

  test("the request of each kind", () => {
    expect(ruleRequestOf(form({ percent: "75", budget_id: "2" }), offered)).toEqual({
      name: "R",
      kind: "budget",
      params: { budget_id: 2, percent: 75 },
      channel_ids: [],
    });
    expect(
      ruleRequestOf(
        form({
          kind: "error_rate",
          scope: "route",
          subject: ANY,
          percent: "10",
          window_minutes: "15",
          min_requests: "30",
          channel_ids: ["1", "2"],
        }),
        offered,
      ),
    ).toEqual({
      name: "R",
      kind: "error_rate",
      params: { scope: "route", subject: null, percent: 10, window_minutes: 15, min_requests: 30 },
      channel_ids: [1, 2],
    });
    expect(
      ruleRequestOf(form({ kind: "circuit_open", provider: "openai", model: ANY }), offered).params,
    ).toEqual({ provider: "openai", model: null });
  });

  test("a choice that is not offered any more is never sent", () => {
    expect(ruleRequestOf(form({ budget_id: "999" }), offered).params).toMatchObject({
      budget_id: null,
    });
    expect(
      ruleRequestOf(form({ kind: "error_rate", scope: "route", subject: "gone" }), offered).params,
    ).toMatchObject({ subject: null });
    expect(
      ruleRequestOf(form({ kind: "error_rate", scope: "key", subject: "999" }), offered).params,
    ).toMatchObject({ subject: null });
    expect(
      ruleRequestOf(form({ kind: "circuit_open", provider: "gone", model: "gpt-4o" }), offered)
        .params,
    ).toEqual({ provider: null, model: "gpt-4o" });
    // A model of another provider than the one chosen.
    expect(
      ruleRequestOf(form({ kind: "circuit_open", provider: "openai", model: "claude" }), offered)
        .params,
    ).toEqual({ provider: "openai", model: null });
    expect(ruleRequestOf(form({ channel_ids: ["1", "999"] }), offered).channel_ids).toEqual([1]);
  });

  test("a revoked key is not offered, so the request cannot silently differ from the select", () => {
    const revoked = fixtures.keyList.find((key) => key.status === "revoked");
    const keys = fixtures.keyList.filter((key) => key.status !== "revoked").map((key) => key.id);
    const narrow = { ...offered, keys };
    const id = String(revoked?.id);
    const asked = form({ kind: "error_rate", scope: "key", subject: id });
    expect(ruleRequestOf(asked, narrow).params).toMatchObject({ subject: null });
  });

  test("offeredOf leaves a revoked key out and keeps the others", () => {
    const revoked = fixtures.keyList.filter((key) => key.status === "revoked");
    expect(revoked.length).toBeGreaterThan(0);
    const lists: Lists = {
      budgets: fixtures.budgetList,
      routes: fixtures.routeList,
      providers: fixtures.providerList,
      keys: fixtures.keyList,
      models: fixtures.modelList,
      channels: fixtures.alertChannelList,
    };
    const result = offeredOf(lists);
    for (const key of revoked) {
      expect(result.keys).not.toContain(key.id);
    }
    expect(result.keys).toEqual(
      fixtures.keyList.filter((key) => key.status !== "revoked").map((key) => key.id),
    );
    expect(result.keys.length).toBeGreaterThan(0);
    expect(result.channels).toEqual(fixtures.alertChannelList.map((one) => one.id));
  });

  test("what the form did not touch is kept as the rule has it", () => {
    const rule = {
      ...fixtures.alertRules.errors,
      params: fixtures.freeForm({ scope: "route", subject: "gone", percent: 10, window_minutes: 5, min_requests: 20 }),
    };
    const kept = ruleRequestOf(ruleFormOf(rule), offered, rule);
    expect(kept.params).toMatchObject({ subject: "gone" });
    expect(ruleChangesOf(rule, kept)).toEqual({});
    const touched = ruleRequestOf({ ...ruleFormOf(rule), subject: "also-gone" }, offered, rule);
    expect(touched.params).toMatchObject({ subject: null });
  });

  test("the name is trimmed", () => {
    expect(ruleRequestOf(form({ name: "  Spaced " }), offered).name).toBe("Spaced");
  });

  test("the subject of the gateway scope is never sent", () => {
    const request = ruleRequestOf(form({ kind: "error_rate", scope: "gateway", subject: "support-chat" }), offered);
    expect(request.params).toMatchObject({ scope: "gateway", subject: null });
  });

  test.each([
    ["percent", { percent: "0" }, "Enter a whole number from 1 to 100."],
    ["percent", { percent: "101" }, "Enter a whole number from 1 to 100."],
    ["percent", { percent: "8.5" }, "Enter a whole number from 1 to 100."],
    ["percent", { percent: "" }, "Enter a whole number from 1 to 100."],
    ["window_minutes", { kind: "error_rate", window_minutes: "4" }, "Enter a whole number from 5 to 60."],
    ["window_minutes", { kind: "error_rate", window_minutes: "61" }, "Enter a whole number from 5 to 60."],
    ["min_requests", { kind: "error_rate", min_requests: "0" }, "Enter a whole number from 1 to 100000."],
  ])("%s is refused in words: %j", (field, patch, message) => {
    let thrown: unknown;
    try {
      ruleRequestOf(form(patch), offered);
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(ConsoleRefusal);
    expect(thrown).toMatchObject({ message, field });
  });
});

test("parameter errors of the gateway are named as the form fields", () => {
  const error = new ApiError(422, "validation_failed", "Some fields are not valid.", {
    "params.percent": "bad",
    name: "also bad",
  });
  const mapped = paramsOnFields(error);
  expect(mapped).toBeInstanceOf(ApiError);
  expect(mapped).toMatchObject({ status: 422, fields: { percent: "bad", name: "also bad" } });
  expect(paramsOnFields("other")).toBe("other");
});

describe("what changed in a rule", () => {
  const rule = fixtures.alertRules.errors;

  test("nothing, when the form is the rule", () => {
    expect(ruleChangesOf(rule, ruleRequestOf(ruleFormOf(rule), offered))).toEqual({});
  });

  test("only what differs", () => {
    const request = (patch: Partial<RuleForm>) => ruleRequestOf({ ...ruleFormOf(rule), ...patch }, offered);
    expect(ruleChangesOf(rule, request({ name: "Renamed" }))).toEqual({ name: "Renamed" });
    expect(ruleChangesOf(rule, request({ name: ` ${rule.name} ` }))).toEqual({});
    expect(ruleChangesOf(rule, request({ percent: "11" }))).toEqual({
      params: { scope: "route", subject: "support-chat", percent: 11, window_minutes: 5, min_requests: 20 },
    });
    expect(ruleChangesOf(rule, request({ channel_ids: ["2", "1"] }))).toEqual({ channel_ids: [2, 1] });
    expect(ruleChangesOf(rule, request({ channel_ids: ["1"] }))).toEqual({});
  });

  test("the kind is never among them", () => {
    expect(ruleChangesOf(rule, ruleRequestOf({ ...ruleFormOf(rule), name: "x" }, offered))).not.toHaveProperty("kind");
  });
});
