import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import type { useUpdateUser } from "@/api/queries";
import { dialogButton, dialogFit, useReturnFocus } from "@/components/dialog-fit";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure } from "@/components/form";
import { FormError } from "@/components/FormError";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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

  return (
    <form
      ref={formRef}
      aria-label="Edit name"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        // One request at a time: a form that is being sent is not sent again.
        if (form.state.isSubmitting) return;
        void form.handleSubmit();
      }}
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
      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          className={dialogButton}
          disabled={update.isPending}
          onClick={onCancel}
        >
          Cancel
        </Button>
        <Button type="submit" className={dialogButton} disabled={update.isPending}>
          {update.isPending ? "Saving" : "Save"}
        </Button>
      </DialogFooter>
    </form>
  );
}

/**
 * Changes the name of a user: of any user on their page, by who may, and of
 * the user who is signed in on the account page. While the name is saved the
 * dialog stays: it cannot be closed, and the form cannot be sent a second time.
 */
export function NameDialog({ open, ...form }: NameFormProps & { open: boolean }) {
  const returnFocus = useReturnFocus(open);
  const { update, onCancel } = form;
  const running = update.isPending;
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !running) onCancel();
      }}
    >
      <DialogContent
        className={dialogFit}
        showCloseButton={!running}
        onCloseAutoFocus={returnFocus}
      >
        <DialogHeader>
          <DialogTitle>Edit name</DialogTitle>
          <DialogDescription>The name is shown in the console and in the audit log.</DialogDescription>
        </DialogHeader>
        <NameForm {...form} />
      </DialogContent>
    </Dialog>
  );
}
