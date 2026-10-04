import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import type { useUpdateModel } from "@/api/queries";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Input } from "@/components/ui/input";
import { priceFormOf, pricesRequestOf } from "@/lib/models";

type Model = components["schemas"]["ModelView"];

export const PRICE_DESCRIPTION =
  "Dollars per 1M tokens. Leave a price empty when it is not known: calls of the model are then logged without a cost.";

interface FormProps {
  model: Model;
  update: ReturnType<typeof useUpdateModel>;
  onDone: () => void;
  onCancel: () => void;
}

const FIELDS = [
  ["input_price_micros", "Input price ($ per 1M tokens)"],
  ["output_price_micros", "Output price ($ per 1M tokens)"],
] as const;

// Mounted while the dialog is open: every opening starts from the prices as they are.
function PriceForm({ model, update, onDone, onCancel }: FormProps) {
  const { mutateAsync, reset } = update;
  const form = useForm({
    defaultValues: priceFormOf(model),
    onSubmit: async ({ value }) => {
      try {
        const body = pricesRequestOf(value);
        await mutateAsync({ id: model.id, body });
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
      aria-label="Edit price"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      {FIELDS.map(([name, label]) => (
        <form.Field key={name} name={name}>
          {(field) => (
            <Field label={label} name={field.name} error={failure.fieldError(field.name)}>
              {({ id, name: fieldName, ...described }) => (
                <Input
                  {...described}
                  id={id}
                  name={fieldName}
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
      ))}
      <FormDialogFooter
        running={update.isPending}
        submit="Save price"
        submitting="Saving"
        onCancel={onCancel}
      />
    </form>
  );
}

export function PriceDialog({
  open,
  model,
  ...form
}: Omit<FormProps, "model"> & { open: boolean; model: Model | null }) {
  return (
    <FormDialog
      open={open}
      running={form.update.isPending}
      title="Edit price"
      description={PRICE_DESCRIPTION}
      onCancel={form.onCancel}
    >
      {model === null ? null : <PriceForm model={model} {...form} />}
    </FormDialog>
  );
}
