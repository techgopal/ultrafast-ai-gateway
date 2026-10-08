// What the alerts page knows of rules, subjects and deliveries: the parameters
// of each kind of rule as the console types them, the words for a condition,
// and the form of a rule with the request it makes.
import { ApiError, ConsoleRefusal } from "@/api/errors";
import type { components } from "@/api/schema";

type Budget = components["schemas"]["BudgetView"];
type Rule = components["schemas"]["RuleView"];
type CreateRuleRequest = components["schemas"]["CreateRuleRequest"];

export const KINDS = [
  ["budget", "Budget"],
  ["error_rate", "Error rate"],
  ["circuit_open", "Circuit"],
] as const;

export function kindText(kind: string): string {
  return KINDS.find(([value]) => value === kind)?.[1] ?? kind;
}

export const SCOPES = [
  ["gateway", "Gateway"],
  ["route", "Route"],
  ["provider", "Provider"],
  ["key", "Key"],
] as const;

type Scope = (typeof SCOPES)[number][0];

function isScope(value: unknown): value is Scope {
  return SCOPES.some(([scope]) => scope === value);
}

/** The parameters of a rule, by kind. */
export type Params =
  | { kind: "budget"; budget_id: number | null; percent: number }
  | {
      kind: "error_rate";
      scope: Scope;
      subject: string | null;
      percent: number;
      window_minutes: number;
      min_requests: number;
    }
  | { kind: "circuit_open"; provider: string | null; model: string | null };

function field(params: unknown, name: string): unknown {
  return typeof params === "object" && params !== null ? Reflect.get(params, name) : undefined;
}

const isCount = (value: unknown): value is number =>
  typeof value === "number" && Number.isInteger(value);
const isNameOrNull = (value: unknown): value is string | null =>
  value === null || typeof value === "string";

/** The parameters, when they are of the shape the kind has; `null` otherwise. */
export function paramsOf(rule: { kind: string; params: unknown }): Params | null {
  const { params } = rule;
  if (rule.kind === "budget") {
    const budgetId = field(params, "budget_id") ?? null;
    const percent = field(params, "percent");
    if ((budgetId === null || isCount(budgetId)) && isCount(percent)) {
      return { kind: "budget", budget_id: budgetId, percent };
    }
  }
  if (rule.kind === "error_rate") {
    const scope = field(params, "scope");
    const subject = field(params, "subject") ?? null;
    const percent = field(params, "percent");
    const windowMinutes = field(params, "window_minutes");
    const minRequests = field(params, "min_requests");
    if (
      isScope(scope) &&
      isNameOrNull(subject) &&
      isCount(percent) &&
      isCount(windowMinutes) &&
      isCount(minRequests)
    ) {
      return {
        kind: "error_rate",
        scope,
        subject,
        percent,
        window_minutes: windowMinutes,
        min_requests: minRequests,
      };
    }
  }
  if (rule.kind === "circuit_open") {
    const provider = field(params, "provider") ?? null;
    const model = field(params, "model") ?? null;
    if (isNameOrNull(provider) && isNameOrNull(model)) {
      return { kind: "circuit_open", provider, model };
    }
  }
  return null;
}

/** What the words about a rule look things up in. */
export interface Lookups {
  budgets: readonly Budget[];
  keys: readonly { id: number; name: string }[];
}

/** A budget as the page names it: `team Platform weekly`. */
export function budgetName(budget: Pick<Budget, "label" | "period">): string {
  return `${budget.label.replaceAll("'", "")} ${budget.period}`;
}

function budgetText(id: number, lookups: Lookups): string {
  const found = lookups.budgets.find((budget) => budget.id === id);
  return found === undefined ? String(id) : budgetName(found);
}

function keyText(id: string, lookups: Lookups): string {
  return lookups.keys.find((key) => String(key.id) === id)?.name ?? id;
}

const NO_CONDITION = "Unknown condition";

/** The condition of a rule in words. */
export function conditionText(rule: { kind: string; params: unknown }, lookups: Lookups): string {
  const params = paramsOf(rule);
  if (params === null) return NO_CONDITION;
  switch (params.kind) {
    case "budget":
      return params.budget_id === null
        ? `Any budget at ${String(params.percent)}%`
        : `Budget ${
            lookups.budgets.some((budget) => budget.id === params.budget_id)
              ? `'${budgetText(params.budget_id, lookups)}'`
              : String(params.budget_id)
          } at ${String(params.percent)}%`;
    case "error_rate": {
      const head = `Errors ≥ ${String(params.percent)}% over ${String(params.window_minutes)} min on`;
      if (params.scope === "gateway") return `${head} the gateway`;
      if (params.subject === null) return `${head} each ${params.scope}`;
      return params.scope === "key"
        ? `${head} key ${keyText(params.subject, lookups)}`
        : `${head} ${params.scope} ${params.subject}`;
    }
    case "circuit_open":
      if (params.provider !== null && params.model !== null) {
        return `Circuit opens on ${params.provider}/${params.model}`;
      }
      if (params.provider !== null) return `Circuit opens on any model of ${params.provider}`;
      if (params.model !== null) return `Circuit opens on model ${params.model} of any provider`;
      return "Circuit opens on any target";
  }
}

/**
 * What an event or a firing is about, from its subject: `route:chat`,
 * `key:12`, `target:openai/gpt-4.1`, `budget:<id>:<period start>`.
 */
export function subjectText(subject: string, lookups: Lookups): string {
  if (subject === "gateway") return "the gateway";
  if (subject.startsWith("channel:")) return "a channel test";
  if (subject.startsWith("target:")) return subject.slice("target:".length);
  if (subject.startsWith("key:")) return `key ${keyText(subject.slice("key:".length), lookups)}`;
  if (subject.startsWith("route:") || subject.startsWith("provider:")) {
    return subject.replace(":", " ");
  }
  const budget = /^budget:(\d+):(.+)$/.exec(subject);
  if (budget !== null) {
    const [, id = "", start = ""] = budget;
    const found = lookups.budgets.find((one) => String(one.id) === id);
    return `${found === undefined ? `budget ${id}` : budgetName(found)} from ${start}`;
  }
  return subject;
}

/** One delivery of an event to a channel, as the gateway records it. */
export interface Delivery {
  channel_id: number;
  channel_name: string;
  ok: boolean;
  status: number | null;
  tries: number;
  error: string | null;
}

function isDelivery(value: unknown): value is Delivery {
  return (
    typeof field(value, "channel_name") === "string" &&
    typeof field(value, "ok") === "boolean" &&
    isCount(field(value, "tries"))
  );
}

/** The deliveries of an event; what is not one is left out. */
export function deliveriesOf(event: { deliveries: unknown }): Delivery[] {
  return Array.isArray(event.deliveries) ? (event.deliveries as unknown[]).filter(isDelivery) : [];
}

export function deliverySummary(deliveries: readonly Delivery[], noChannels = false): string {
  if (deliveries.length === 0) return noChannels ? "No channels" : "None yet";
  const delivered = deliveries.filter((one) => one.ok).length;
  return delivered === deliveries.length
    ? `${String(delivered)} delivered`
    : `${String(delivered)} of ${String(deliveries.length)} delivered`;
}

export function deliveryLine(delivery: Delivery): string {
  if (delivery.ok) {
    return `${delivery.channel_name}: delivered${
      delivery.status === null ? "" : ` (${String(delivery.status)})`
    }`;
  }
  const tries =
    delivery.tries === 0 ? "" : ` (${String(delivery.tries)} ${delivery.tries === 1 ? "try" : "tries"})`;
  return `${delivery.channel_name}: failed${delivery.error === null ? "" : `, ${delivery.error}`}${tries}`;
}

// ------------------------------------------------------------- the form

/** The choice that leaves a parameter out: any budget, each route, any model. */
export const ANY = "any one";

/** What a form holds. Every number is a text, as it is typed. */
export interface RuleForm {
  name: string;
  kind: string;
  budget_id: string;
  percent: string;
  scope: string;
  subject: string;
  window_minutes: string;
  min_requests: string;
  provider: string;
  model: string;
  channel_ids: string[];
}

/** What the page offers to choose from. A choice that is not among them is never sent. */
export interface Offered {
  budgets: readonly number[];
  routes: readonly string[];
  providers: readonly string[];
  keys: readonly number[];
  models: readonly { provider: string; name: string }[];
  channels: readonly number[];
}

/** The percent a new rule of the kind starts with. */
export const DEFAULT_PERCENT: Readonly<Record<string, string>> = {
  budget: "80",
  error_rate: "10",
};

export const DEFAULT_WINDOW = "5";
export const DEFAULT_MIN_REQUESTS = "20";

export function emptyRuleForm(): RuleForm {
  return {
    name: "",
    kind: "budget",
    budget_id: ANY,
    percent: DEFAULT_PERCENT.budget ?? "80",
    scope: "gateway",
    subject: ANY,
    window_minutes: DEFAULT_WINDOW,
    min_requests: DEFAULT_MIN_REQUESTS,
    provider: ANY,
    model: ANY,
    channel_ids: [],
  };
}

const orAny = (value: string | null): string => value ?? ANY;

export function ruleFormOf(rule: Rule): RuleForm {
  const form: RuleForm = {
    ...emptyRuleForm(),
    name: rule.name,
    kind: rule.kind,
    channel_ids: rule.channels.map((channel) => String(channel.id)),
  };
  const params = paramsOf(rule);
  if (params?.kind === "budget") {
    form.budget_id = params.budget_id === null ? ANY : String(params.budget_id);
    form.percent = String(params.percent);
  } else if (params?.kind === "error_rate") {
    form.scope = params.scope;
    form.subject = orAny(params.subject);
    form.percent = String(params.percent);
    form.window_minutes = String(params.window_minutes);
    form.min_requests = String(params.min_requests);
  } else if (params?.kind === "circuit_open") {
    form.provider = orAny(params.provider);
    form.model = orAny(params.model);
  }
  return form;
}

const WHOLE = /^\d+$/;

function whole(typed: string, low: number, high: number, name: string): number {
  const text = typed.trim();
  const value = WHOLE.test(text) ? Number(text) : 0;
  if (value < low || value > high) {
    throw new ConsoleRefusal(`Enter a whole number from ${String(low)} to ${String(high)}.`, name);
  }
  return value;
}

/** The choice if it is offered; any otherwise. */
export function chosen(choice: string, offered: readonly (string | number)[]): string {
  return offered.some((one) => String(one) === choice) ? choice : ANY;
}

/**
 * The API description types the parameters as `Record<string, never>`, which
 * no object satisfies. The parameters are built by kind, here, and the
 * gateway checks them (`paramsOf` reads them the same way).
 */
function free(value: object): CreateRuleRequest["params"] {
  return value as CreateRuleRequest["params"];
}

function paramsRequest(
  form: RuleForm,
  offered: Offered,
  untouched: (field: keyof RuleForm) => boolean,
): CreateRuleRequest["params"] {
  // A choice the rule already had is kept as it is, offered or not; a choice made now must be offered.
  const pick = (field: "budget_id" | "subject" | "provider", choices: readonly (string | number)[]) =>
    untouched(field) ? form[field] : chosen(form[field], choices);
  switch (form.kind) {
    case "budget": {
      const budget = pick("budget_id", offered.budgets);
      return free({
        budget_id: budget === ANY ? null : Number(budget),
        percent: whole(form.percent, 1, 100, "percent"),
      });
    }
    case "error_rate": {
      const scope = SCOPES.find(([value]) => value === form.scope)?.[0] ?? "gateway";
      const choices =
        scope === "route" ? offered.routes : scope === "provider" ? offered.providers : offered.keys;
      const subject = scope === "gateway" ? ANY : pick("subject", choices);
      return free({
        scope,
        subject: subject === ANY ? null : subject,
        percent: whole(form.percent, 1, 100, "percent"),
        window_minutes: whole(form.window_minutes, 5, 60, "window_minutes"),
        min_requests: whole(form.min_requests, 1, 100_000, "min_requests"),
      });
    }
    default: {
      const provider = pick("provider", offered.providers);
      const model =
        (untouched("model") && untouched("provider")) ||
        offered.models.some(
          (one) => one.name === form.model && (provider === ANY || one.provider === provider),
        )
          ? form.model
          : ANY;
      return free({
        provider: provider === ANY ? null : provider,
        model: model === ANY ? null : model,
      });
    }
  }
}

type UpdateRuleRequest = components["schemas"]["UpdateRuleRequest"];

function sameJson(a: unknown, b: unknown): boolean {
  const sorted = (value: unknown): unknown =>
    typeof value === "object" && value !== null && !Array.isArray(value)
      ? Object.fromEntries(
          Object.entries(value)
            .sort(([x], [y]) => x.localeCompare(y))
            .map(([key, one]) => [key, sorted(one)]),
        )
      : value;
  return JSON.stringify(sorted(a)) === JSON.stringify(sorted(b));
}

/**
 * What of the request differs from the rule, for a PATCH. New parameters make
 * the gateway forget what the rule was firing for, so they are sent only when
 * they changed. The kind cannot change and is never sent.
 */
export function ruleChangesOf(rule: Rule, request: CreateRuleRequest): UpdateRuleRequest {
  const changes: UpdateRuleRequest = {};
  if (request.name !== rule.name) changes.name = request.name;
  if (!sameJson(request.params, rule.params)) changes.params = request.params;
  const was = rule.channels.map((channel) => channel.id).sort((a, b) => a - b);
  const now = [...request.channel_ids].sort((a, b) => a - b);
  if (!sameJson(was, now)) changes.channel_ids = request.channel_ids;
  return changes;
}

/**
 * The request of a form, or a `ConsoleRefusal` about the first field that
 * cannot be sent. A choice made now that is not offered is not sent: it
 * becomes "any", and a channel that is gone is left out.
 */
export function ruleRequestOf(
  form: RuleForm,
  offered: Offered,
  /** The rule that is changed: what of it the form did not touch is kept as it is. */
  original?: Rule,
): CreateRuleRequest {
  const was = original === undefined ? null : ruleFormOf(original);
  const untouched = (field: keyof RuleForm) =>
    was !== null && original?.kind === form.kind && Object.is(was[field], form[field]);
  return {
    name: form.name.trim(),
    kind: form.kind,
    params: paramsRequest(form, offered, untouched),
    channel_ids: form.channel_ids
      .filter((id) => offered.channels.some((one) => String(one) === id))
      .map(Number),
  };
}

/**
 * The gateway names a parameter that is not valid `params.<name>`; the form
 * has a field of that name. The error stays what the gateway answered, with
 * its status, its code and its message; only the names of the fields change.
 */
export function paramsOnFields(error: unknown): unknown {
  if (!(error instanceof ApiError)) return error;
  const fields = Object.fromEntries(
    Object.entries(error.fields).map(([name, text]) => [name.replace(/^params\./, ""), text]),
  );
  return new ApiError(error.status, error.code, error.message, fields, error.retryAfter);
}
