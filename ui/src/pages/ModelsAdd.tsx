import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import { ConsoleRefusal } from "@/api/errors";
import { useProviders, type useCreateModel, type useSyncProvider } from "@/api/queries";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { ErrorState } from "@/components/ErrorState";
import { Field, type FieldWiring } from "@/components/Field";
import { applyApiError, onField, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Skeleton } from "@/components/ui/skeleton";

type Provider = components["schemas"]["ProviderView"];
type SyncResult = components["schemas"]["SyncResult"];

export const NAME_HINT = "The provider's model ID, for example gpt-4o-mini.";
export const CHOOSE_A_PROVIDER = "Choose a provider.";
export const ENTER_A_NAME = "Enter the model's name.";

/** What the console says about the one field of the form. It is no answer of the gateway. */
function refuse(message: string, field: string): ConsoleRefusal {
  return new ConsoleRefusal(message, field);
}

/**
 * The providers to choose one from, read when the dialog opens: while they
 * are on their way, a skeleton; when they could not be read, the error with
 * Retry; and the choice when they are there.
 */
function ProviderChoice({
  wiring,
  value,
  onChange,
}: {
  wiring: FieldWiring;
  value: string;
  onChange: (value: string) => void;
}) {
  const providers = useProviders();
  if (providers.data === undefined) {
    if (providers.error !== null) {
      return (
        <ErrorState
          error={providers.error}
          onRetry={() => {
            void providers.refetch();
          }}
        />
      );
    }
    return (
      <div
        role="status"
        aria-busy="true"
        aria-label="Loading the providers"
        className="flex flex-col gap-2"
      >
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-full" />
      </div>
    );
  }
  if (providers.data.providers.length === 0) {
    return <p className="text-sm text-muted-foreground">No provider has been added yet.</p>;
  }
  const { id, name, ...described } = wiring;
  return (
    <RadioGroup {...described} id={id} name={name} value={value} onValueChange={onChange}>
      {providers.data.providers.map((provider) => (
        <Label key={provider.id} htmlFor={`${id}-${String(provider.id)}`} className={control}>
          <RadioGroupItem id={`${id}-${String(provider.id)}`} value={String(provider.id)} />
          <span className="min-w-0 font-mono break-all">{provider.name}</span>
        </Label>
      ))}
    </RadioGroup>
  );
}

/** The provider that was chosen, when it is one of those offered; otherwise nothing. */
function providerChosen(value: string, offered: readonly Provider[] | undefined): number {
  const found = offered?.find((provider) => String(provider.id) === value);
  if (found === undefined) throw refuse(CHOOSE_A_PROVIDER, "provider_id");
  return found.id;
}

// ------------------------------------------------------------------- add

interface AddFormProps {
  create: ReturnType<typeof useCreateModel>;
  onAdded: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with an empty form.
function AddForm({ create, onAdded, onCancel }: AddFormProps) {
  const { mutateAsync, reset } = create;
  const providers = useProviders();
  const form = useForm({
    defaultValues: { provider_id: "", name: "" },
    onSubmit: async ({ value }) => {
      try {
        const providerId = providerChosen(value.provider_id, providers.data?.providers);
        const name = value.name.trim();
        if (name === "") throw refuse(ENTER_A_NAME, "name");
        await mutateAsync({ provider_id: providerId, name });
        reset();
        onAdded();
      } catch (error) {
        reset();
        applyApiError(form, onField(error, "model_exists", "name"));
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
      aria-label="Add model"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="provider_id">
        {(field) => (
          <Field group label="Provider" name={field.name} error={failure.fieldError(field.name)}>
            {(wiring) => (
              <ProviderChoice
                wiring={wiring}
                value={field.state.value}
                onChange={field.handleChange}
              />
            )}
          </Field>
        )}
      </form.Field>
      <form.Field name="name">
        {(field) => (
          <Field
            label="Model name"
            name={field.name}
            required
            hint={NAME_HINT}
            error={failure.fieldError(field.name)}
          >
            <Input
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              className={control}
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
        running={create.isPending}
        submit="Add model"
        submitting="Adding the model"
        onCancel={onCancel}
      />
    </form>
  );
}

export function AddDialog({ open, ...form }: AddFormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.create.isPending}
      title="Add model"
      description="The model starts disabled, and nobody has access to it."
      onCancel={form.onCancel}
    >
      <AddForm {...form} />
    </FormDialog>
  );
}

// ------------------------------------------------------------------ sync

interface SyncFormProps {
  sync: ReturnType<typeof useSyncProvider>;
  onSynced: (result: SyncResult) => void;
  onCancel: () => void;
}

function SyncForm({ sync, onSynced, onCancel }: SyncFormProps) {
  const { mutateAsync, reset } = sync;
  const providers = useProviders();
  const form = useForm({
    defaultValues: { provider_id: "" },
    onSubmit: async ({ value }) => {
      try {
        const id = providerChosen(value.provider_id, providers.data?.providers);
        const result = await mutateAsync({ id });
        reset();
        onSynced(result);
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
      aria-label="Sync models"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="provider_id">
        {(field) => (
          <Field group label="Provider" name={field.name} error={failure.fieldError(field.name)}>
            {(wiring) => (
              <ProviderChoice
                wiring={wiring}
                value={field.state.value}
                onChange={field.handleChange}
              />
            )}
          </Field>
        )}
      </form.Field>
      <FormDialogFooter
        running={sync.isPending}
        submit="Sync"
        submitting="Syncing"
        onCancel={onCancel}
      />
    </form>
  );
}

export function SyncDialog({ open, ...form }: SyncFormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.sync.isPending}
      title="Sync models"
      description="The gateway reads the list of models of the provider and adds the new ones. Nothing is removed."
      onCancel={form.onCancel}
    >
      <SyncForm {...form} />
    </FormDialog>
  );
}
