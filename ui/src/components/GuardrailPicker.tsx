import { ArrowDown, ArrowUp, X } from "lucide-react";
import { useGuardrails } from "@/api/queries";
import { control, cutLongChoice, selectList } from "@/components/classes";
import { ErrorState } from "@/components/ErrorState";
import type { FieldWiring } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { moved } from "@/lib/guardrails";

export const NO_GUARDRAILS = "There are no guardrails yet. Add one on the Guardrails page.";
export const NONE_CHOSEN = "None chosen.";
export const ORDER_HINT =
  "They run in this order, after the guardrails that apply to every call. Built-in rules run before external ones.";

interface PickerProps {
  wiring: FieldWiring;
  /** The ids of the chosen guardrails, in the order they run. */
  value: readonly number[];
  onChange: (ids: number[]) => void;
}

/**
 * Chooses guardrails and puts the chosen ones in order: the chosen are a list
 * with buttons to move and remove each, and a select adds another at the end.
 * An id that no guardrail has any more is not shown, and is dropped on the
 * next change.
 */
export function GuardrailPicker({ wiring, value, onChange }: PickerProps) {
  const list = useGuardrails();
  if (list.data === undefined) {
    if (list.error !== null) {
      return (
        <ErrorState
          error={list.error}
          onRetry={() => {
            void list.refetch();
          }}
        />
      );
    }
    return (
      <div role="status" aria-busy="true" aria-label="Loading the guardrails" className="flex flex-col gap-2">
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-full" />
      </div>
    );
  }
  const all = list.data.guardrails;
  if (all.length === 0) return <p className="text-sm text-muted-foreground">{NO_GUARDRAILS}</p>;
  // What is chosen and still exists, in the order it runs.
  const chosen = value.flatMap((id) => all.filter((one) => one.id === id));
  const ids = chosen.map((one) => one.id);
  const rest = all.filter((one) => !ids.includes(one.id));
  const { id } = wiring;
  return (
    <div
      role="group"
      id={id}
      aria-labelledby={wiring["aria-labelledby"]}
      aria-describedby={wiring["aria-describedby"]}
      className="flex flex-col gap-2"
    >
      {chosen.length === 0 ? (
        <p className="text-sm text-muted-foreground">{NONE_CHOSEN}</p>
      ) : (
        <ol aria-label="Chosen guardrails" className="flex flex-col gap-1">
          {chosen.map((one, index) => (
            <li key={one.id} className="flex flex-wrap items-center gap-1">
              <span className="flex min-w-0 flex-1 basis-40 items-center gap-2">
                <span className="text-muted-foreground tabular-nums">{`${String(index + 1)}.`}</span>
                <span className="min-w-0 break-all">{one.name}</span>
                {one.enabled ? null : <Badge variant="outline">Disabled</Badge>}
              </span>
              <Button
                type="button"
                variant="outline"
                size="icon"
                className={control}
                aria-label={`Move ${one.name} up`}
                disabled={index === 0}
                onClick={() => {
                  onChange(moved(ids, index, -1));
                }}
              >
                <ArrowUp aria-hidden="true" />
              </Button>
              <Button
                type="button"
                variant="outline"
                size="icon"
                className={control}
                aria-label={`Move ${one.name} down`}
                disabled={index === chosen.length - 1}
                onClick={() => {
                  onChange(moved(ids, index, 1));
                }}
              >
                <ArrowDown aria-hidden="true" />
              </Button>
              <Button
                type="button"
                variant="outline"
                size="icon"
                className={control}
                aria-label={`Remove ${one.name}`}
                onClick={() => {
                  onChange(ids.filter((other) => other !== one.id));
                }}
              >
                <X aria-hidden="true" />
              </Button>
            </li>
          ))}
        </ol>
      )}
      {rest.length === 0 ? null : (
        // Always empty: choosing adds to the list, and the select is ready for the next.
        <Select
          value=""
          onValueChange={(next) => {
            const added = Number(next);
            if (!ids.includes(added)) onChange([...ids, added]);
          }}
        >
          <SelectTrigger
            aria-label="Add a guardrail"
            className={`${control} w-full sm:w-72 ${cutLongChoice}`}
          >
            <SelectValue placeholder="Add a guardrail" />
          </SelectTrigger>
          <SelectContent className={selectList}>
            {rest.map((one) => (
              <SelectItem key={one.id} value={String(one.id)}>
                {one.enabled ? one.name : `${one.name} (disabled)`}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </div>
  );
}
