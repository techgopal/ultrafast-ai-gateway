import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import type { useUpdateKey } from "@/api/queries";
import type { components } from "@/api/schema";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { GuardrailPicker, KEY_HINT } from "@/components/GuardrailPicker";

type Key = components["schemas"]["KeyView"];

interface GuardrailsFormProps {
  /** The key as the list shows it; its guardrails are where the form starts. */
  keyOf: Pick<Key, "id" | "guardrails">;
  /** The mutation of the page that opens the dialog: the page resets it when the dialog closes. */
  update: ReturnType<typeof useUpdateKey>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with the guardrails as they are.
function GuardrailsForm({ keyOf, update, onDone, onCancel }: GuardrailsFormProps) {
  const { mutateAsync } = update;
  const start: { guardrail_ids: number[] } = {
    guardrail_ids: keyOf.guardrails.map((one) => one.id),
  };
  const form = useForm({
    defaultValues: start,
    onSubmit: async ({ value }) => {
      try {
        // All of them: what is sent replaces the guardrails of the key, and [] takes them off.
        await mutateAsync({ id: keyOf.id, body: { guardrail_ids: value.guardrail_ids } });
        onDone();
      } catch (error) {
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
      aria-label="Edit guardrails"
      noValidate
      className="flex min-w-0 flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="guardrail_ids">
        {(field) => (
          <Field
            group
            label="Guardrails"
            name={field.name}
            hint={KEY_HINT}
            error={failure.fieldError(field.name)}
          >
            {(wiring) => (
              <GuardrailPicker
                wiring={wiring}
                value={field.state.value}
                onChange={field.handleChange}
              />
            )}
          </Field>
        )}
      </form.Field>
      <FormDialogFooter
        running={update.isPending}
        submit="Save"
        submitting="Saving"
        onCancel={onCancel}
      />
    </form>
  );
}

/** Changes the guardrails of a key: what only an admin may. */
export function GuardrailsDialog({ open, ...form }: GuardrailsFormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.update.isPending}
      title="Edit guardrails"
      description="The key's guardrails check every call of the key, after the guardrails of the gateway and of the route."
      onCancel={form.onCancel}
    >
      <GuardrailsForm {...form} />
    </FormDialog>
  );
}
