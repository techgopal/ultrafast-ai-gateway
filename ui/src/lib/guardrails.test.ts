import { describe, expect, test } from "vitest";
import { ApiError } from "@/api/errors";
import * as fixtures from "@/test/fixtures";
import {
  attachmentOf,
  check,
  emptyForm,
  formOf,
  hasProblems,
  loggedLabel,
  moved,
  piiLabel,
  PII_TYPES,
  requestOf,
  ruleErrorsOf,
  segmentsOf,
  sideLines,
  verdictOf,
  type GuardrailForm,
} from "./guardrails";

const { guardrails } = fixtures;

/** The one rule of a new form. */
function aRule() {
  const [rule] = emptyForm().rules;
  if (rule === undefined) throw new Error("a new form has a rule");
  return rule;
}

function formWith(patch: Partial<GuardrailForm>): GuardrailForm {
  return { ...emptyForm(), name: "g", ...patch };
}

describe("the form of a guardrail", () => {
  test("a new one starts as a rules guardrail with one PII rule, off for every call", () => {
    const form = emptyForm();
    expect(form).toMatchObject({
      name: "",
      kind: "rules",
      enabled: true,
      is_default: false,
      directions: "both",
      fail_mode: "open",
      timeout_ms: "3000",
    });
    expect(form.rules).toHaveLength(1);
    expect(form.rules[0]).toMatchObject({ kind: "pii", pii: ["EMAIL"], action: "redact", directions: "both" });
  });

  test("a stored guardrail is read into the form and written back as it was", () => {
    for (const one of [guardrails.pii, guardrails.words]) {
      const form = formOf(one);
      const body = requestOf(form, one);
      expect(body.rules).toEqual(one.rules);
    }
    const words = formOf(guardrails.words);
    expect(words.rules[0]).toMatchObject({ kind: "keywords", words: "swordfish\nproject x", wholeWord: true });
    expect(words.rules[1]).toMatchObject({ kind: "regex", regex: "TICKET-[0-9]+" });
  });

  test("an external guardrail keeps its settings and never holds its URL", () => {
    const form = formOf(guardrails.external);
    expect(form).toMatchObject({ kind: "external", url: "", directions: "both", fail_mode: "open", timeout_ms: "3000" });
  });

  test("keywords are one for each line, blanks dropped and trimmed", () => {
    const form = formWith({
      rules: [{ ...aRule(), kind: "keywords", words: " a \n\n b\r\nc c ", wholeWord: false }],
    });
    expect(requestOf(form, null).rules?.[0]?.matcher).toEqual({
      keywords: { words: ["a", "b", "c c"], whole_word: false },
    });
  });

  test("a new external guardrail sends its URL and no rules, a rules one no URL", () => {
    const external = formWith({ kind: "external", url: " https://guard.example.test/x ", fail_mode: "closed" });
    expect(requestOf(external, null)).toEqual({
      name: "g",
      description: "",
      kind: "external",
      enabled: true,
      is_default: false,
      url: "https://guard.example.test/x",
      directions: "both",
      fail_mode: "closed",
      timeout_ms: 3000,
    });
    const rules = requestOf(formWith({}), null);
    expect(rules).not.toHaveProperty("url");
    expect(rules).not.toHaveProperty("timeout_ms");
    expect(rules.kind).toBe("rules");
  });

  test("a saved external guardrail sends a URL only when one is typed", () => {
    const kept = requestOf(formOf(guardrails.external), guardrails.external);
    expect(kept).not.toHaveProperty("url");
    const changed = requestOf({ ...formOf(guardrails.external), url: "https://n.example.test" }, guardrails.external);
    expect(changed.url).toBe("https://n.example.test");
    // The kind is not changed once made.
    expect(kept).not.toHaveProperty("kind");
  });
});

describe("what the form checks before it is sent", () => {
  test("a name, a rule and its content are needed", () => {
    const empty = emptyForm();
    const problems = check({ ...empty, rules: [{ ...aRule(), id: "", pii: [] }] });
    expect(problems.fields.name).toBe("Enter a name.");
    // The id is asked for first.
    expect(problems.rows[0]).toBe("Enter an id for the rule.");
    expect(hasProblems(problems)).toBe(true);
    expect(check(formWith({ rules: [] })).fields.rules).toBe("Add at least one rule.");
  });

  test.each([
    ["keywords", { kind: "keywords" as const, words: "  \n " }, "Enter at least one word."],
    ["regex", { kind: "regex" as const, regex: " " }, "Enter a regular expression."],
    ["pii", { kind: "pii" as const, pii: [] }, "Choose at least one type."],
  ])("a %s rule with nothing in it", (_, patch, message) => {
    const form = formWith({ rules: [{ ...aRule(), ...patch }] });
    expect(check(form).rows[0]).toBe(message);
  });

  test("a rule id is needed and may not repeat", () => {
    const one = aRule();
    expect(check(formWith({ rules: [{ ...one, id: " " }] })).rows[0]).toBe("Enter an id for the rule.");
    const two = check(formWith({ rules: [{ ...one, id: "a", key: 1 }, { ...one, id: "a", key: 2 }] }));
    expect(two.rows[1]).toBe("Another rule has this id.");
    expect(two.rows[0]).toBeUndefined();
  });

  test("a new external guardrail needs a URL and a timeout in range", () => {
    const external = formWith({ kind: "external", url: "", timeout_ms: "999" });
    const problems = check(external, null);
    expect(problems.fields.url).toBe("Enter the URL to post to.");
    expect(problems.fields.timeout_ms).toBe("Enter 1000 to 10000 milliseconds.");
    expect(check({ ...external, url: "https://x.example.test", timeout_ms: "10000" }, null).fields).toEqual({});
    // A saved one keeps its URL.
    expect(check({ ...external, timeout_ms: "3000" }, guardrails.external).fields.url).toBeUndefined();
  });

  test("a rules guardrail ignores the external settings", () => {
    expect(check(formWith({ timeout_ms: "x", url: "" })).fields).toEqual({});
  });
});

describe("the faults the gateway names", () => {
  test("rules[i] keys go to the row, the set fault to the list, the rest stays", () => {
    const error = new ApiError(422, "validation_failed", "Some fields are not valid.", {
      "rules[1]": "regex parse error",
      "rules[2].kind": "matcher must be keywords, regex or pii",
      "rules[0].types": "unknown PII type",
      rules: "add at least one rule",
      name: "name must be 1 to 100 characters",
    });
    const found = ruleErrorsOf(error);
    expect(found.rows).toEqual({
      0: "unknown PII type",
      1: "regex parse error",
      2: "matcher must be keywords, regex or pii",
    });
    expect(found.set).toBe("add at least one rule");
    expect(found.rest).toBeInstanceOf(ApiError);
    const rest = found.rest as ApiError;
    expect(rest.fields).toEqual({ name: "name must be 1 to 100 characters" });
    expect(rest.status).toBe(422);
  });

  test("an error that is not the gateway's is left alone", () => {
    const error = new Error("x");
    expect(ruleErrorsOf(error)).toEqual({ rows: {}, set: undefined, rest: error });
  });
});

describe("the redacted text", () => {
  test("placeholders are marked, the rest is plain", () => {
    expect(segmentsOf("Write to [REDACTED:EMAIL] or [REDACTED] now.")).toEqual([
      { text: "Write to ", redacted: false },
      { text: "[REDACTED:EMAIL]", redacted: true },
      { text: " or ", redacted: false },
      { text: "[REDACTED]", redacted: true },
      { text: " now.", redacted: false },
    ]);
    expect(segmentsOf("plain")).toEqual([{ text: "plain", redacted: false }]);
    expect(segmentsOf("")).toEqual([]);
    expect(segmentsOf("[REDACTED:EMAIL]")).toEqual([{ text: "[REDACTED:EMAIL]", redacted: true }]);
  });

  test("something like a placeholder is not one", () => {
    expect(segmentsOf("[redacted:email] [REDACTED:] [REDACTED:a b]")).toHaveLength(1);
  });
});

describe("the verdict of a test", () => {
  test("says what happened, by counts and names only", () => {
    expect(verdictOf({ blocked_by: null, flags: [], redactions: {} })).toEqual(["Nothing found."]);
    expect(verdictOf({ blocked_by: { id: 2, name: "house-rules" }, flags: [], redactions: {} })).toEqual([
      "Blocked by house-rules.",
    ]);
    expect(
      verdictOf({
        blocked_by: null,
        flags: [{ guardrail_id: 0, guardrail_name: "Test rules", rule_id: "ticket" }],
        redactions: { EMAIL: 2, other: 1 },
      }),
    ).toEqual(["Redacted: EMAIL 2, other 1.", "Flagged: ticket."]);
  });

  test("an external failure flag is said as a failure", () => {
    expect(
      verdictOf({
        blocked_by: null,
        flags: [{ guardrail_id: 3, guardrail_name: "acme-scanner", rule_id: "external_error:timeout" }],
        redactions: {},
      }),
    ).toEqual(["acme-scanner could not be asked: timeout."]);
  });
});

describe("small helpers", () => {
  test("the PII types are the gateway's, each with a label", () => {
    expect(PII_TYPES.map((one) => one.type)).toEqual([
      "EMAIL",
      "PHONE",
      "CREDIT_CARD",
      "IBAN",
      "US_SSN",
      "IPV4",
      "IPV6",
      "SECRET",
    ]);
    expect(piiLabel("US_SSN")).toBe("US Social Security number");
  });

  test("moved puts an item one place up or down, and stays at the ends", () => {
    expect(moved([1, 2, 3], 1, -1)).toEqual([2, 1, 3]);
    expect(moved([1, 2, 3], 1, 1)).toEqual([1, 3, 2]);
    expect(moved([1, 2, 3], 0, -1)).toEqual([1, 2, 3]);
    expect(moved([1, 2, 3], 2, 1)).toEqual([1, 2, 3]);
  });
});

describe("what is sent for an attachment", () => {
  test("nothing when it is as it was, the whole list when it is not", () => {
    expect(attachmentOf([], null)).toBeUndefined();
    expect(attachmentOf([2, 1], [2, 1])).toBeUndefined();
    expect(attachmentOf([1, 2], [2, 1])).toEqual([1, 2]);
    expect(attachmentOf([3], null)).toEqual([3]);
    // Taking them all off is a change; [] is sent.
    expect(attachmentOf([], [1])).toEqual([]);
  });
});

describe("what the log says of a direction", () => {
  test("names, counts and rule ids", () => {
    expect(
      sideLines({
        action: "blocked",
        checked_with: [
          { id: 1, name: "mask-emails" },
          { id: 2, name: "house-rules" },
        ],
        blocked_by: { id: 2, name: "house-rules" },
        redactions: { EMAIL: 2 },
        flags: [
          { guardrail_id: 2, rule_id: "ticket" },
          { guardrail_id: 3, rule_id: "external_error:timeout" },
        ],
      }),
    ).toEqual([
      "Checked with mask-emails, house-rules.",
      "Blocked by house-rules.",
      "Redacted: EMAIL 2.",
      "Flagged: ticket.",
      "Could not be asked: timeout.",
    ]);
  });

  test("a direction with nothing but a check says only that", () => {
    expect(sideLines({ action: "flagged", checked_with: [] })).toEqual([]);
    expect(loggedLabel("redacted")).toBe("Redacted");
  });
});
