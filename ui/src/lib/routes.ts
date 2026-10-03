// What the console knows of a route: the form it is edited in and the request
// that is made of the form, the rules the gateway checks (so that a form that
// cannot be sent says why before it is), and how a route and the health of its
// targets are said in words. Pure functions: `pages/RoutesEdit` asks them, and
// the gateway decides.
import { ApiError } from "@/api/errors";
import type { components } from "@/api/schema";

type Route = components["schemas"]["RouteView"];
type RouteRequest = components["schemas"]["RouteRequest"];
type TargetHealth = components["schemas"]["TargetHealth"];
type TargetState = components["schemas"]["TargetState"];

/** Who may use the route. `admins` is a route that is for admins alone. */
export type Audience = "all" | "chosen" | "admins";

export interface PrimaryRow {
  /** The id of the model, as a text; empty while none is chosen. */
  model: string;
  weight: string;
}

/** What the form holds. Every number is a text, as it is typed. */
export interface RouteForm {
  name: string;
  primaries: PrimaryRow[];
  /** The ids of the models, in the order they are tried. */
  fallbacks: string[];
  audience: Audience;
  team_ids: string[];
  retries: string;
  first_token_s: string;
  total_s: string;
  breaker_failures: string;
  breaker_window_s: string;
  breaker_open_s: string;
  /** Answers are kept for a time, and given again to the same asking. */
  cache_enabled: boolean;
  cache_ttl_s: string;
  /** Whose asking counts as the same: see `CACHE_SCOPES`. */
  cache_scope: string;
}

/** Whose calls share a cached answer, in the order they are offered. */
export const CACHE_SCOPES = [
  ["team", "Team"],
  ["key", "Key"],
  ["user", "User"],
] as const;

const DEFAULT_CACHE_SCOPE = "team";

/** The settings of a route that is made without any: what the gateway has. */
export const DEFAULTS = {
  retries: "2",
  first_token_s: "30",
  total_s: "300",
  breaker_failures: "5",
  breaker_window_s: "60",
  breaker_open_s: "30",
  cache_ttl_s: "300",
} as const;

/** A new route is for admins alone, as a new model is granted to nobody. */
export function emptyForm(): RouteForm {
  return {
    name: "",
    primaries: [],
    fallbacks: [],
    audience: "admins",
    team_ids: [],
    ...DEFAULTS,
    cache_enabled: false,
    cache_scope: "team",
  };
}

function secondsText(milliseconds: number): string {
  return String(milliseconds / 1000);
}

/** The form of a route as an admin sees it. */
export function formOf(route: Route): RouteForm {
  return {
    name: route.name,
    primaries: route.primaries.map((p) => ({ model: String(p.model_id), weight: String(p.weight) })),
    fallbacks: route.fallbacks.map((f) => String(f.model_id)),
    audience: route.everyone ? "all" : route.team_ids.length > 0 ? "chosen" : "admins",
    team_ids: route.team_ids.map(String),
    retries: String(route.retries),
    first_token_s: secondsText(route.first_token_timeout_ms),
    total_s: secondsText(route.total_timeout_ms),
    breaker_failures: String(route.breaker_failures),
    breaker_window_s: String(route.breaker_window_s),
    breaker_open_s: String(route.breaker_open_s),
    cache_enabled: route.cache_enabled,
    cache_ttl_s: String(route.cache_ttl_s),
    cache_scope: route.cache_scope,
  };
}

// ----------------------------------------------------------------- rules

const NAME = /^[a-z0-9][a-z0-9._-]{0,63}$/;
const WHOLE = /^\d+$/;
const SECONDS = /^\d+(\.\d{1,3})?$/;

export const NAME_RULE =
  "Use 1 to 64 characters: a-z, 0-9, '.', '_' and '-', starting with a letter or a digit.";
export const NEEDS_A_PRIMARY = "Add at least one primary target.";
export const CHOOSE_A_MODEL = "Choose a model.";
export const ALREADY_IN_THE_ROUTE = "This model is already in the route.";
export const WEIGHT_RULE = "Enter a whole number from 1 to 1000.";
export const CHOOSE_A_TEAM = "Choose at least one team, or choose Admins only.";
export const TOTAL_BELOW_FIRST = "Must not be below the first token timeout.";

/** The limits of the numbers the gateway takes; the times in seconds, as the form has them. */
const LIMITS = {
  retries: { kind: "whole", min: 0, max: 5 },
  first_token_s: { kind: "seconds", min: 1, max: 300 },
  total_s: { kind: "seconds", min: 1, max: 3600 },
  breaker_failures: { kind: "whole", min: 1, max: 100 },
  breaker_window_s: { kind: "seconds", min: 5, max: 3600 },
  breaker_open_s: { kind: "seconds", min: 5, max: 3600 },
  // The gateway checks it whether the cache is on or not.
  cache_ttl_s: { kind: "whole", min: 1, max: 86_400 },
} as const;

type Setting = keyof typeof LIMITS;
export const SETTINGS = Object.keys(LIMITS) as Setting[];

function rangeText(setting: Setting): string {
  const { kind, min, max } = LIMITS[setting];
  return kind === "whole"
    ? `Enter a whole number from ${String(min)} to ${String(max)}.`
    : `Enter seconds from ${String(min)} to ${String(max)}.`;
}

/** The number a setting holds, or `null` when it is not one within its limits. */
function valueOf(setting: Setting, text: string): number | null {
  const { kind, min, max } = LIMITS[setting];
  const pattern = kind === "whole" ? WHOLE : SECONDS;
  if (!pattern.test(text)) return null;
  const value = Number(text);
  return value >= min && value <= max ? value : null;
}

export interface Offered {
  /** The ids of the models a target can be. */
  models: readonly number[];
  /** The ids of the teams that can be chosen. */
  teams: readonly number[];
}

export interface RowProblem {
  model?: string;
  weight?: string;
}

export type FieldName = "name" | "primaries" | "team_ids" | "cache_scope" | Setting;

export interface Problems {
  fields: Partial<Record<FieldName, string>>;
  /** One for each primary row; an empty one has no problem. */
  primaries: RowProblem[];
  /** One for each fallback row. */
  fallbacks: (string | undefined)[];
}

/** What the form must be to be sent, as the gateway checks it. */
export function check(form: RouteForm, offered: Offered): Problems {
  const fields: Problems["fields"] = {};
  if (!NAME.test(form.name.trim())) fields.name = NAME_RULE;
  if (form.primaries.length === 0) fields.primaries = NEEDS_A_PRIMARY;

  // A model appears once in a route, as a primary or as a fallback.
  const seen = new Set<number>();
  function modelProblem(model: string): string | undefined {
    const id = WHOLE.test(model) ? Number(model) : null;
    if (id === null || !offered.models.includes(id)) return CHOOSE_A_MODEL;
    if (seen.has(id)) return ALREADY_IN_THE_ROUTE;
    seen.add(id);
    return undefined;
  }
  const primaries = form.primaries.map((row): RowProblem => {
    const problem: RowProblem = {};
    const model = modelProblem(row.model);
    if (model !== undefined) problem.model = model;
    const weight = Number(row.weight);
    if (!WHOLE.test(row.weight) || weight < 1 || weight > 1000) problem.weight = WEIGHT_RULE;
    return problem;
  });
  const fallbacks = form.fallbacks.map(modelProblem);

  const numbers = new Map<Setting, number>();
  for (const setting of SETTINGS) {
    const value = valueOf(setting, form[setting]);
    if (value === null) fields[setting] = rangeText(setting);
    else numbers.set(setting, value);
  }
  const first = numbers.get("first_token_s");
  const total = numbers.get("total_s");
  if (first !== undefined && total !== undefined && total < first) fields.total_s = TOTAL_BELOW_FIRST;

  if (form.audience === "chosen") {
    const some = form.team_ids.some((id) => offered.teams.includes(Number(id)));
    if (!some) fields.team_ids = CHOOSE_A_TEAM;
  }
  return { fields, primaries, fallbacks };
}

export function hasProblems(problems: Problems): boolean {
  return (
    Object.keys(problems.fields).length > 0 ||
    problems.primaries.some((row) => Object.keys(row).length > 0) ||
    problems.fallbacks.some((row) => row !== undefined)
  );
}

function milliseconds(text: string): number {
  return Math.round(Number(text) * 1000);
}

/**
 * The request of a form that has no problem. A team that is no longer
 * offered is not sent (the gateway would refuse the id); with everyone or
 * with admins only there is none.
 */
export function requestOf(form: RouteForm, offered: Offered): RouteRequest {
  const teams =
    form.audience === "chosen"
      ? form.team_ids.map(Number).filter((id) => offered.teams.includes(id))
      : [];
  return {
    name: form.name.trim(),
    everyone: form.audience === "all",
    primaries: form.primaries.map((row) => ({ model_id: Number(row.model), weight: Number(row.weight) })),
    fallbacks: form.fallbacks.map(Number),
    retries: Number(form.retries),
    first_token_timeout_ms: milliseconds(form.first_token_s),
    total_timeout_ms: milliseconds(form.total_s),
    breaker_failures: Number(form.breaker_failures),
    breaker_window_s: Number(form.breaker_window_s),
    breaker_open_s: Number(form.breaker_open_s),
    cache_enabled: form.cache_enabled,
    cache_ttl_s: Number(form.cache_ttl_s),
    // A scope that is not offered is not sent.
    cache_scope: CACHE_SCOPES.some(([value]) => value === form.cache_scope)
      ? form.cache_scope
      : DEFAULT_CACHE_SCOPE,
    team_ids: teams,
  };
}

/** The names the gateway has for the fields the form holds under other names. */
const FORM_FIELD: Record<string, string> = {
  first_token_timeout_ms: "first_token_s",
  total_timeout_ms: "total_s",
  everyone: "audience",
};

/**
 * What the gateway says of single fields, by the names and in the words of
 * the form: a time is in seconds in the form and in milliseconds there, so a
 * range it names is the form's. The other messages are the gateway's.
 */
export function gatewayFields(fields: Readonly<Record<string, string>>): Record<string, string> {
  const result: Record<string, string> = {};
  for (const [name, message] of Object.entries(fields)) {
    const own = FORM_FIELD[name] ?? name;
    if (own === "total_s" && message.startsWith("must not be below")) {
      result[own] = TOTAL_BELOW_FIRST;
    } else if (own in LIMITS) {
      result[own] = rangeText(own as Setting);
    } else {
      result[own] = message;
    }
  }
  return result;
}

// ----------------------------------------------------------------- words

/** A target as a call names it, with its weight: "openai/gpt-4o-mini ×3". */
export function targetText(target: { model: string; weight: number }): string {
  return `${target.model} ×${String(target.weight)}`;
}

/** Who may use the route, in a few words. */
export function teamsText(route: Pick<Route, "everyone" | "team_ids">): string {
  if (route.everyone) return "All teams";
  const count = route.team_ids.length;
  if (count === 0) return "Admins only";
  return `${String(count)} ${count === 1 ? "team" : "teams"}`;
}

const STATES: Record<TargetState, string> = {
  closed: "Healthy",
  open: "Failing",
  half_open: "Testing",
};

export function healthText(state: TargetState): string {
  return STATES[state];
}

/** The health of the targets that belong to the route. */
export function healthFor(route: Route, targets: readonly TargetHealth[]): TargetHealth[] {
  const models = new Set([...route.primaries, ...route.fallbacks].map((t) => t.model));
  return targets.filter((target) => models.has(`${target.provider}/${target.model}`));
}

/** The list with the item at `index` moved by `by` places; at an end it stays. The list is not changed. */
export function moved<T>(list: readonly T[], index: number, by: -1 | 1): T[] {
  const to = index + by;
  const item = list[index];
  if (item === undefined || to < 0 || to >= list.length) return [...list];
  const rest = list.filter((_, at) => at !== index);
  rest.splice(to, 0, item);
  return rest;
}

/**
 * What the gateway answered, with its fields by the names and in the words of
 * the form (`gatewayFields`). It is still what the gateway answered: its
 * status, its code and its message are kept. Any other error is returned as it is.
 */
export function inFormWords(error: unknown): unknown {
  if (!(error instanceof ApiError) || Object.keys(error.fields).length === 0) return error;
  return new ApiError(error.status, error.code, error.message, gatewayFields(error.fields));
}

/**
 * A gateway error about `team_ids`, while the form does not ask for teams
 * (the audience is not "chosen"): no field shows it, so it is named by the
 * words of the choice and said with the form's other messages. Any other
 * error is returned as it is.
 */
export function teamsErrorAside(error: unknown, audience: Audience): unknown {
  if (audience === "chosen" || !(error instanceof ApiError) || !("team_ids" in error.fields)) {
    return error;
  }
  const { team_ids: text, ...rest } = error.fields;
  return new ApiError(error.status, error.code, error.message, {
    ...rest,
    "Chosen teams": text,
  });
}
