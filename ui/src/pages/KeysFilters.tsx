import type { components } from "@/api/schema";
import { control, cutLongChoice, selectList } from "@/components/classes";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { NO_TEAM, WITHOUT_TEAM } from "@/lib/keys";

type Key = components["schemas"]["KeyView"];

/** A select among the filters: no wider than the page. */
const filterTrigger = `${control} max-w-full ${cutLongChoice}`;

/** The value of a filter that leaves nothing out. No id and no status is written so. */
export const ANY = "*";

/** The statuses the gateway knows, in the order they are offered. */
const STATUSES = ["active", "suspended", "expired", "revoked"];

interface Filtering {
  search: string;
  team: string;
  status: string;
  showRevoked: boolean;
}

/** The text is looked for in the name, the owner and what is shown of the key. */
export function matches(key: Key, { search, team, status, showRevoked }: Filtering): boolean {
  if (!showRevoked && key.status === "revoked") return false;
  if (status !== ANY && key.status !== status) return false;
  if (team === WITHOUT_TEAM ? key.team_id !== null : team !== ANY && String(key.team_id) !== team) {
    return false;
  }
  const text = search.trim().toLowerCase();
  if (text === "") return true;
  return [key.name, key.owner_email ?? "", key.display].some((value) =>
    value.toLowerCase().includes(text),
  );
}

interface Choice {
  value: string;
  label: string;
}

/** The teams the keys belong to, by name, and "No team" when a key has none. */
export function teamChoices(keys: readonly Key[]): Choice[] {
  const names = new Map<number, string>();
  for (const key of keys) {
    if (key.team_id !== null) names.set(key.team_id, key.team_name ?? String(key.team_id));
  }
  const teams = [...names]
    .map(([id, label]) => ({ value: String(id), label }))
    .sort((a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: "base" }));
  const none = keys.some((key) => key.team_id === null)
    ? [{ value: WITHOUT_TEAM, label: NO_TEAM }]
    : [];
  return [{ value: ANY, label: "All teams" }, ...teams, ...none];
}

/**
 * The statuses of the gateway, and after them what else the keys have: a
 * status the console does not know can be chosen as it is. "revoked" is a
 * choice only while revoked keys are shown.
 */
export function statusChoices(keys: readonly Key[], showRevoked: boolean): Choice[] {
  const others = [...new Set(keys.map((key) => key.status))]
    .filter((status) => !STATUSES.includes(status))
    .sort();
  const statuses = [...STATUSES, ...others].filter(
    (status) => showRevoked || status !== "revoked",
  );
  return [
    { value: ANY, label: "All statuses" },
    ...statuses.map((status) => ({ value: status, label: status })),
  ];
}

/** What is chosen, when it is still a choice; otherwise nothing is left out. */
export function chosen(value: string, choices: readonly Choice[]): string {
  return choices.some((choice) => choice.value === value) ? value : ANY;
}

interface FilterSelectProps {
  label: string;
  value: string;
  choices: readonly Choice[];
  onChange: (value: string) => void;
}

export function FilterSelect({ label, value, choices, onChange }: FilterSelectProps) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger aria-label={label} className={filterTrigger}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent className={selectList}>
        {choices.map((choice) => (
          <SelectItem key={choice.value} value={choice.value}>
            {choice.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
