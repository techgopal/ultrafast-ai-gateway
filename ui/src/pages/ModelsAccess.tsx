import { useForm } from "@tanstack/react-form";
import { useMemo, useRef, type ReactNode } from "react";
import { useTeams, useUsers, type usePutModelGrants } from "@/api/queries";
import type { components } from "@/api/schema";
import { ErrorState } from "@/components/ErrorState";
import { Field, type FieldWiring } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { grantsOf } from "@/lib/models";

type Model = components["schemas"]["ModelView"];

/** A long list scrolls inside the dialog. */
const scrolling = "max-h-48 overflow-y-auto";

interface ChecksProps<T extends { id: number }> {
  wiring: FieldWiring;
  /** `null` while the list is on its way. */
  items: readonly T[] | null;
  error: unknown;
  retry: () => void;
  loading: string;
  none: string;
  checked: readonly string[];
  onChange: (ids: string[]) => void;
  label: (item: T) => ReactNode;
}

/** A list of checkboxes, one for each thing that can be granted. */
function Checks<T extends { id: number }>({
  wiring,
  items,
  error,
  retry,
  loading,
  none,
  checked,
  onChange,
  label,
}: ChecksProps<T>) {
  if (items === null) {
    if (error !== null) return <ErrorState error={error} onRetry={retry} />;
    return (
      <div role="status" aria-busy="true" aria-label={loading} className="flex flex-col gap-2">
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-full" />
      </div>
    );
  }
  if (items.length === 0) return <p className="text-sm text-muted-foreground">{none}</p>;
  const { id } = wiring;
  return (
    <div
      role="group"
      id={id}
      aria-labelledby={wiring["aria-labelledby"]}
      aria-describedby={wiring["aria-describedby"]}
      className={`flex flex-col ${scrolling}`}
    >
      {items.map((item) => {
        const value = String(item.id);
        const inputId = `${id}-${value}`;
        return (
          <Label key={item.id} htmlFor={inputId} className="min-h-11 items-start py-2">
            <Checkbox
              id={inputId}
              checked={checked.includes(value)}
              onCheckedChange={(on) => {
                onChange(on === true ? [...checked, value] : checked.filter((one) => one !== value));
              }}
            />
            <span className="flex min-w-0 flex-wrap gap-x-2">{label(item)}</span>
          </Label>
        );
      })}
    </div>
  );
}

interface FormProps {
  model: Model;
  put: ReturnType<typeof usePutModelGrants>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts from the grants as they are.
function AccessForm({ model, put, onDone, onCancel }: FormProps) {
  const { mutateAsync } = put;
  const teams = useTeams();
  const users = useUsers();
  const { grants } = model;
  const teamList = useMemo(() => teams.data?.teams ?? null, [teams.data]);
  // Who can be chosen: every user who is not disabled, and who has the grant already.
  const userList = useMemo(
    () =>
      users.data === undefined
        ? null
        : users.data.users.filter(
            (user) => user.status !== "disabled" || grants.user_ids.includes(user.id),
          ),
    [users.data, grants.user_ids],
  );
  const form = useForm({
    defaultValues: {
      everyone: grants.everyone,
      team_ids: grants.team_ids.map(String),
      user_ids: grants.user_ids.map(String),
    },
    onSubmit: async ({ value }) => {
      try {
        const body = grantsOf(
          value,
          (teamList ?? []).map((team) => team.id),
          (userList ?? []).map((user) => user.id),
        );
        await mutateAsync({ id: model.id, body });
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
  const waiting = teamList === null || userList === null;

  return (
    <form
      ref={formRef}
      aria-label="Edit access"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="everyone">
        {(field) => (
          <Field label="Everyone" name={field.name} error={failure.fieldError(field.name)}>
            {({ id, name, ...described }) => (
              <Switch
                {...described}
                id={id}
                name={name}
                className="relative after:absolute after:-inset-x-2 after:-inset-y-3"
                checked={field.state.value}
                onCheckedChange={field.handleChange}
              />
            )}
          </Field>
        )}
      </form.Field>
      <form.Subscribe selector={(state) => state.values.everyone}>
        {(everyone) =>
          everyone ? null : (
            <>
              <form.Field name="team_ids">
                {(field) => (
                  <Field group label="Teams" name={field.name} error={failure.fieldError(field.name)}>
                    {(wiring) => (
                      <Checks
                        wiring={wiring}
                        items={teamList}
                        error={teams.error}
                        retry={() => {
                          void teams.refetch();
                        }}
                        loading="Loading the teams"
                        none="There are no teams yet."
                        checked={field.state.value}
                        onChange={field.handleChange}
                        label={(team) => <span>{team.name}</span>}
                      />
                    )}
                  </Field>
                )}
              </form.Field>
              <form.Field name="user_ids">
                {(field) => (
                  <Field group label="Users" name={field.name} error={failure.fieldError(field.name)}>
                    {(wiring) => (
                      <Checks
                        wiring={wiring}
                        items={userList}
                        error={users.error}
                        retry={() => {
                          void users.refetch();
                        }}
                        loading="Loading the users"
                        none="There are no users to choose from."
                        checked={field.state.value}
                        onChange={field.handleChange}
                        label={(user) => (
                          <>
                            <span>{user.name}</span>
                            <span className="font-normal break-all text-muted-foreground">
                              {user.email}
                            </span>
                          </>
                        )}
                      />
                    )}
                  </Field>
                )}
              </form.Field>
            </>
          )
        }
      </form.Subscribe>
      <form.Subscribe selector={(state) => state.values.everyone}>
        {(everyone) => (
          <FormDialogFooter
            running={put.isPending}
            submit="Save access"
            submitting="Saving"
            disabled={!everyone && waiting}
            onCancel={onCancel}
          />
        )}
      </form.Subscribe>
    </form>
  );
}

export function AccessDialog({
  open,
  model,
  ...form
}: Omit<FormProps, "model"> & { open: boolean; model: Model | null }) {
  return (
    <FormDialog
      open={open}
      running={form.put.isPending}
      title="Edit access"
      description="Who may call this model while it is enabled."
      onCancel={form.onCancel}
    >
      {model === null ? null : <AccessForm model={model} {...form} />}
    </FormDialog>
  );
}
