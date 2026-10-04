import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import type { useSetLimit } from "@/api/queries";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Input } from "@/components/ui/input";
import { COUNTS, emptyLimitForm, limitFormOf, limitRequestOf, offeredWith } from "@/lib/limits";
import { ScopeField, TargetField, useTargets } from "@/pages/LimitsTarget";

type Limit = components["schemas"]["LimitView"];

export const LIMIT_DESCRIPTION =
  "Leave a number empty for no limit on it. Saving replaces all three numbers of this target.";

interface FormProps {
  /** The limit that is changed; `null` for a new one. */
  row: Limit | null;
  put: ReturnType<typeof useSetLimit>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts from the limit as it is.
function LimitForm({ row, put, onDone, onCancel }: FormProps) {
  const { mutateAsync, reset } = put;
  const targets = useTargets();
  const form = useForm({
    defaultValues: row === null ? emptyLimitForm() : limitFormOf(row),
    onSubmit: async ({ value }) => {
      try {
        // The target of a limit that is changed is the one it has.
        const body = limitRequestOf(value, offeredWith(targets.offered, row));
        await mutateAsync(body);
        reset();
        onDone();
      } catch (error) {
        reset();
        applyApiError(form, error);
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const onSubmit = useSubmit(form);

  return (
    <form
      ref={formRef}
      aria-label={row === null ? "Set limit" : "Edit limit"}
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      {row === null ? (
        <>
          <form.Field name="scope">
            {(field) => (
              <ScopeField
                value={field.state.value}
                error={failure.fieldError(field.name)}
                onChange={(next) => {
                  field.handleChange(next);
                  // The target of the scope before is none of this one.
                  form.setFieldValue("scope_id", "");
                }}
              />
            )}
          </form.Field>
          <form.Subscribe selector={(state) => state.values.scope}>
            {(scope) => (
              <form.Field name="scope_id">
                {(field) => (
                  <TargetField
                    scope={scope}
                    value={field.state.value}
                    targets={targets}
                    error={failure.fieldError(field.name)}
                    onChange={field.handleChange}
                  />
                )}
              </form.Field>
            )}
          </form.Subscribe>
        </>
      ) : (
        <p className="flex flex-col gap-1 text-sm">
          <span className="text-muted-foreground">Applies to</span>
          <span className="break-all">{row.label}</span>
        </p>
      )}
      {COUNTS.map(([name, label]) => (
        <form.Field key={name} name={name}>
          {(field) => (
            <Field label={label} name={field.name} error={failure.fieldError(field.name)}>
              {({ id, name: fieldName, ...described }) => (
                <Input
                  {...described}
                  id={id}
                  name={fieldName}
                  inputMode="numeric"
                  autoComplete="off"
                  className={control}
                  value={field.state.value}
                  onBlur={field.handleBlur}
                  onChange={(event) => {
                    field.handleChange(event.target.value);
                  }}
                />
              )}
            </Field>
          )}
        </form.Field>
      ))}
      <FormDialogFooter
        running={put.isPending}
        submit="Save limit"
        submitting="Saving"
        onCancel={onCancel}
      />
    </form>
  );
}

export function LimitDialog({
  open,
  row,
  ...form
}: FormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.put.isPending}
      title={row === null ? "Set limit" : "Edit limit"}
      description={LIMIT_DESCRIPTION}
      onCancel={form.onCancel}
    >
      <LimitForm row={row} {...form} />
    </FormDialog>
  );
}
