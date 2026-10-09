// What the console knows of a guardrail: the form it is edited in and the
// request that is made of the form, what the form checks before it is sent
// (the gateway still decides), how the faults it names are shown on the rows
// they are about, and how the result of a test is said. Pure functions:
// `pages/GuardrailsEdit` asks them.
import { ApiError } from "@/api/errors";
import type { components } from "@/api/schema";

type Schemas = components["schemas"];
type Guardrail = Schemas["GuardrailView"];
type RuleSpec = Schemas["RuleSpec"];
type Outcome = Schemas["OutcomeView"];

export type PiiType = Schemas["PiiType"];
export type Action = Schemas["Action"];
export type Directions = Schemas["Directions"];
export type MatcherKind = "keywords" | "regex" | "pii";
export type Kind = "rules" | "external";
export type FailMode = "open" | "closed";

/** The PII types the gateway knows, in its order, with what each one finds. */
export const PII_TYPES: readonly { type: PiiType; label: string; hint: string }[] = [
  { type: "EMAIL", label: "Email address", hint: "Addresses with letters of any script." },
  {
    type: "PHONE",
    label: "Phone number",
    hint: "Needs a + or separators; a bare 10-digit number is not found.",
  },
  { type: "CREDIT_CARD", label: "Credit card number", hint: "13 to 19 digits that pass the Luhn check." },
  { type: "IBAN", label: "IBAN", hint: "Checked by its check digits, not by a length for each country." },
  { type: "US_SSN", label: "US Social Security number", hint: "Written 123-45-6789." },
  { type: "IPV4", label: "IPv4 address", hint: "Four numbers up to 255." },
  { type: "IPV6", label: "IPv6 address", hint: "Needs a digit or at least three colons." },
  { type: "SECRET", label: "Secrets", hint: "API keys of common providers and private-key blocks." },
];

export function piiLabel(type: string): string {
  return PII_TYPES.find((one) => one.type === type)?.label ?? type;
}

export const ACTIONS: readonly [Action, string][] = [
  ["redact", "Redact"],
  ["block", "Block"],
  ["flag", "Flag"],
];

export const DIRECTIONS: readonly [Directions, string][] = [
  ["both", "Input and output"],
  ["input", "Input only"],
  ["output", "Output only"],
];

export function actionLabel(action: string): string {
  return ACTIONS.find(([value]) => value === action)?.[1] ?? action;
}

export function directionsLabel(directions: string): string {
  return DIRECTIONS.find(([value]) => value === directions)?.[1] ?? directions;
}

export const TIMEOUT_MIN = 1000;
export const TIMEOUT_MAX = 10000;
const DEFAULT_TIMEOUT = "3000";

/** One rule as it is edited. */
export interface RuleRow {
  /** Tells the row apart while rows are moved; never sent. */
  key: number;
  id: string;
  kind: MatcherKind;
  /** One word or phrase for each line. */
  words: string;
  wholeWord: boolean;
  regex: string;
  pii: PiiType[];
  action: Action;
  directions: Directions;
}

/** What the form holds. The timeout is a text, as it is typed. */
export interface GuardrailForm {
  name: string;
  description: string;
  kind: Kind;
  enabled: boolean;
  is_default: boolean;
  rules: RuleRow[];
  /** Typed to set or change the URL; the gateway never shows it, so this starts empty. */
  url: string;
  directions: Directions;
  fail_mode: FailMode;
  timeout_ms: string;
}

let nextKey = 1;

export function newRule(): RuleRow {
  nextKey += 1;
  return {
    key: nextKey,
    id: "",
    kind: "pii",
    words: "",
    wholeWord: true,
    regex: "",
    pii: ["EMAIL"],
    action: "redact",
    directions: "both",
  };
}

export function emptyForm(): GuardrailForm {
  return {
    name: "",
    description: "",
    kind: "rules",
    enabled: true,
    is_default: false,
    rules: [{ ...newRule(), id: "email" }],
    url: "",
    directions: "both",
    fail_mode: "open",
    timeout_ms: DEFAULT_TIMEOUT,
  };
}

function rowOf(rule: RuleSpec): RuleRow {
  const row = { ...newRule(), id: rule.id, action: rule.action, directions: rule.directions };
  const { matcher } = rule;
  if ("keywords" in matcher) {
    return {
      ...row,
      kind: "keywords",
      words: matcher.keywords.words.join("\n"),
      wholeWord: matcher.keywords.whole_word ?? true,
      pii: [],
    };
  }
  if ("regex" in matcher) return { ...row, kind: "regex", regex: matcher.regex, pii: [] };
  return { ...row, kind: "pii", pii: [...matcher.pii] };
}

export function formOf(guardrail: Guardrail | null): GuardrailForm {
  if (guardrail === null) return emptyForm();
  return {
    name: guardrail.name,
    description: guardrail.description,
    kind: guardrail.kind === "external" ? "external" : "rules",
    enabled: guardrail.enabled,
    is_default: guardrail.is_default,
    rules: guardrail.rules.map(rowOf),
    url: "",
    directions: guardrail.directions ?? "both",
    fail_mode: guardrail.fail_mode === "closed" ? "closed" : "open",
    timeout_ms: String(guardrail.timeout_ms ?? DEFAULT_TIMEOUT),
  };
}

/** The lines of a list of words: trimmed, and without the blank ones. */
export function wordsOf(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== "");
}

/** The rule a row stands for. */
export function ruleOf(row: RuleRow): RuleSpec {
  const matcher: RuleSpec["matcher"] =
    row.kind === "keywords"
      ? { keywords: { words: wordsOf(row.words), whole_word: row.wholeWord } }
      : row.kind === "regex"
        ? { regex: row.regex }
        : { pii: row.pii };
  return { id: row.id.trim(), matcher, action: row.action, directions: row.directions };
}

/** Every field a request may have; it fits both the creating and the changing request. */
export interface GuardrailBody {
  name: string;
  description: string;
  kind?: string;
  enabled: boolean;
  is_default: boolean;
  rules?: RuleSpec[];
  url?: string;
  directions?: Directions;
  fail_mode?: string;
  timeout_ms?: number;
}

/**
 * The request for the form. `existing` is the guardrail that is changed, or
 * `null` for a new one: the kind is sent when it is made and never after, and
 * a URL is sent only when one is typed.
 */
export function requestOf(form: GuardrailForm, existing: Guardrail | null): GuardrailBody {
  const body: GuardrailBody = {
    name: form.name.trim(),
    description: form.description.trim(),
    enabled: form.enabled,
    is_default: form.is_default,
  };
  if (existing === null) body.kind = form.kind;
  if (form.kind === "rules") {
    body.rules = form.rules.map(ruleOf);
    return body;
  }
  if (form.url.trim() !== "") body.url = form.url.trim();
  body.directions = form.directions;
  body.fail_mode = form.fail_mode;
  body.timeout_ms = Number(form.timeout_ms.trim());
  return body;
}

export interface Problems {
  fields: Partial<Record<"name" | "rules" | "url" | "timeout_ms", string>>;
  /** By the index of the row. */
  rows: Record<number, string>;
}

export function hasProblems(problems: Problems): boolean {
  return Object.keys(problems.fields).length > 0 || Object.keys(problems.rows).length > 0;
}

function rowProblem(row: RuleRow): string | undefined {
  if (row.id.trim() === "") return "Enter an id for the rule.";
  if (row.kind === "keywords" && wordsOf(row.words).length === 0) return "Enter at least one word.";
  if (row.kind === "regex" && row.regex.trim() === "") return "Enter a regular expression.";
  if (row.kind === "pii" && row.pii.length === 0) return "Choose at least one type.";
  return undefined;
}

/** What is plainly wrong with the form. The gateway checks the rest (a regex that does not compile). */
export function check(form: GuardrailForm, existing: Guardrail | null = null): Problems {
  const problems: Problems = { fields: {}, rows: {} };
  if (form.name.trim() === "") problems.fields.name = "Enter a name.";
  if (form.kind === "rules") {
    if (form.rules.length === 0) problems.fields.rules = "Add at least one rule.";
    const seen = new Set<string>();
    form.rules.forEach((row, index) => {
      const id = row.id.trim();
      const problem =
        rowProblem(row) ?? (id !== "" && seen.has(id) ? "Another rule has this id." : undefined);
      seen.add(id);
      if (problem !== undefined) problems.rows[index] = problem;
    });
    return problems;
  }
  if (existing === null && form.url.trim() === "") problems.fields.url = "Enter the URL to post to.";
  const timeout = form.timeout_ms.trim();
  const ms = /^\d+$/.test(timeout) ? Number(timeout) : Number.NaN;
  if (!(ms >= TIMEOUT_MIN && ms <= TIMEOUT_MAX)) {
    problems.fields.timeout_ms = `Enter ${String(TIMEOUT_MIN)} to ${String(TIMEOUT_MAX)} milliseconds.`;
  }
  return problems;
}

const RULE_KEY = /^rules\[(\d+)\](?:\..+)?$/;

export interface RuleErrors {
  /** The fault of each row, by index. */
  rows: Record<number, string>;
  /** A fault of the set of rules (too many, none). */
  set: string | undefined;
  /** The error without those faults, for the form to show the rest. */
  rest: unknown;
}

/**
 * The faults the gateway names for rules (`rules[2]`, `rules[2].kind`,
 * `rules[2].types`, `rules`), set apart so that each shows on its row.
 */
export function ruleErrorsOf(error: unknown): RuleErrors {
  if (!(error instanceof ApiError)) return { rows: {}, set: undefined, rest: error };
  const rows: Record<number, string> = {};
  let set: string | undefined;
  const rest: Record<string, string> = {};
  for (const [name, text] of Object.entries(error.fields)) {
    const match = RULE_KEY.exec(name);
    if (match?.[1] !== undefined) rows[Number(match[1])] ??= text;
    else if (name === "rules") set = text;
    else rest[name] = text;
  }
  if (set === undefined && Object.keys(rows).length === 0) return { rows, set, rest: error };
  return {
    rows,
    set,
    rest: new ApiError(error.status, error.code, error.message, rest, error.retryAfter),
  };
}

/** The text split at the placeholders a redaction leaves: `[REDACTED]` and `[REDACTED:EMAIL]`. */
export function segmentsOf(text: string): { text: string; redacted: boolean }[] {
  const out: { text: string; redacted: boolean }[] = [];
  let at = 0;
  for (const match of text.matchAll(/\[REDACTED(?::[A-Z0-9_]+)?\]/g)) {
    if (match.index > at) out.push({ text: text.slice(at, match.index), redacted: false });
    out.push({ text: match[0], redacted: true });
    at = match.index + match[0].length;
  }
  if (at < text.length) out.push({ text: text.slice(at), redacted: false });
  return out;
}

const EXTERNAL_ERROR = "external_error:";

/** What a test found, said in counts and names; never the text that matched. */
export function verdictOf(outcome: Outcome): string[] {
  const lines: string[] = [];
  if (outcome.blocked_by !== null) lines.push(`Blocked by ${outcome.blocked_by.name}.`);
  const redactions = Object.entries(outcome.redactions);
  if (redactions.length > 0) {
    lines.push(`Redacted: ${redactions.map(([label, count]) => `${label} ${String(count)}`).join(", ")}.`);
  }
  const failures = outcome.flags.filter((flag) => flag.rule_id.startsWith(EXTERNAL_ERROR));
  const flags = outcome.flags.filter((flag) => !flag.rule_id.startsWith(EXTERNAL_ERROR));
  if (flags.length > 0) lines.push(`Flagged: ${flags.map((flag) => flag.rule_id).join(", ")}.`);
  for (const flag of failures) {
    lines.push(
      `${flag.guardrail_name} could not be asked: ${flag.rule_id.slice(EXTERNAL_ERROR.length)}.`,
    );
  }
  return lines.length === 0 ? ["Nothing found."] : lines;
}

/** The list with the item at `index` one place `by` (-1 up, 1 down); the ends stay. */
export function moved<T>(list: readonly T[], index: number, by: -1 | 1): T[] {
  const target = index + by;
  const copy = [...list];
  const item = copy[index];
  if (item === undefined || target < 0 || target >= copy.length) return copy;
  copy.splice(index, 1);
  copy.splice(target, 0, item);
  return copy;
}

/**
 * The `guardrail_ids` to send for a route: left out when they are as they
 * were (the gateway then leaves the attachment alone, whatever changed in
 * the meantime), the whole list when they are not. `before` is `null` for a
 * new route, which has none to begin with.
 */
export function attachmentOf(
  next: readonly number[],
  before: readonly number[] | null,
): number[] | undefined {
  const was = before ?? [];
  const same = next.length === was.length && next.every((id, index) => id === was[index]);
  return same ? undefined : [...next];
}

type Logged = Schemas["LoggedAction"];
type Side = Schemas["SideLog"];

export const LOGGED_ACTIONS: readonly [Logged, string][] = [
  ["blocked", "Blocked"],
  ["redacted", "Redacted"],
  ["flagged", "Flagged"],
];

export function loggedLabel(action: string): string {
  return LOGGED_ACTIONS.find(([value]) => value === action)?.[1] ?? action;
}

/** What the guardrails did in one direction of a call, as lines: names, counts and rule ids only. */
export function sideLines(side: Side): string[] {
  const lines: string[] = [];
  if (side.checked_with.length > 0) {
    lines.push(`Checked with ${side.checked_with.map((one) => one.name).join(", ")}.`);
  }
  if (side.blocked_by !== undefined && side.blocked_by !== null) {
    lines.push(`Blocked by ${side.blocked_by.name}.`);
  }
  const redactions = Object.entries(side.redactions ?? {});
  if (redactions.length > 0) {
    lines.push(`Redacted: ${redactions.map(([label, count]) => `${label} ${String(count)}`).join(", ")}.`);
  }
  const flags = side.flags ?? [];
  const failures = flags.filter((flag) => flag.rule_id.startsWith(EXTERNAL_ERROR));
  const rules = flags.filter((flag) => !flag.rule_id.startsWith(EXTERNAL_ERROR));
  if (rules.length > 0) lines.push(`Flagged: ${rules.map((flag) => flag.rule_id).join(", ")}.`);
  if (failures.length > 0) {
    lines.push(
      `Could not be asked: ${failures.map((flag) => flag.rule_id.slice(EXTERNAL_ERROR.length)).join(", ")}.`,
    );
  }
  return lines;
}

/**
 * A refusal of the gateway about one field of a test, in words: a rule by its
 * place (`rules[1].kind` is "Rule 2"), the other fields by the label the form
 * gives them. A field this does not know is said by its message alone, so a
 * field path is never shown.
 */
export function fieldProblem(path: string, message: string): string {
  const rule = /^rules\[(\d+)\]/.exec(path);
  if (rule?.[1] !== undefined) return `Rule ${String(Number(rule[1]) + 1)}: ${message}`;
  const labels: Record<string, string> = {
    guardrail_id: "Guardrail",
    text: "Text",
    direction: "Check as",
  };
  const label = labels[path];
  if (label !== undefined) return `${label}: ${message}`;
  return message.charAt(0).toUpperCase() + message.slice(1);
}

/** `1st`, `2nd`, `3rd`, `4th`, `11th`, `21st`. */
export function ordinal(n: number): string {
  const teen = n % 100 >= 11 && n % 100 <= 13;
  const suffix = teen ? "th" : (["th", "st", "nd", "rd"][n % 10] ?? "th");
  return `${String(n)}${suffix}`;
}
