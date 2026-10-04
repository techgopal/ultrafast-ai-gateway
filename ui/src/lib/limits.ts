// What the console knows of limits and budgets: the forms they are edited in,
// the requests made of the forms (with the ranges the gateway checks, so that
// a form that cannot be sent says why before it is), and how they are said in
// words. Pure functions: the pages ask them, and the gateway decides.
import { ApiError, ConsoleRefusal } from "@/api/errors";
import type { components } from "@/api/schema";
import { dollarsToMicros, microsToDollars } from "@/lib/money";

type LimitView = components["schemas"]["LimitView"];
type BudgetView = components["schemas"]["BudgetView"];
type SetLimitRequest = components["schemas"]["SetLimitRequest"];
type SetBudgetRequest = components["schemas"]["SetBudgetRequest"];

/** What a limit or a budget can be about; the gateway as a whole has no id. */
export const SCOPES = [
  ["gateway", "Gateway"],
  ["team", "Team"],
  ["user", "User"],
  ["key", "Key"],
] as const;

export type Scope = (typeof SCOPES)[number][0];
export type Targeted = Exclude<Scope, "gateway">;

export const PERIODS = [
  ["daily", "Daily"],
  ["weekly", "Weekly"],
  ["monthly", "Monthly"],
] as const;

export const ACTIONS = [
  ["block", "Blocks"],
  ["alert", "Alerts"],
] as const;

/** The scope in words; one the console does not know is shown as it is. */
export function scopeText(scope: string): string {
  return SCOPES.find(([value]) => value === scope)?.[1] ?? scope;
}

export function periodText(period: string): string {
  return PERIODS.find(([value]) => value === period)?.[1] ?? period;
}

export function actionText(action: string): string {
  return ACTIONS.find(([value]) => value === action)?.[1] ?? action;
}

function isTargeted(scope: string): scope is Targeted {
  return scope === "team" || scope === "user" || scope === "key";
}

/** The ids that can be chosen for each scope that has a target. */
export interface OfferedTargets {
  team: readonly number[];
  user: readonly number[];
  key: readonly number[];
}

/**
 * The ids that can be chosen, and the target of the row that is changed: it
 * is not chosen again, so it is offered as long as the row has it (a key
 * that was revoked since is still the key of its budget).
 */
export function offeredWith(
  offered: OfferedTargets,
  row: { scope: string; scope_id: number | null } | null,
): OfferedTargets {
  if (row === null || row.scope_id === null || !isTargeted(row.scope)) return offered;
  return { ...offered, [row.scope]: [...offered[row.scope], row.scope_id] };
}

/** The choice when it is one of the offered ids; otherwise nothing (`""`). */
export function chosenTarget(value: string, offered: readonly number[]): string {
  return offered.some((id) => String(id) === value) ? value : "";
}

const WHOLE = /^\d+$/;
const grouped = new Intl.NumberFormat("en-US");

export const NEEDS_A_LIMIT = "Set at least one limit. To remove the limits, delete them.";

/** The ranges of the numbers of a limit, by the name of the field. */
export const COUNTS = [
  ["requests_per_minute", "Requests per minute", 1_000_000],
  ["tokens_per_minute", "Tokens per minute", 1_000_000_000_000],
  ["concurrent", "Concurrent requests", 1_000_000],
] as const;

type CountName = (typeof COUNTS)[number][0];

function countRule(max: number): string {
  return `Enter a whole number from 1 to ${grouped.format(max)}.`;
}

/** The most a budget can be, in micros: $1,000,000,000. */
const MAX_AMOUNT_MICROS = 1_000_000_000_000_000;
export const AMOUNT_RULE = "Enter dollars above 0, with up to 6 decimals, up to $1,000,000,000.";

const TARGET_NAMES: Record<Targeted, string> = { team: "team", user: "user", key: "key" };

/** What a form holds. Every number is a text, as it is typed. */
export interface LimitForm {
  scope: string;
  /** The id of the team, user or key, as a text; empty while none is chosen. */
  scope_id: string;
  requests_per_minute: string;
  tokens_per_minute: string;
  concurrent: string;
}

export interface BudgetForm {
  scope: string;
  scope_id: string;
  /** In dollars, as it is typed. */
  amount: string;
  period: string;
  action: string;
}

export function emptyLimitForm(): LimitForm {
  return {
    scope: "gateway",
    scope_id: "",
    requests_per_minute: "",
    tokens_per_minute: "",
    concurrent: "",
  };
}

export function emptyBudgetForm(): BudgetForm {
  return { scope: "gateway", scope_id: "", amount: "", period: "monthly", action: "block" };
}

const text = (count: number | null): string => (count === null ? "" : String(count));

export function limitFormOf(row: LimitView): LimitForm {
  return {
    scope: row.scope,
    scope_id: row.scope_id === null ? "" : String(row.scope_id),
    requests_per_minute: text(row.requests_per_minute),
    tokens_per_minute: text(row.tokens_per_minute),
    concurrent: text(row.concurrent),
  };
}

export function budgetFormOf(row: BudgetView): BudgetForm {
  return {
    scope: row.scope,
    scope_id: row.scope_id === null ? "" : String(row.scope_id),
    amount: microsToDollars(row.amount_micros),
    period: row.period,
    action: row.action,
  };
}

/**
 * The scope and the id of the form as the gateway has them. A target that is
 * not offered is never sent; the id of another scope is dropped.
 */
function targetOf(
  scope: string,
  scopeId: string,
  offered: OfferedTargets,
): { scope: Scope; scope_id?: number } {
  if (scope === "gateway") return { scope };
  if (!isTargeted(scope)) throw new ConsoleRefusal("Choose what it applies to.", "scope");
  const id = chosenTarget(scopeId, offered[scope]);
  if (id === "") throw new ConsoleRefusal(`Choose a ${TARGET_NAMES[scope]}.`, "scope_id");
  return { scope, scope_id: Number(id) };
}

/**
 * The request of a form, or a `ConsoleRefusal` about the first field that
 * cannot be sent. Numbers left empty are left out: the request replaces all
 * three limits of the target.
 */
export function limitRequestOf(form: LimitForm, offered: OfferedTargets): SetLimitRequest {
  const request: SetLimitRequest = { ...targetOf(form.scope, form.scope_id, offered) };
  const counts: Partial<Record<CountName, number>> = {};
  for (const [name, , max] of COUNTS) {
    const typed = form[name].trim();
    if (typed === "") continue;
    const value = WHOLE.test(typed) ? Number(typed) : 0;
    if (value < 1 || value > max) throw new ConsoleRefusal(countRule(max), name);
    counts[name] = value;
  }
  if (Object.keys(counts).length === 0) {
    throw new ConsoleRefusal(NEEDS_A_LIMIT, "requests_per_minute");
  }
  return { ...request, ...counts };
}

export function budgetRequestOf(form: BudgetForm, offered: OfferedTargets): SetBudgetRequest {
  const target = targetOf(form.scope, form.scope_id, offered);
  const micros = dollarsToMicros(form.amount);
  if (micros === null || micros < 1 || micros > MAX_AMOUNT_MICROS) {
    throw new ConsoleRefusal(AMOUNT_RULE, "amount");
  }
  if (!PERIODS.some(([value]) => value === form.period)) {
    throw new ConsoleRefusal("Choose a period.", "period");
  }
  if (!ACTIONS.some(([value]) => value === form.action)) {
    throw new ConsoleRefusal("Choose what happens when the amount is spent.", "action");
  }
  return { ...target, amount_micros: micros, period: form.period, action: form.action };
}

/** How much of the amount is spent, in whole percent, rounded down; more than 100 when over. */
export function percentOf(spent: number, amount: number): number {
  if (amount <= 0) return 0;
  return Math.floor((spent / amount) * 100);
}

/**
 * What the gateway says of single fields, by the names of the form: the
 * amount is in dollars in the form and in micros there. The error is still
 * what the gateway answered: its status, its code and its message are kept.
 * Any other error is returned as it is.
 */
export function inFormWords(error: unknown): unknown {
  if (!(error instanceof ApiError) || !("amount_micros" in error.fields)) return error;
  const rest = Object.fromEntries(
    Object.entries(error.fields).filter(([name]) => name !== "amount_micros"),
  );
  return new ApiError(error.status, error.code, error.message, { ...rest, amount: AMOUNT_RULE });
}
