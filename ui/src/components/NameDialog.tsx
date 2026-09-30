import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import type { useUpdateUser } from "@/api/queries";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Input } from "@/components/ui/input";

interface NameFormProps {
  /** Whose name it is. */
  user: { id: number; name: string };
  /** The mutation of the page that opens the dialog: the page resets it when the dialog closes. */
  update: ReturnType<typeof useUpdateUser>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with the name as it is.
function NameForm({ user, update, onDone, onCancel }: NameFormProps) {
  const { mutateAsync } = update;
  const form = useForm({
    defaultValues: { name: user.name },
    onSubmit: async ({ value }) => {
      try {
        await mutateAsync({ id: user.id, body: { name: value.name } });
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
      aria-label="Edit name"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="name">
        {(field) => (
          <Field label="Name" name={field.name} required error={failure.fieldError(field.name)}>
            <Input
              autoComplete="off"
              className="min-h-11 md:min-h-8"
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
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

/**
 * Changes the name of a user: of any user on their page, by who may, and of
 * the user who is signed in on the account page.
 */
export function NameDialog({ open, ...form }: NameFormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.update.isPending}
      title="Edit name"
      description="The name is shown in the console and in the audit log."
      onCancel={form.onCancel}
    >
      <NameForm {...form} />
    </FormDialog>
  );
}
