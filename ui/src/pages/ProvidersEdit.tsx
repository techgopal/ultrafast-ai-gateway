import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import type { useUpdateProvider } from "@/api/queries";
import { ConsoleRefusal } from "@/api/errors";
import type { components } from "@/api/schema";
import { ApiKeyInput } from "@/components/ApiKeyInput";
import { ApiVersionField } from "@/components/ApiVersionField";
import { BaseUrlField } from "@/components/BaseUrlField";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { DEFAULT_API_VERSION, HOST_CHANGED, kindName, newApiKeyOf, sameHost } from "@/lib/providers";

type Provider = components["schemas"]["ProviderView"];
type UpdateProviderRequest = components["schemas"]["UpdateProviderRequest"];

/** What is done with the credential: the three things the API can be told. */
const WITH_A_KEY = [
  ["keep", "Keep the current key"],
  ["replace", "Replace the key"],
  ["remove", "Remove the key"],
] as const;

/** There is no key to keep, to replace or to remove. */
const WITHOUT_A_KEY = [
  ["keep", "Leave without a key"],
  ["replace", "Set a key"],
] as const;

interface EditFormProps {
  provider: Provider;
  update: ReturnType<typeof useUpdateProvider>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with the provider as
// it is. The credential it has is not in the form: the API never returns it.
function EditForm({ provider, update, onDone, onCancel }: EditFormProps) {
  const { mutateAsync, reset } = update;
  // Only an Azure OpenAI provider has an API version; the gateway may show none for one that has the default.
  const azure = provider.kind === "azure";
  const version = provider.api_version ?? DEFAULT_API_VERSION;
  const form = useForm({
    defaultValues: {
      base_url: provider.base_url,
      api_version: version,
      credential: "keep",
      api_key: "",
    },
    onSubmit: async ({ value }) => {
      // A version cleared is no change: the gateway has no empty version.
      const typed = value.api_version.trim();
      const versionChanged = azure && typed !== "" && typed !== version;
      // Nothing was changed: nothing is sent, and nothing is reported as updated.
      if (value.credential === "keep" && value.base_url === provider.base_url && !versionChanged) {
        onCancel();
        return;
      }
      try {
        // The gateway sends a stored key only to the host it was given for.
        if (
          value.credential === "keep" &&
          provider.has_credential &&
          !sameHost(provider.base_url, value.base_url)
        ) {
          throw new ConsoleRefusal(HOST_CHANGED, "credential");
        }
        // Keeping the key sends no `api_key` at all.
        const body: UpdateProviderRequest = { base_url: value.base_url };
        if (versionChanged) body.api_version = typed;
        if (value.credential === "replace") body.api_key = newApiKeyOf(value.api_key);
        if (value.credential === "remove") body.api_key = null;
        await mutateAsync({ id: provider.id, body });
        // The provider has the key now: the form holds it no longer.
        form.setFieldValue("api_key", "");
        reset();
        onDone();
      } catch (error) {
        // The mutation does not keep what it sent. The field does: see `ApiKeyInput`.
        reset();
        applyApiError(form, error);
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const onSubmit = useSubmit(form);
  const choices = provider.has_credential ? WITH_A_KEY : WITHOUT_A_KEY;

  return (
    <form
      ref={formRef}
      aria-label="Edit provider"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="base_url">
        {(field) => (
          <BaseUrlField
            name={field.name}
            kind={provider.kind}
            value={field.state.value}
            onChange={field.handleChange}
            onBlur={field.handleBlur}
            error={failure.fieldError(field.name)}
          />
        )}
      </form.Field>
      {azure ? (
        <form.Field name="api_version">
          {(field) => (
            <ApiVersionField
              name={field.name}
              value={field.state.value}
              onChange={field.handleChange}
              onBlur={field.handleBlur}
              error={failure.fieldError(field.name)}
            />
          )}
        </form.Field>
      ) : null}
      <form.Field name="credential">
        {(field) => (
          <>
            <Field
              group
              label="API key"
              name={field.name}
              error={failure.fieldError(field.name)}
            >
              {({ id, name, ...described }) => (
                <RadioGroup
                  {...described}
                  id={id}
                  name={name}
                  value={field.state.value}
                  onValueChange={(next) => {
                    field.handleChange(next);
                    // A key that was typed is not kept behind another choice.
                    form.setFieldValue("api_key", "");
                  }}
                >
                  {choices.map(([value, label]) => (
                    <Label key={value} htmlFor={`${id}-${value}`} className={control}>
                      <RadioGroupItem id={`${id}-${value}`} value={value} />
                      {label}
                    </Label>
                  ))}
                </RadioGroup>
              )}
            </Field>
            {field.state.value === "replace" ? (
              <form.Field name="api_key">
                {(key) => (
                  <Field
                    label="New API key"
                    name={key.name}
                    required
                    hint="The gateway stores it encrypted and never shows it."
                    error={failure.fieldError(key.name)}
                  >
                    {(wiring) => (
                      <ApiKeyInput
                        wiring={wiring}
                        value={key.state.value}
                        onBlur={key.handleBlur}
                        onChange={key.handleChange}
                      />
                    )}
                  </Field>
                )}
              </form.Field>
            ) : null}
          </>
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

interface EditDialogProps extends Omit<EditFormProps, "provider"> {
  open: boolean;
  /** The provider the dialog is about. Kept while the dialog closes. */
  provider: Provider | null;
}

export function EditDialog({ open, provider, ...form }: EditDialogProps) {
  return (
    <FormDialog
      open={open && provider !== null}
      running={form.update.isPending}
      title="Edit provider"
      description={
        provider === null
          ? null
          : `${provider.name} (${kindName(provider.kind)}). The name and the kind cannot be changed.`
      }
      onCancel={form.onCancel}
    >
      {provider === null ? null : <EditForm provider={provider} {...form} />}
    </FormDialog>
  );
}
