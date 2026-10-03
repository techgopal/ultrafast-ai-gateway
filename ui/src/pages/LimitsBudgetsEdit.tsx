import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import type { useSetBudget } from "@/api/queries";
import type { components } from "@/api/schema";
import { control, cutLongChoice } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  ACTIONS,
  budgetFormOf,
  budgetRequestOf,
  emptyBudgetForm,
  inFormWords,
  offeredWith,
  PERIODS,
} from "@/lib/limits";
import { ScopeField, TargetField, useTargets } from "@/pages/LimitsTarget";

type Budget = components["schemas"]["BudgetView"];

export const BUDGET_DESCRIPTION =
  "A cap on spend in a calendar period (UTC). A budget that blocks refuses calls once the amount is spent.";
export const ACTION_HINT =
  "Blocks refuses calls once the amount is spent. Alerts allows them and writes one audit entry per period.";

interface FormProps {
  /** The budget that is changed; `null` for a new one. */
  row: Budget | null;
  put: ReturnType<typeof useSetBudget>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts from the budget as it is.
function BudgetForm({ row, put, onDone, onCancel }: FormProps) {
  const { mutateAsync, reset } = put;
  const targets = useTargets();
  const form = useForm({
    defaultValues: row === null ? emptyBudgetForm() : budgetFormOf(row),
    onSubmit: async ({ value }) => {
      try {
        const body = budgetRequestOf(value, offeredWith(targets.offered, row));
        await mutateAsync(body);
        reset();
        onDone();
      } catch (error) {
        reset();
        applyApiError(form, inFormWords(error));
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
      aria-label={row === null ? "Set budget" : "Edit budget"}
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
      <form.Field name="amount">
        {(field) => (
          <Field label="Amount (USD)" name={field.name} error={failure.fieldError(field.name)}>
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                inputMode="decimal"
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
      <form.Field name="period">
        {(field) => (
          <Field label="Period" name={field.name} error={failure.fieldError(field.name)}>
            {({ id, name, ...described }) => (
              <Select name={name} value={field.state.value} onValueChange={field.handleChange}>
                <SelectTrigger id={id} {...described} className={`${control} w-full ${cutLongChoice}`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PERIODS.map(([value, label]) => (
                    <SelectItem key={value} value={value}>
                      {label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </Field>
        )}
      </form.Field>
      <form.Field name="action">
        {(field) => (
          <Field
            group
            label="Action"
            name={field.name}
            hint={ACTION_HINT}
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <RadioGroup
                {...described}
                id={id}
                name={name}
                value={field.state.value}
                onValueChange={field.handleChange}
              >
                {ACTIONS.map(([value, label]) => (
                  <Label key={value} htmlFor={`${id}-${value}`} className={control}>
                    <RadioGroupItem id={`${id}-${value}`} value={value} />
                    {label}
                  </Label>
                ))}
              </RadioGroup>
            )}
          </Field>
        )}
      </form.Field>
      <FormDialogFooter
        running={put.isPending}
        submit="Save budget"
        submitting="Saving"
        onCancel={onCancel}
      />
    </form>
  );
}

export function BudgetDialog({
  open,
  row,
  ...form
}: FormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.put.isPending}
      title={row === null ? "Set budget" : "Edit budget"}
      description={BUDGET_DESCRIPTION}
      onCancel={form.onCancel}
    >
      <BudgetForm row={row} {...form} />
    </FormDialog>
  );
}
