// What the console knows of the catalog of models: how it is filtered and
// sorted, how access is said in words, and what is sent as grants.
import { ConsoleRefusal } from "@/api/errors";
import type { components } from "@/api/schema";
import { dollarsToMicros, formatDollars, microsToDollars } from "@/lib/money";

type Model = components["schemas"]["ModelView"];
type Grants = components["schemas"]["GrantsView"];
type SyncResult = components["schemas"]["SyncResult"];

/** The value of a filter that leaves nothing out. No id and no status is written so. */
export const ANY = "*";

export const STATUSES = ["enabled", "disabled"] as const;

function count(number: number, one: string, many: string): string {
  return `${String(number)} ${number === 1 ? one : many}`;
}

/** Who may call the model, in a few words: "Everyone", "No one", "2 teams, 1 user". */
export function accessSummary(grants: Grants): string {
  if (grants.everyone) return "Everyone";
  const parts = [
    grants.team_ids.length > 0 ? count(grants.team_ids.length, "team", "teams") : null,
    grants.user_ids.length > 0 ? count(grants.user_ids.length, "user", "users") : null,
  ].filter((part) => part !== null);
  return parts.length === 0 ? "No one" : parts.join(", ");
}

/** Nobody but an admin can call a model that is granted to no one. */
export function hasNoAccess(grants: Grants): boolean {
  return !grants.everyone && grants.team_ids.length === 0 && grants.user_ids.length === 0;
}

/** How a call names the model. */
export function refOf(model: Pick<Model, "provider_name" | "name">): string {
  return `${model.provider_name}/${model.name}`;
}

function byText(a: string, b: string): number {
  return a.localeCompare(b, undefined, { sensitivity: "base", numeric: true });
}

/** By provider, then by name. The list is not changed. */
export function sortModels(models: readonly Model[]): Model[] {
  return [...models].sort(
    (a, b) => byText(a.provider_name, b.provider_name) || byText(a.name, b.name),
  );
}

interface Filtering {
  search: string;
  provider: string;
  status: string;
}

/** The text is looked for in the name and in the provider. */
export function matches(model: Model, { search, provider, status }: Filtering): boolean {
  if (provider !== ANY && String(model.provider_id) !== provider) return false;
  if (status !== ANY && (model.enabled ? "enabled" : "disabled") !== status) return false;
  const text = search.trim().toLowerCase();
  if (text === "") return true;
  return [model.name, model.provider_name].some((value) => value.toLowerCase().includes(text));
}

export interface Choice {
  value: string;
  label: string;
}

/** The providers the models belong to, by name. */
export function providerChoices(models: readonly Model[]): Choice[] {
  const names = new Map<number, string>();
  for (const model of models) names.set(model.provider_id, model.provider_name);
  const providers = [...names]
    .map(([id, label]) => ({ value: String(id), label }))
    .sort((a, b) => byText(a.label, b.label));
  return [{ value: ANY, label: "All providers" }, ...providers];
}

export function statusChoices(): Choice[] {
  return [
    { value: ANY, label: "All statuses" },
    { value: "enabled", label: "Enabled" },
    { value: "disabled", label: "Disabled" },
  ];
}

/** What is chosen, when it is still a choice; otherwise nothing is left out. */
export function chosen(value: string, choices: readonly Choice[]): string {
  return choices.some((choice) => choice.value === value) ? value : ANY;
}

/** What the page says of a sync. */
export function syncText(result: SyncResult): string {
  const added = result.added.length;
  if (added === 0) return "No new models.";
  return added === 1
    ? "Added 1 model. It starts disabled."
    : `Added ${String(added)} models. They start disabled.`;
}

export interface GrantChoice {
  everyone: boolean;
  team_ids: readonly string[];
  user_ids: readonly string[];
}

/**
 * The grants to send. A team or a user that is no longer offered is not sent
 * (the gateway would refuse the id), and with everyone on there is none.
 */
export function grantsOf(
  choice: GrantChoice,
  offeredTeams: readonly number[],
  offeredUsers: readonly number[],
): Grants {
  if (choice.everyone) return { everyone: true, team_ids: [], user_ids: [] };
  const only = (ids: readonly string[], offered: readonly number[]) =>
    ids.map(Number).filter((id) => offered.includes(id));
  return {
    everyone: false,
    team_ids: only(choice.team_ids, offeredTeams),
    user_ids: only(choice.user_ids, offeredUsers),
  };
}

export const PRICE_RULE = "Enter dollars, 0 or more, with up to 6 decimals.";
export const NO_PRICE = "Not set";

/** A price for the table: dollars per 1M tokens, or that it is not set. */
export function priceText(micros: number | null): string {
  return micros === null ? NO_PRICE : formatDollars(micros);
}

/** What the price dialog holds: dollars as typed; empty is unknown. */
export interface PriceForm {
  input_price_micros: string;
  output_price_micros: string;
}

export function priceFormOf(model: Pick<Model, "input_price_micros" | "output_price_micros">): PriceForm {
  const text = (micros: number | null) => (micros === null ? "" : microsToDollars(micros));
  return {
    input_price_micros: text(model.input_price_micros),
    output_price_micros: text(model.output_price_micros),
  };
}

/**
 * The request of the form: both prices, in micros; an empty one is `null`,
 * which makes the price unknown. A `ConsoleRefusal` about the first field
 * that is no price.
 */
export function pricesRequestOf(form: PriceForm): {
  input_price_micros: number | null;
  output_price_micros: number | null;
} {
  const one = (name: keyof PriceForm): number | null => {
    const typed = form[name].trim();
    if (typed === "") return null;
    const micros = dollarsToMicros(typed);
    if (micros === null) throw new ConsoleRefusal(PRICE_RULE, name);
    return micros;
  };
  return { input_price_micros: one("input_price_micros"), output_price_micros: one("output_price_micros") };
}
