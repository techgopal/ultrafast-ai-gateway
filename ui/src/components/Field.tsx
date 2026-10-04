import { Slot } from "radix-ui";
import { useId, type ReactElement, type ReactNode } from "react";
import { Label } from "@/components/ui/label";

/** What ties a control to its label, its hint and its error. */
export interface FieldWiring {
  id: string;
  name: string;
  required?: true;
  "aria-invalid"?: true;
  "aria-describedby"?: string;
  /** For a group of controls only: the id of the text that names the group. */
  "aria-labelledby"?: string;
}

interface FieldProps {
  label: string;
  /** The name of the field in the form, which is the name the API has for it. */
  name: string;
  /** What is wrong with the value. */
  error?: string | undefined;
  /** Shown under the field at all times. */
  hint?: string | undefined;
  required?: boolean | undefined;
  /**
   * The field is a group of controls, such as a radio group, each with a
   * label of its own. A `<label>` names one control and cannot name a group:
   * the label of the field is then a text, and the wiring has its id as
   * `aria-labelledby`, which goes on the group and on nothing else.
   */
  group?: boolean;
  /**
   * The control: an element, which is given the wiring as its props, or a
   * function that puts the wiring on the control itself.
   */
  children: ReactElement | ((wiring: FieldWiring) => ReactNode);
}

/**
 * A labelled control. The label is tied to it with `htmlFor`, the error and
 * the hint with `aria-describedby`, and an error sets `aria-invalid`. The
 * error is announced when it appears. A group of controls is named by the
 * label with `aria-labelledby` instead: see `group`.
 */
export function Field({ label, name, error, hint, required, group, children }: FieldProps) {
  const id = useId();
  const labelId = `${id}-label`;
  const errorId = `${id}-error`;
  const hintId = `${id}-hint`;
  const describedBy = [error === undefined ? null : errorId, hint === undefined ? null : hintId]
    .filter((part) => part !== null)
    .join(" ");
  const wiring: FieldWiring = {
    id,
    name,
    ...(required === true ? { required: true } : {}),
    ...(error === undefined ? {} : { "aria-invalid": true }),
    ...(describedBy === "" ? {} : { "aria-describedby": describedBy }),
    ...(group === true ? { "aria-labelledby": labelId } : {}),
  };
  return (
    <div className="flex flex-col gap-2">
      <div className="flex gap-1">
        {group === true ? (
          <Label asChild>
            <span id={labelId}>{label}</span>
          </Label>
        ) : (
          <Label htmlFor={id}>{label}</Label>
        )}
        {required === true ? (
          <span aria-hidden="true" className="text-sm leading-none text-destructive">
            *
          </span>
        ) : null}
      </div>
      {typeof children === "function" ? (
        children(wiring)
      ) : (
        <Slot.Root {...wiring}>{children}</Slot.Root>
      )}
      {error === undefined ? null : (
        <p id={errorId} role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {hint === undefined ? null : (
        <p id={hintId} className="text-sm text-muted-foreground">
          {hint}
        </p>
      )}
    </div>
  );
}
