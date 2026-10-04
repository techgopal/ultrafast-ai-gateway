import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import type { useUpdateKey } from "@/api/queries";
import type { components } from "@/api/schema";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { TagsEditor } from "@/components/TagsEditor";
import { rowsOf, tagsOf, type TagRow } from "@/lib/tags";

type Key = components["schemas"]["KeyView"];

interface TagsFormProps {
  /** The key as the list shows it; its tags are where the form starts. */
  keyOf: Pick<Key, "id" | "tags">;
  /** The mutation of the page that opens the dialog: the page resets it when the dialog closes. */
  update: ReturnType<typeof useUpdateKey>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with the tags as they are.
function TagsForm({ keyOf, update, onDone, onCancel }: TagsFormProps) {
  const { mutateAsync } = update;
  const start: { tags: readonly TagRow[] } = { tags: rowsOf(keyOf.tags) };
  const form = useForm({
    defaultValues: start,
    onSubmit: async ({ value }) => {
      try {
        // All of them: what is sent replaces the tags of the key.
        await mutateAsync({ id: keyOf.id, body: { tags: tagsOf(value.tags) } });
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
      aria-label="Edit tags"
      noValidate
      className="flex min-w-0 flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="tags">
        {(tags) => (
          <TagsEditor
            rows={tags.state.value}
            onChange={tags.handleChange}
            error={failure.fieldError(tags.name)}
          />
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

/** Changes the tags of a key: who may revoke it may. */
export function TagsDialog({ open, ...form }: TagsFormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.update.isPending}
      title="Edit tags"
      description="Tags are added to every call of the key. A call cannot change a tag the key has."
      onCancel={form.onCancel}
    >
      <TagsForm {...form} />
    </FormDialog>
  );
}
