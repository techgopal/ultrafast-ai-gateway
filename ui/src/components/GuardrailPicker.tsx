import { ArrowDown, ArrowUp, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
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
import { moved, ordinal } from "@/lib/guardrails";

export const NO_GUARDRAILS = "There are no guardrails yet. Add one on the Guardrails page.";
export const NONE_CHOSEN = "None chosen.";
export const ORDER_HINT =
  "They run in this order, after the guardrails that apply to every call. Built-in rules run before external ones.";
/** What a guardrail on a key does not cover: a person can make another key. */
export const KEY_HINT = `${ORDER_HINT} These check only the calls made with this key: a new key, or a direct call to a model, is not checked by them.`;
/** What a guardrail on a team covers: every key of the team, also one made later. */
export const TEAM_HINT = `${ORDER_HINT} These check the calls of every key of this team, also a key made later. A key's own guardrails run after them.`;
/** What a guardrail on a user covers: every key the user owns. */
export const USER_HINT = `${ORDER_HINT} These check the calls of every key this user owns, also a key made later. A key's own guardrails run after them.`;
/** What a guardrail on a route does not cover: a model can be called directly. */
export const ROUTE_HINT = `${ORDER_HINT} These check only the calls that go through this route: a key that may also call a model directly is not checked by them.`;

/** The control that is to take the focus once the list has changed. */
type Focus = { guardrail: number; control: "up" | "down" | "remove" } | "add" | "group";

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
  const group = useRef<HTMLDivElement>(null);
  const focusAfter = useRef<Focus | null>(null);
  const [said, setSaid] = useState("");
  // A button that moves or goes is gone or off after the change: the focus
  // goes to the control that is the person's next step, not to the page.
  useEffect(() => {
    const want = focusAfter.current;
    focusAfter.current = null;
    const root = group.current;
    if (want === null || root === null) return;
    const selector =
      want === "group"
        ? null
        : want === "add"
          ? '[data-control="add"]'
          : `[data-guardrail="${String(want.guardrail)}"][data-control="${want.control}"]`;
    const target = selector === null ? root : root.querySelector<HTMLElement>(selector);
    (target ?? root).focus();
  }, [value]);
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
  // What applies to every call is not offered: attaching it changes nothing.
  const rest = all.filter((one) => !ids.includes(one.id) && !one.is_default);
  const { id } = wiring;
  const move = (index: number, by: -1 | 1, name: string) => {
    const to = index + by;
    // At an end the button that moved it is off: the focus goes to the other.
    const control = to === 0 ? "down" : to === ids.length - 1 ? "up" : by === -1 ? "up" : "down";
    const first = ids[index];
    if (first !== undefined) focusAfter.current = { guardrail: first, control };
    setSaid(`${name} is now ${ordinal(to + 1)} of ${String(ids.length)}.`);
    onChange(moved(ids, index, by));
  };
  const remove = (index: number, name: string) => {
    const after = ids.filter((_, at) => at !== index);
    const next = after[index] ?? after[index - 1];
    focusAfter.current =
      next !== undefined
        ? { guardrail: next, control: "remove" }
        : all.some((one) => !after.includes(one.id) && !one.is_default)
          ? "add"
          : "group";
    setSaid(`Removed ${name}.`);
    onChange(after);
  };
  return (
    <div
      ref={group}
      tabIndex={-1}
      role="group"
      id={id}
      aria-labelledby={wiring["aria-labelledby"]}
      aria-describedby={wiring["aria-describedby"]}
      className="flex flex-col gap-2 outline-none"
    >
      <div role="status" aria-live="polite" className="sr-only">
        {said}
      </div>
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
                {one.is_default ? <Badge variant="secondary">Every call</Badge> : null}
              </span>
              <Button
                type="button"
                variant="outline"
                size="icon"
                className={control}
                aria-label={`Move ${one.name} up`}
                data-guardrail={one.id}
                data-control="up"
                disabled={index === 0}
                onClick={() => {
                  move(index, -1, one.name);
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
                data-guardrail={one.id}
                data-control="down"
                disabled={index === chosen.length - 1}
                onClick={() => {
                  move(index, 1, one.name);
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
                data-guardrail={one.id}
                data-control="remove"
                onClick={() => {
                  remove(index, one.name);
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
            if (ids.includes(added)) return;
            const name = all.find((one) => one.id === added)?.name ?? "";
            setSaid(`Added ${name} as ${ordinal(ids.length + 1)} of ${String(ids.length + 1)}.`);
            onChange([...ids, added]);
          }}
        >
          <SelectTrigger
            aria-label="Add a guardrail"
            data-control="add"
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
