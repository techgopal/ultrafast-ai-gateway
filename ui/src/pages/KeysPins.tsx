import { useForm } from "@tanstack/react-form";
import { useMemo, useRef } from "react";
import type { useUpdateKey } from "@/api/queries";
import type { components } from "@/api/schema";
import { Checks } from "@/components/CheckList";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { useCallable } from "@/pages/KeysAllowed";

type Key = components["schemas"]["KeyView"];

export const PINS_HINT =
  "When a key can call more tools than the gateway lists at once, the combined endpoint lists the search tools and these. The key finds the others with find_tools.";

interface PinsFormProps {
  /** The key as the list shows it; its pinned tools are where the form starts. */
  keyOf: Pick<Key, "id" | "pinned_tools" | "team_id" | "team_only">;
  /** The mutation of the page that opens the dialog: the page resets it when the dialog closes. */
  update: ReturnType<typeof useUpdateKey>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with the pins as they are.
function PinsForm({ keyOf, update, onDone, onCancel }: PinsFormProps) {
  const { mutateAsync } = update;
  const callable = useCallable(true, keyOf.team_only ? keyOf.team_id : null);
  const start: { pinned_tools: string[] } = { pinned_tools: [...keyOf.pinned_tools] };
  const form = useForm({
    defaultValues: start,
    onSubmit: async ({ value }) => {
      try {
        // All of them: what is sent replaces the pinned tools, and [] unpins them.
        await mutateAsync({ id: keyOf.id, body: { pinned_tools: value.pinned_tools } });
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
  // What can be pinned: the tools the person can call, and what is pinned already
  // (so that it can be taken off even if it is not offered any more).
  const items = useMemo(() => {
    const offered = callable.mcp.map((one) => one.id);
    const extra = keyOf.pinned_tools.filter((one) => !offered.includes(one));
    return [...callable.mcp, ...extra.map((id) => ({ id, route: false }))];
  }, [callable.mcp, keyOf.pinned_tools]);

  return (
    <form
      ref={formRef}
      aria-label="Edit pinned tools"
      noValidate
      className="flex min-w-0 flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="pinned_tools">
        {(field) => (
          <Field
            group
            label="Pinned tools"
            name={field.name}
            hint={PINS_HINT}
            error={failure.fieldError(field.name)}
          >
            {(wiring) => (
              <Checks
                wiring={wiring}
                items={items}
                error={null}
                retry={() => undefined}
                loading="Loading the MCP tools"
                none="There is no MCP tool to pin yet."
                checked={field.state.value}
                onChange={field.handleChange}
                label={(item) => <span className="font-mono break-all">{item.id}</span>}
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

/** Changes the tools pinned to a key: an admin, or a lead of the key's team. */
export function PinsDialog({ open, ...form }: PinsFormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.update.isPending}
      title="Edit pinned tools"
      description="Tools the combined MCP endpoint always lists for this key."
      onCancel={form.onCancel}
    >
      <PinsForm {...form} />
    </FormDialog>
  );
}
