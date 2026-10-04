import { describe, expect, test } from "vitest";
import { ApiError, ConsoleRefusal } from "@/api/errors";
import {
  budgetFormOf,
  budgetRequestOf,
  chosenTarget,
  inFormWords,
  limitFormOf,
  limitRequestOf,
  NEEDS_A_LIMIT,
  offeredWith,
  percentOf,
  periodText,
  scopeText,
  type BudgetForm,
  type LimitForm,
} from "@/lib/limits";

const offered = { team: [1, 2], user: [5], key: [9] };

const limit = (patch: Partial<LimitForm>): LimitForm => ({
  scope: "gateway",
  scope_id: "",
  requests_per_minute: "",
  tokens_per_minute: "",
  concurrent: "",
  ...patch,
});

const budget = (patch: Partial<BudgetForm>): BudgetForm => ({
  scope: "gateway",
  scope_id: "",
  amount: "100",
  period: "monthly",
  action: "block",
  ...patch,
});

function refusal(run: () => unknown): ConsoleRefusal {
  try {
    run();
  } catch (error) {
    if (error instanceof ConsoleRefusal) return error;
    throw error;
  }
  throw new Error("nothing was refused");
}

describe("the limit that is sent", () => {
  test("the gateway has no id, and an empty number is left out", () => {
    expect(limitRequestOf(limit({ requests_per_minute: "60" }), offered)).toEqual({
      scope: "gateway",
      requests_per_minute: 60,
    });
  });

  test("a team, user or key goes with its id; all three numbers go", () => {
    expect(
      limitRequestOf(
        limit({
          scope: "team",
          scope_id: "2",
          requests_per_minute: "60",
          tokens_per_minute: "1000000",
          concurrent: "4",
        }),
        offered,
      ),
    ).toEqual({
      scope: "team",
      scope_id: 2,
      requests_per_minute: 60,
      tokens_per_minute: 1_000_000,
      concurrent: 4,
    });
  });

  test("the id of another scope is not sent", () => {
    // The team that was chosen is no longer one when the scope is the gateway.
    expect(limitRequestOf(limit({ scope_id: "2", concurrent: "1" }), offered)).toEqual({
      scope: "gateway",
      concurrent: 1,
    });
  });

  test("at least one number, on the first field", () => {
    const refused = refusal(() => limitRequestOf(limit({}), offered));
    expect([refused.message, refused.field]).toEqual([NEEDS_A_LIMIT, "requests_per_minute"]);
  });

  test.each([
    ["requests_per_minute", "0", "Enter a whole number from 1 to 1,000,000."],
    ["requests_per_minute", "1000001", "Enter a whole number from 1 to 1,000,000."],
    ["requests_per_minute", "1.5", "Enter a whole number from 1 to 1,000,000."],
    ["requests_per_minute", "-3", "Enter a whole number from 1 to 1,000,000."],
    ["tokens_per_minute", "1000000000001", "Enter a whole number from 1 to 1,000,000,000,000."],
    ["tokens_per_minute", "abc", "Enter a whole number from 1 to 1,000,000,000,000."],
    ["concurrent", "1000001", "Enter a whole number from 1 to 1,000,000."],
  ] as const)("%s of %j is refused", (field, text, message) => {
    const refused = refusal(() => limitRequestOf(limit({ [field]: text }), offered));
    expect([refused.field, refused.message]).toEqual([field, message]);
  });

  test("the ends of the ranges are accepted", () => {
    expect(
      limitRequestOf(
        limit({ requests_per_minute: "1", tokens_per_minute: "1000000000000", concurrent: "1000000" }),
        offered,
      ),
    ).toEqual({
      scope: "gateway",
      requests_per_minute: 1,
      tokens_per_minute: 1_000_000_000_000,
      concurrent: 1_000_000,
    });
  });

  test.each([
    ["team", "Choose a team."],
    ["user", "Choose a user."],
    ["key", "Choose a key."],
  ] as const)("a %s that is not offered is refused", (scope, message) => {
    for (const id of ["", "77"]) {
      const refused = refusal(() =>
        limitRequestOf(limit({ scope, scope_id: id, concurrent: "1" }), offered),
      );
      expect([refused.field, refused.message]).toEqual(["scope_id", message]);
    }
  });

  test("the form of a row starts from its numbers", () => {
    expect(
      limitFormOf({
        id: 1,
        scope: "team",
        scope_id: 2,
        label: "team 'Platform'",
        requests_per_minute: 60,
        tokens_per_minute: null,
        concurrent: 4,
      }),
    ).toEqual(
      limit({ scope: "team", scope_id: "2", requests_per_minute: "60", concurrent: "4" }),
    );
  });
});

describe("the budget that is sent", () => {
  test("dollars become micros, the gateway has no id", () => {
    expect(budgetRequestOf(budget({ amount: "12.5" }), offered)).toEqual({
      scope: "gateway",
      amount_micros: 12_500_000,
      period: "monthly",
      action: "block",
    });
  });

  test("a user with its id, a weekly alert", () => {
    expect(
      budgetRequestOf(
        budget({ scope: "user", scope_id: "5", amount: "0.000001", period: "weekly", action: "alert" }),
        offered,
      ),
    ).toEqual({ scope: "user", scope_id: 5, amount_micros: 1, period: "weekly", action: "alert" });
  });

  test.each(["", "0", "0.0", "-1", "abc", "1.2345678", "1e3", "2000000000"])(
    "an amount of %j is refused on the amount",
    (amount) => {
      const refused = refusal(() => budgetRequestOf(budget({ amount }), offered));
      expect(refused.field).toBe("amount");
    },
  );

  test("the largest amount is accepted", () => {
    expect(budgetRequestOf(budget({ amount: "1000000000" }), offered).amount_micros).toBe(
      1_000_000_000_000_000,
    );
  });

  test("a period or an action that is not offered is refused", () => {
    expect(refusal(() => budgetRequestOf(budget({ period: "yearly" }), offered)).field).toBe("period");
    expect(refusal(() => budgetRequestOf(budget({ action: "stop" }), offered)).field).toBe("action");
  });

  test("a target that is not offered is refused", () => {
    expect(
      refusal(() => budgetRequestOf(budget({ scope: "key", scope_id: "1" }), offered)).field,
    ).toBe("scope_id");
  });

  test("the form of a row has its amount in dollars", () => {
    expect(
      budgetFormOf({
        id: 1,
        scope: "key",
        scope_id: 9,
        label: "key 'ci'",
        amount_micros: 2_500_000,
        period: "daily",
        action: "alert",
        period_start: "2026-09-30",
        spent_micros: 0,
      }),
    ).toEqual(budget({ scope: "key", scope_id: "9", amount: "2.5", period: "daily", action: "alert" }));
  });
});

describe("words and shares", () => {
  test("scope and period are said in words; what is unknown is shown as it is", () => {
    expect(["gateway", "team", "user", "key", "org"].map(scopeText)).toEqual([
      "Gateway",
      "Team",
      "User",
      "Key",
      "org",
    ]);
    expect(["daily", "weekly", "monthly", "yearly"].map(periodText)).toEqual([
      "Daily",
      "Weekly",
      "Monthly",
      "yearly",
    ]);
  });

  test("the share spent is rounded down, so that a budget is not full before it is", () => {
    expect(percentOf(0, 100)).toBe(0);
    expect(percentOf(12_500_000, 100_000_000)).toBe(12);
    expect(percentOf(99_999_999, 100_000_000)).toBe(99);
    expect(percentOf(100_000_000, 100_000_000)).toBe(100);
    expect(percentOf(250_000_000, 100_000_000)).toBe(250);
    expect(percentOf(5, 0)).toBe(0);
  });

  test("the target of the row that is changed is offered besides the others", () => {
    expect(offeredWith(offered, null)).toBe(offered);
    expect(offeredWith(offered, { scope: "gateway", scope_id: null })).toBe(offered);
    expect(offeredWith(offered, { scope: "key", scope_id: 77 })).toEqual({
      team: [1, 2],
      user: [5],
      key: [9, 77],
    });
    expect(offered.key).toEqual([9]);
  });

  test("a choice that is not offered is not kept", () => {
    expect(chosenTarget("2", [1, 2])).toBe("2");
    expect(chosenTarget("3", [1, 2])).toBe("");
    expect(chosenTarget("", [1, 2])).toBe("");
  });
});

describe("what the gateway says of single fields", () => {
  test("the amount has the name of the form, in its words", () => {
    const error = new ApiError(422, "validation_failed", "Some fields are not valid.", {
      amount_micros: "must be from 1 to 1000000000000000",
      period: "must be daily, weekly or monthly",
    });
    const words = inFormWords(error);
    expect(words).toBeInstanceOf(ApiError);
    expect(words).toMatchObject({
      status: 422,
      code: "validation_failed",
      fields: {
        amount: "Enter dollars above 0, with up to 6 decimals, up to $1,000,000,000.",
        period: "must be daily, weekly or monthly",
      },
    });
  });

  test("another error is returned as it is", () => {
    const error = new ApiError(500, "internal_error", "Something went wrong.");
    expect(inFormWords(error)).toBe(error);
    const plain = new Error("x");
    expect(inFormWords(plain)).toBe(plain);
  });
});
