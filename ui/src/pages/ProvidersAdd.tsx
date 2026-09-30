import { useForm } from "@tanstack/react-form";
import { useId, useRef } from "react";
import type { useCreateProvider } from "@/api/queries";
import type { components } from "@/api/schema";
import { ApiKeyInput } from "@/components/ApiKeyInput";
import { BaseUrlField } from "@/components/BaseUrlField";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, onField, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { apiKeyOf, kindName, KINDS } from "@/lib/providers";

type CreateProviderRequest = components["schemas"]["CreateProviderRequest"];

export const NAME_HINT = "lowercase letters, digits, - and _";

/**
 * Base URLs of providers that many use. Choosing one fills the field of the
 * form with this text. The console asks none of them for anything: it is the
 * gateway that calls a provider.
 */
const KNOWN = [
  { name: "OpenAI", kind: "openai", base_url: "https://api.openai.com/v1" },
  { name: "Anthropic", kind: "anthropic", base_url: "https://api.anthropic.com" },
  { name: "Groq", kind: "openai", base_url: "https://api.groq.com/openai/v1" },
  { name: "Mistral", kind: "openai", base_url: "https://api.mistral.ai/v1" },
  { name: "OpenRouter", kind: "openai", base_url: "https://openrouter.ai/api/v1" },
  { name: "Ollama", kind: "openai", base_url: "http://localhost:11434/v1" },
] as const;

interface AddFormProps {
  create: ReturnType<typeof useCreateProvider>;
  /** The provider was added under this name. */
  onAdded: (name: string) => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with an empty form.
function AddForm({ create, onAdded, onCancel }: AddFormProps) {
  const { mutateAsync, reset } = create;
  const knownId = useId();
  const form = useForm({
    defaultValues: { name: "", kind: "openai", base_url: "", api_key: "" },
    onSubmit: async ({ value }) => {
      try {
        const body: CreateProviderRequest = {
          name: value.name,
          kind: value.kind,
          base_url: value.base_url,
        };
        // No key is no field: an empty one the gateway refuses.
        const key = apiKeyOf(value.api_key);
        if (key !== "") body.api_key = key;
        const made = await mutateAsync(body);
        // The provider has the key now: the form holds it no longer.
        form.setFieldValue("api_key", "");
        reset();
        onAdded(made.name);
      } catch (error) {
        // The mutation does not keep what it sent. The field does: see `ApiKeyInput`.
        reset();
        applyApiError(form, onField(error, "provider_exists", "name"));
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
      aria-label="Add provider"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="name">
        {(field) => (
          <Field
            label="Name"
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
      <div className="flex flex-col gap-2">
        <span id={knownId} className="text-sm leading-none font-medium">
          Known providers
        </span>
        <div role="group" aria-labelledby={knownId} className="flex flex-wrap gap-2">
          {KNOWN.map((known) => (
            <Button
              key={known.name}
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                form.setFieldValue("base_url", known.base_url);
                form.setFieldValue("kind", known.kind);
              }}
            >
              {known.name}
            </Button>
          ))}
        </div>
        <p className="text-sm text-muted-foreground">Choosing one fills the base URL and the kind.</p>
      </div>
      <form.Field name="kind">
        {(field) => (
          <Field group label="Kind" name={field.name} error={failure.fieldError(field.name)}>
            {({ id, name, ...described }) => (
              <RadioGroup
                {...described}
                id={id}
                name={name}
                value={field.state.value}
                onValueChange={field.handleChange}
              >
                {KINDS.map((kind) => (
                  <div key={kind} className="flex min-h-11 items-center gap-2 md:min-h-8">
                    <RadioGroupItem id={`${id}-${kind}`} value={kind} />
                    <Label htmlFor={`${id}-${kind}`}>{kindName(kind)}</Label>
                  </div>
                ))}
              </RadioGroup>
            )}
          </Field>
        )}
      </form.Field>
      <form.Subscribe selector={(state) => state.values.kind}>
        {(kind) => (
          <form.Field name="base_url">
            {(field) => (
              <BaseUrlField
                name={field.name}
                kind={kind}
                value={field.state.value}
                onChange={field.handleChange}
                onBlur={field.handleBlur}
                error={failure.fieldError(field.name)}
              />
            )}
          </form.Field>
        )}
      </form.Subscribe>
      <form.Field name="api_key">
        {(field) => (
          <Field
            label="API key"
            name={field.name}
            hint="Optional. The gateway stores it encrypted and never shows it."
            error={failure.fieldError(field.name)}
          >
            {(wiring) => (
              <ApiKeyInput
                wiring={wiring}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={field.handleChange}
              />
            )}
          </Field>
        )}
      </form.Field>
      <FormDialogFooter
        running={create.isPending}
        submit="Add provider"
        submitting="Adding the provider"
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
      title="Add provider"
      description="The gateway sends the calls for the models of a provider to its base URL."
      onCancel={form.onCancel}
    >
      <AddForm {...form} />
    </FormDialog>
  );
}
