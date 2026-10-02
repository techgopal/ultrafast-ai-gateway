import { ArrowDown, ArrowUp } from "lucide-react";
import type { components } from "@/api/schema";
import { control, cutLongChoice, selectList } from "@/components/classes";
import type { FieldWiring } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { refOf } from "@/lib/models";
import { moved, type PrimaryRow, type RowProblem } from "@/lib/routes";

type Model = components["schemas"]["ModelView"];

const selectTrigger = `${control} w-full min-w-0 ${cutLongChoice}`;
const rowClass = "flex flex-wrap items-start gap-2 rounded-lg border bg-card p-2";

export const NO_MODELS = "There are no models to choose from. Enable a model first.";

function modelText(model: Model): string {
  return model.enabled ? refOf(model) : `${refOf(model)} (disabled)`;
}

interface ModelSelectProps {
  /** Names the select. */
  label: string;
  value: string;
  /** What can be chosen. */
  models: readonly Model[];
  /** The ids of the models that another row has: they are listed, and cannot be chosen again. */
  taken: readonly string[];
  invalid: boolean;
  describedBy: string | undefined;
  onChange: (value: string) => void;
}

function ModelSelect({
  label,
  value,
  models,
  taken,
  invalid,
  describedBy,
  onChange,
}: ModelSelectProps) {
  // A model that is no longer offered is not shown as chosen.
  const shown = models.some((model) => String(model.id) === value) ? value : "";
  return (
    <div className="min-w-0 flex-1 basis-48">
      <Select value={shown} onValueChange={onChange}>
        <SelectTrigger
          aria-label={label}
          aria-invalid={invalid ? true : undefined}
          aria-describedby={describedBy}
          className={selectTrigger}
        >
          <SelectValue placeholder="Choose a model" />
        </SelectTrigger>
        <SelectContent className={selectList}>
          {models.map((model) => (
            <SelectItem
              key={model.id}
              value={String(model.id)}
              disabled={taken.includes(String(model.id))}
            >
              {modelText(model)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

function RowProblems({ id, messages }: { id: string; messages: readonly (string | undefined)[] }) {
  const shown = messages.filter((message) => message !== undefined);
  if (shown.length === 0) return null;
  return (
    <p id={id} className="basis-full text-sm text-destructive">
      {shown.join(" ")}
    </p>
  );
}

interface PrimariesProps {
  wiring: FieldWiring;
  models: readonly Model[];
  rows: readonly PrimaryRow[];
  /** The models of the fallbacks: a primary cannot be one of them. */
  fallbacks: readonly string[];
  problems: readonly RowProblem[];
  onChange: (rows: PrimaryRow[]) => void;
}

export function Primaries({ wiring, models, rows, fallbacks, problems, onChange }: PrimariesProps) {
  const { id } = wiring;
  const change = (index: number, patch: Partial<PrimaryRow>) => {
    onChange(rows.map((row, at) => (at === index ? { ...row, ...patch } : row)));
  };
  return (
    <div
      role="group"
      id={id}
      aria-labelledby={wiring["aria-labelledby"]}
      aria-describedby={wiring["aria-describedby"]}
      className="flex flex-col gap-2"
    >
      {models.length === 0 ? <p className="text-sm text-muted-foreground">{NO_MODELS}</p> : null}
      <ul className="flex flex-col gap-2">
        {rows.map((row, index) => {
          const number = index + 1;
          const problem = problems[index] ?? {};
          const errorId = `${id}-row-${String(number)}-error`;
          const described = problem.model !== undefined || problem.weight !== undefined ? errorId : undefined;
          const taken = [...rows.filter((_, at) => at !== index).map((r) => r.model), ...fallbacks];
          return (
            // The place of a row is its identity: it has no id of its own.
            <li key={index} className={rowClass}>
              <ModelSelect
                label={`Model of primary target ${String(number)}`}
                value={row.model}
                models={models}
                taken={taken}
                invalid={problem.model !== undefined}
                describedBy={described}
                onChange={(model) => {
                  change(index, { model });
                }}
              />
              <Input
                aria-label={`Weight of primary target ${String(number)}`}
                aria-invalid={problem.weight !== undefined ? true : undefined}
                aria-describedby={described}
                inputMode="numeric"
                autoComplete="off"
                className={`${control} w-24`}
                value={row.weight}
                onChange={(event) => {
                  change(index, { weight: event.target.value });
                }}
              />
              <Button
                type="button"
                variant="outline"
                className={control}
                aria-label={`Remove primary target ${String(number)}`}
                onClick={() => {
                  onChange(rows.filter((_, at) => at !== index));
                }}
              >
                Remove
              </Button>
              <RowProblems id={errorId} messages={[problem.model, problem.weight]} />
            </li>
          );
        })}
      </ul>
      <Button
        type="button"
        variant="outline"
        className={`${control} w-fit`}
        onClick={() => {
          onChange([...rows, { model: "", weight: "1" }]);
        }}
      >
        Add primary target
      </Button>
    </div>
  );
}

interface FallbacksProps {
  wiring: FieldWiring;
  models: readonly Model[];
  rows: readonly string[];
  /** The models of the primaries: a fallback cannot be one of them. */
  primaries: readonly string[];
  problems: readonly (string | undefined)[];
  onChange: (rows: string[]) => void;
}

export function Fallbacks({ wiring, models, rows, primaries, problems, onChange }: FallbacksProps) {
  const { id } = wiring;
  return (
    <div
      role="group"
      id={id}
      aria-labelledby={wiring["aria-labelledby"]}
      aria-describedby={wiring["aria-describedby"]}
      className="flex flex-col gap-2"
    >
      <ol className="flex flex-col gap-2">
        {rows.map((row, index) => {
          const number = index + 1;
          const problem = problems[index];
          const errorId = `${id}-row-${String(number)}-error`;
          const taken = [...rows.filter((_, at) => at !== index), ...primaries];
          return (
            <li key={index} className={rowClass}>
              <ModelSelect
                label={`Model of fallback ${String(number)}`}
                value={row}
                models={models}
                taken={taken}
                invalid={problem !== undefined}
                describedBy={problem === undefined ? undefined : errorId}
                onChange={(model) => {
                  onChange(rows.map((one, at) => (at === index ? model : one)));
                }}
              />
              <Button
                type="button"
                variant="outline"
                className={control}
                aria-label={`Move fallback ${String(number)} up`}
                disabled={index === 0}
                onClick={() => {
                  onChange(moved(rows, index, -1));
                }}
              >
                <ArrowUp aria-hidden="true" />
              </Button>
              <Button
                type="button"
                variant="outline"
                className={control}
                aria-label={`Move fallback ${String(number)} down`}
                disabled={index === rows.length - 1}
                onClick={() => {
                  onChange(moved(rows, index, 1));
                }}
              >
                <ArrowDown aria-hidden="true" />
              </Button>
              <Button
                type="button"
                variant="outline"
                className={control}
                aria-label={`Remove fallback ${String(number)}`}
                onClick={() => {
                  onChange(rows.filter((_, at) => at !== index));
                }}
              >
                Remove
              </Button>
              <RowProblems id={errorId} messages={[problem]} />
            </li>
          );
        })}
      </ol>
      <Button
        type="button"
        variant="outline"
        className={`${control} w-fit`}
        onClick={() => {
          onChange([...rows, ""]);
        }}
      >
        Add fallback
      </Button>
    </div>
  );
}
