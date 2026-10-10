import { useForm, useSelector } from "@tanstack/react-form";
import { useMemo, useRef, useState, type ReactNode } from "react";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import {
  applyApiError,
  onField,
  useFocusOnFailure,
  useFormFailure,
  useSubmit,
} from "@/components/form";
import { FilterSelect } from "@/components/FilterSelect";
import { FormError } from "@/components/FormError";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  check,
  hasProblems,
  MAX_MESSAGES,
  newKey,
  ROLES,
  variablesIn,
  type Draft,
  type DraftMessage,
  type Role,
} from "@/lib/prompts";

export const FIX_THE_FIELDS = "Some fields are not valid. They are marked below.";
export const NAME_HINT =
  "The id a call names. Case sensitive, up to 100 characters, and no @: the logs write name@version.";
export const MODEL_HINT =
  "Used when a call names no model: provider/model, or a route. Empty: a call must name one.";
export const NO_VARIABLES = "No variables yet. Write {{name}} where a value goes.";
export const FORMAT_KEPT = "Response format: kept from this version.";

const ROLE_CHOICES = ROLES.map((role) => ({ value: role, label: role }));

type Values = Omit<Draft, "responseFormat">;

/** What is wrong with the values; a version has no name or description of its own. */
function checkValues(mode: "create" | "version", value: Values) {
  return check(
    mode === "version"
      ? { ...value, name: "version", description: "", responseFormat: null }
      : { ...value, responseFormat: null },
  );
}

interface RowsProps {
  rows: readonly DraftMessage[];
  errors: Readonly<Record<number, string>>;
  onChange: (rows: DraftMessage[]) => void;
}

/** The messages of a version: a role and a text each, in the order a call gets them. */
function Rows({ rows, errors, onChange }: RowsProps) {
  const replace = (at: number, row: DraftMessage) => {
    onChange(rows.map((one, index) => (index === at ? row : one)));
  };
  const move = (at: number, by: -1 | 1) => {
    const next = [...rows];
    const moved = next[at];
    const other = next[at + by];
    if (moved === undefined || other === undefined) return;
    next[at] = other;
    next[at + by] = moved;
    onChange(next);
  };
  return (
    <div className="flex flex-col gap-3">
      <ol aria-label="Messages" className="flex flex-col gap-4">
        {rows.map((row, at) => {
          const n = String(at + 1);
          return (
            <li key={row.key} className="flex flex-col gap-2 rounded-md border p-3">
              <Field label={`Message ${n}`} name={`message-${String(row.key)}`} error={errors[row.key]}>
                <Textarea
                  className="min-h-24 font-mono text-xs"
                  spellCheck={false}
                  value={row.content}
                  onChange={(event) => {
                    replace(at, { ...row, content: event.target.value });
                  }}
                />
              </Field>
              <div className="flex flex-wrap items-center gap-2">
                <FilterSelect
                  label={`Role of message ${n}`}
                  value={row.role}
                  choices={ROLE_CHOICES}
                  onChange={(role) => {
                    replace(at, { ...row, role: ROLES.find((one) => one === role) ?? row.role });
                  }}
                />
                <Button
                  type="button"
                  variant="outline"
                  className={`${control} min-w-11 md:min-w-0`}
                  aria-label={`Move message ${n} up`}
                  disabled={at === 0}
                  onClick={() => {
                    move(at, -1);
                  }}
                >
                  Up
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  className={`${control} min-w-11 md:min-w-0`}
                  aria-label={`Move message ${n} down`}
                  disabled={at === rows.length - 1}
                  onClick={() => {
                    move(at, 1);
                  }}
                >
                  Down
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  className={control}
                  aria-label={`Remove message ${n}`}
                  disabled={rows.length === 1}
                  onClick={() => {
                    onChange(rows.filter((_, index) => index !== at));
                  }}
                >
                  Remove
                </Button>
              </div>
            </li>
          );
        })}
      </ol>
      <div>
        <Button
          type="button"
          variant="outline"
          className={control}
          disabled={rows.length >= MAX_MESSAGES}
          onClick={() => {
            const role: Role = rows.at(-1)?.role === "user" ? "assistant" : "user";
            onChange([...rows, { key: newKey(), role, content: "" }]);
          }}
        >
          Add message
        </Button>
      </div>
    </div>
  );
}

interface PromptFormProps {
  /** `create`: a name, a description and version 1. `version`: the next version of a template. */
  mode: "create" | "version";
  /** Where the form starts. It is read once: another start is another form (give it a `key`). */
  start: Draft;
  /** Names the form for assistive technology. */
  label: string;
  submitLabel: string;
  /** Sent when the draft is valid. A rejection puts the error on the form. */
  onSend: (draft: Draft) => Promise<void>;
  pending: boolean;
  /** After the buttons: a way out. */
  extra?: ReactNode;
}

/** The form of a new template and of a new version: the same messages and settings. */
export function PromptForm({ mode, start, label, submitLabel, onSend, pending, extra }: PromptFormProps) {
  const [attempted, setAttempted] = useState(false);
  const [first] = useState<Values>(() => ({
    name: start.name,
    description: start.description,
    model: start.model,
    temperature: start.temperature,
    maxTokens: start.maxTokens,
    topP: start.topP,
    messages: start.messages,
  }));
  const form = useForm({
    defaultValues: first,
    onSubmit: async ({ value }) => {
      if (hasProblems(checkValues(mode, value))) {
        setAttempted(true);
        failed();
        return;
      }
      try {
        await onSend({ ...value, responseFormat: start.responseFormat });
      } catch (error) {
        applyApiError(form, onField(error, "prompt_exists", "name"));
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const failed = useFocusOnFailure(formRef, errorRef);
  const onSubmit = useSubmit(form);
  const values = useSelector(form.store, (state) => state.values);
  const live = useMemo(() => (attempted ? checkValues(mode, values) : null), [attempted, mode, values]);
  const variables = useMemo(
    () => variablesIn(values.messages.map((message) => message.content)),
    [values.messages],
  );
  const errorOf = (name: keyof Values): string | undefined =>
    failure.fieldError(name) ?? (live?.fields as Record<string, string | undefined> | undefined)?.[name];
  const kept = start.responseFormat !== null && start.responseFormat !== undefined;

  return (
    <form
      ref={formRef}
      aria-label={label}
      noValidate
      className="flex max-w-2xl flex-col gap-6"
      onSubmit={onSubmit}
    >
      <FormError
        ref={errorRef}
        messages={
          failure.messages.length > 0 || live === null || !hasProblems(live)
            ? failure.messages
            : [FIX_THE_FIELDS]
        }
      />
      {mode === "create" ? (
        <>
          <form.Field name="name">
            {(field) => (
              <Field label="Name" name={field.name} required hint={NAME_HINT} error={errorOf("name")}>
                <Input
                  autoComplete="off"
                  className={control}
                  value={field.state.value}
                  onBlur={field.handleBlur}
                  onChange={(event) => {
                    field.handleChange(event.target.value);
                  }}
                />
              </Field>
            )}
          </form.Field>
          <form.Field name="description">
            {(field) => (
              <Field label="Description" name={field.name} error={errorOf("description")}>
                <Textarea
                  autoComplete="off"
                  className="min-h-16"
                  value={field.state.value}
                  onBlur={field.handleBlur}
                  onChange={(event) => {
                    field.handleChange(event.target.value);
                  }}
                />
              </Field>
            )}
          </form.Field>
        </>
      ) : null}
      <form.Field name="messages">
        {(field) => (
          <div className="flex flex-col gap-2">
            <p className="text-sm font-medium">Messages</p>
            {errorOf("messages") === undefined ? null : (
              <p role="alert" className="text-sm text-destructive">
                {errorOf("messages")}
              </p>
            )}
            <Rows rows={field.state.value} errors={live?.rows ?? {}} onChange={field.handleChange} />
            <p role="status" aria-label="Variables" className="text-sm text-muted-foreground">
              {variables.length === 0 ? NO_VARIABLES : `Variables: ${variables.join(", ")}`}
            </p>
          </div>
        )}
      </form.Field>
      <form.Field name="model">
        {(field) => (
          <Field label="Model" name={field.name} hint={MODEL_HINT} error={errorOf("model")}>
            <Input
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              className={control}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      {(
        [
          ["temperature", "Temperature", "From 0 to 2. Empty: the call's own, or the provider's default.", "decimal"],
          ["maxTokens", "Max tokens", "A whole number. Empty: the call's own, or the provider's default.", "numeric"],
          ["topP", "Top P", "From 0 to 1. Empty: the call's own, or the provider's default.", "decimal"],
        ] as const
      ).map(([name, title, hint, input]) => (
        <form.Field key={name} name={name}>
          {(field) => (
            <Field label={title} name={field.name} hint={hint} error={errorOf(name)}>
              <Input
                inputMode={input}
                autoComplete="off"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
          )}
        </form.Field>
      ))}
      {kept ? <p className="text-sm text-muted-foreground">{FORMAT_KEPT}</p> : null}
      <div className="flex flex-wrap gap-2">
        <Button type="submit" className={control} disabled={pending}>
          {pending ? "Saving" : submitLabel}
        </Button>
        {extra}
      </div>
    </form>
  );
}
