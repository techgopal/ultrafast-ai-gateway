import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { GuardrailPicker } from "@/components/GuardrailPicker";

/** The toast after the guardrails of a team or a user are saved. */
export const HOLDER_SAVED = "Guardrails saved.";

interface HolderFormProps {
  /** What the guardrails check, said under the title. */
  description: string;
  /** What the picker says about the order and the reach. */
  hint: string;
  /** The ids the form starts with. */
  start: readonly number[];
  /** Sends the whole list; the page owns the mutation and resets it when the dialog closes. */
  send: (ids: number[]) => Promise<unknown>;
  pending: boolean;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with the guardrails as they are.
function HolderForm({ hint, start, send, pending, onDone, onCancel }: HolderFormProps) {
  const form = useForm({
    defaultValues: { guardrail_ids: [...start] },
    onSubmit: async ({ value }) => {
      try {
        // All of them: what is sent replaces the guardrails, and [] takes them off.
        await send(value.guardrail_ids);
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
            hint={hint}
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
      <FormDialogFooter running={pending} submit="Save" submitting="Saving" onCancel={onCancel} />
    </form>
  );
}

/** Changes the guardrails of a team or a user: what only an admin may. */
export function GuardrailsHolderDialog({ open, ...form }: HolderFormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.pending}
      title="Edit guardrails"
      description={form.description}
      onCancel={form.onCancel}
    >
      <HolderForm {...form} />
    </FormDialog>
  );
}
