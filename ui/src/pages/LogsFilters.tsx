import { useQuery } from "@tanstack/react-query";
import { useId, useState } from "react";
import { keysOptions, teamsOptions, usersOptions } from "@/api/queries";
import { control } from "@/components/classes";
import { FilterSelect, type Choice } from "@/components/FilterSelect";
import { Input } from "@/components/ui/input";
import { LOGGED_ACTIONS } from "@/lib/guardrails";
import { parseTagFilter } from "@/lib/tags";

/** The value of a select that leaves nothing out. No id is written so. */
export const ANY = "*";

export type Range = "1h" | "24h" | "7d" | "30d" | "custom";

export const RANGES: readonly Choice[] = [
  { value: "1h", label: "Last hour" },
  { value: "24h", label: "Last 24 hours" },
  { value: "7d", label: "Last 7 days" },
  { value: "30d", label: "Last 30 days" },
  { value: "custom", label: "Custom range" },
];

const STATUSES: readonly Choice[] = [
  { value: ANY, label: "All statuses" },
  { value: "errors", label: "Errors only" },
];

const GUARDRAILS: readonly Choice[] = [
  { value: ANY, label: "Any guardrail result" },
  ...LOGGED_ACTIONS.map(([value, label]) => ({ value, label })),
];

/** What the viewer chose. A choice is an id as text, or `ANY`. */
export interface Filters {
  range: Range;
  /** Dates of a custom range, `YYYY-MM-DD` or empty. */
  from: string;
  to: string;
  key: string;
  user: string;
  team: string;
  model: string;
  errorsOnly: boolean;
  /** What the guardrails did at worst, or `ANY`. */
  guardrail: string;
  /** `name:value` as it was applied, or empty. */
  tag: string;
}

export const NO_FILTERS: Filters = {
  range: "24h",
  from: "",
  to: "",
  key: ANY,
  user: ANY,
  team: ANY,
  model: "",
  errorsOnly: false,
  guardrail: ANY,
  tag: "",
};

/** What is chosen, when it is still offered; otherwise nothing is left out. */
export function chosen(value: string, choices: readonly Choice[]): string {
  return choices.some((choice) => choice.value === value) ? value : ANY;
}

interface Offered {
  keys: Choice[];
  users: Choice[];
  teams: Choice[];
}

/**
 * The keys, users and teams the viewer may choose among: what the API lists
 * to them. Asked only when the viewer sees more than themselves.
 */
export function useOffered(others: boolean): Offered {
  const keys = useQuery({ ...keysOptions(), enabled: others });
  const users = useQuery({ ...usersOptions(), enabled: others });
  const teams = useQuery({ ...teamsOptions(), enabled: others });
  return {
    keys: [
      { value: ANY, label: "All keys" },
      ...(keys.data?.keys ?? []).map((one) => ({ value: String(one.id), label: one.name })),
    ],
    users: [
      { value: ANY, label: "All users" },
      ...(users.data?.users ?? []).map((one) => ({ value: String(one.id), label: one.email })),
    ],
    teams: [
      { value: ANY, label: "All teams" },
      ...(teams.data?.teams ?? []).map((one) => ({ value: String(one.id), label: one.name })),
    ],
  };
}

interface LogsFiltersProps {
  filters: Filters;
  offered: Offered;
  /** Whether the viewer sees more than their own calls: key, user and team are offered. */
  others: boolean;
  onChange: (patch: Partial<Filters>) => void;
}

/** The model is applied when it is entered or left, not at every key. */
function ModelField({ value, onApply }: { value: string; onApply: (model: string) => void }) {
  const [draft, setDraft] = useState(value);
  const apply = () => {
    const model = draft.trim();
    if (model !== value) onApply(model);
  };
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        apply();
      }}
    >
      <Input
        type="text"
        aria-label="Model"
        placeholder="Model name"
        autoComplete="off"
        className={`${control} w-full sm:w-48`}
        value={draft}
        onChange={(event) => {
          setDraft(event.target.value);
        }}
        onBlur={apply}
      />
    </form>
  );
}

/**
 * The tag is `name:value`, applied on Enter. A text that is not one is
 * refused here, and nothing is asked of the gateway for it.
 */
function TagField({ value, onApply }: { value: string; onApply: (tag: string) => void }) {
  const [draft, setDraft] = useState(value);
  const [problem, setProblem] = useState<string | null>(null);
  const errorId = useId();
  return (
    <form
      className="flex flex-col gap-1"
      onSubmit={(event) => {
        event.preventDefault();
        if (draft.trim() === "") {
          setProblem(null);
          if (value !== "") onApply("");
          return;
        }
        const parsed = parseTagFilter(draft);
        if ("problem" in parsed) {
          setProblem(parsed.problem);
          return;
        }
        setProblem(null);
        if (parsed.tag !== value) onApply(parsed.tag);
      }}
    >
      <Input
        type="text"
        aria-label="Tag"
        placeholder="Tag name:value"
        autoComplete="off"
        className={`${control} w-full sm:w-48`}
        value={draft}
        {...(problem === null ? {} : { "aria-invalid": true, "aria-describedby": errorId })}
        onChange={(event) => {
          setDraft(event.target.value);
          setProblem(null);
        }}
      />
      {problem === null ? null : (
        <p id={errorId} role="alert" className="text-sm text-destructive">
          {problem}
        </p>
      )}
    </form>
  );
}

export function LogsFilters({ filters, offered, others, onChange }: LogsFiltersProps) {
  return (
    <div role="group" aria-label="Filters" className="flex flex-wrap items-center gap-2">
      <FilterSelect
        label="Time range"
        value={filters.range}
        choices={RANGES}
        onChange={(range) => {
          onChange({ range: range as Range });
        }}
      />
      {filters.range === "custom" ? (
        <>
          <Input
            type="date"
            aria-label="From (UTC)"
            className={`${control} w-full sm:w-40`}
            value={filters.from}
            onChange={(event) => {
              onChange({ from: event.target.value });
            }}
          />
          <Input
            type="date"
            aria-label="To (UTC)"
            className={`${control} w-full sm:w-40`}
            value={filters.to}
            onChange={(event) => {
              onChange({ to: event.target.value });
            }}
          />
        </>
      ) : null}
      {others ? (
        <>
          <FilterSelect
            label="Key"
            value={chosen(filters.key, offered.keys)}
            choices={offered.keys}
            onChange={(key) => {
              onChange({ key });
            }}
          />
          <FilterSelect
            label="User"
            value={chosen(filters.user, offered.users)}
            choices={offered.users}
            onChange={(user) => {
              onChange({ user });
            }}
          />
          <FilterSelect
            label="Team"
            value={chosen(filters.team, offered.teams)}
            choices={offered.teams}
            onChange={(team) => {
              onChange({ team });
            }}
          />
        </>
      ) : null}
      <ModelField
        value={filters.model}
        onApply={(model) => {
          onChange({ model });
        }}
      />
      <TagField
        value={filters.tag}
        onApply={(tag) => {
          onChange({ tag });
        }}
      />
      <FilterSelect
        label="Status"
        value={filters.errorsOnly ? "errors" : ANY}
        choices={STATUSES}
        onChange={(status) => {
          onChange({ errorsOnly: status === "errors" });
        }}
      />
      <FilterSelect
        label="Guardrails"
        value={filters.guardrail}
        choices={GUARDRAILS}
        onChange={(guardrail) => {
          onChange({ guardrail });
        }}
      />
    </div>
  );
}
