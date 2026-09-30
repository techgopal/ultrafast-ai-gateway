import { useForm } from "@tanstack/react-form";
import { useId, useRef, useState } from "react";
import { useCreateProvider, useDeleteProvider, useProviders, useUpdateProvider } from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { ApiKeyInput } from "@/components/ApiKeyInput";
import { BaseUrlField } from "@/components/BaseUrlField";
import { control, longText } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { applyApiError, onField, submitOnce, useFormFailure } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { useToast } from "@/components/toast";
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";

type Provider = components["schemas"]["ProviderView"];
type CreateProviderRequest = components["schemas"]["CreateProviderRequest"];
type UpdateProviderRequest = components["schemas"]["UpdateProviderRequest"];

export const NAME_HINT = "lowercase letters, digits, - and _";
export const DELETE_CONSEQUENCE = "Calls to models of this provider will fail at once.";

export const DONE = {
  update: "Provider updated.",
  delete: "Provider deleted.",
} as const;

const KIND_NAMES: Record<string, string> = { openai: "OpenAI-compatible", anthropic: "Anthropic" };
const KINDS = ["openai", "anthropic"] as const;

function kindName(kind: string): string {
  return KIND_NAMES[kind] ?? kind;
}

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
      const body: CreateProviderRequest = {
        name: value.name,
        kind: value.kind,
        base_url: value.base_url,
      };
      // No key is no field: an empty one the gateway refuses.
      if (value.api_key.trim() !== "") body.api_key = value.api_key;
      try {
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

  return (
    <form
      ref={formRef}
      aria-label="Add provider"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={submitOnce(form)}
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

function AddDialog({ open, ...form }: AddFormProps & { open: boolean }) {
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
  const form = useForm({
    defaultValues: { base_url: provider.base_url, credential: "keep", api_key: "" },
    onSubmit: async ({ value }) => {
      // Nothing was changed: nothing is sent, and nothing is reported as updated.
      if (value.credential === "keep" && value.base_url === provider.base_url) {
        onCancel();
        return;
      }
      // Keeping the key sends no `api_key` at all.
      const body: UpdateProviderRequest = { base_url: value.base_url };
      if (value.credential === "replace") body.api_key = value.api_key;
      if (value.credential === "remove") body.api_key = null;
      try {
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
  const choices = provider.has_credential ? WITH_A_KEY : WITHOUT_A_KEY;

  return (
    <form
      ref={formRef}
      aria-label="Edit provider"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={submitOnce(form)}
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
                    <div key={value} className="flex min-h-11 items-center gap-2 md:min-h-8">
                      <RadioGroupItem id={`${id}-${value}`} value={value} />
                      <Label htmlFor={`${id}-${value}`}>{label}</Label>
                    </div>
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

function EditDialog({ open, provider, ...form }: EditDialogProps) {
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

const columns: Column<Provider>[] = [
  {
    id: "name",
    header: "Name",
    cell: (provider) => (
      <span className={`${longText} font-mono font-medium break-all md:max-w-64`}>
        {provider.name}
      </span>
    ),
    sortValue: (provider) => provider.name,
  },
  {
    id: "kind",
    header: "Kind",
    // A kind the console does not know is shown as it is.
    cell: (provider) => <Badge variant="outline">{kindName(provider.kind)}</Badge>,
    sortValue: (provider) => provider.kind,
  },
  {
    id: "base_url",
    header: "Base URL",
    cell: (provider) => (
      <span className={`${longText} break-all md:max-w-80`}>{provider.base_url}</span>
    ),
    sortValue: (provider) => provider.base_url,
  },
  {
    id: "has_credential",
    header: "Credential",
    // Whether there is one, in words. The credential itself the API never gives.
    cell: (provider) =>
      provider.has_credential ? (
        <Badge variant="secondary">Set</Badge>
      ) : (
        <Badge variant="outline">None</Badge>
      ),
    sortValue: (provider) => (provider.has_credential ? 1 : 0),
  },
];

type Asking = "add" | "edit" | "delete";

export function Providers() {
  const session = useSession();
  const providers = useProviders();
  const create = useCreateProvider();
  const update = useUpdateProvider();
  const remove = useDeleteProvider();
  const toast = useToast();
  const [asking, setAsking] = useState<Asking | null>(null);
  // Which provider the dialog is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Provider | null>(null);
  // The name of the provider that was added, for the notice that says how to call it.
  const [added, setAdded] = useState<string | null>(null);

  if (session.status !== "signedIn") return null;
  const mayManage = can(session.me, { type: "manageProviders" });

  function closing(reset: () => void) {
    return (open: boolean) => {
      if (open) return;
      setAsking(null);
      reset();
    };
  }

  function askAbout(what: "edit" | "delete", provider: Provider) {
    return () => {
      setTarget(provider);
      setAsking(what);
    };
  }

  const addButton = mayManage ? (
    <Button
      type="button"
      className={control}
      onClick={() => {
        setAsking("add");
      }}
    >
      Add provider
    </Button>
  ) : undefined;

  const rowActions = mayManage
    ? (provider: Provider) => (
        <>
          <Button
            type="button"
            variant="outline"
            className={control}
            onClick={askAbout("edit", provider)}
          >
            Edit
          </Button>
          <Button
            type="button"
            variant="outline"
            className={control}
            onClick={askAbout("delete", provider)}
          >
            Delete
          </Button>
        </>
      )
    : undefined;

  const failed = providers.error !== null && providers.data === undefined;
  return (
    <>
      {/* One child of the page: the place of the notice takes no room while it is empty. */}
      <div className="flex flex-col">
        <PageHeader title="Providers" actions={failed ? undefined : addButton} />
        {/*
          Where the page says how a provider that was added is called. The
          place is there before the text: what comes into a live region is
          announced, a live region that comes with its text often is not.
        */}
        <div role="status" className="not-empty:mt-6">
          {added === null ? null : (
            // The announcement is that of the place: the alert is none of its own.
            <Alert role="presentation">
              <AlertTitle>Provider added</AlertTitle>
              <AlertDescription>
                <p>
                  Call its models as{" "}
                  <code className="font-mono break-words">{`${added}/<model>`}</code>: the name of
                  the provider, a slash, and the name of the model.
                </p>
              </AlertDescription>
              <AlertAction>
                <Button
                  type="button"
                  variant="outline"
                  className={control}
                  onClick={() => {
                    setAdded(null);
                  }}
                >
                  Dismiss
                </Button>
              </AlertAction>
            </Alert>
          )}
        </div>
      </div>
      {failed ? (
        <QueryProblem
          error={providers.error}
          onRetry={() => {
            void providers.refetch();
          }}
        />
      ) : (
        <DataTable
          caption="Providers"
          columns={columns}
          rows={providers.data?.providers ?? []}
          loading={providers.isPending}
          getRowId={(provider) => String(provider.id)}
          empty={<EmptyState title="No providers" description="No provider has been added yet." />}
          {...(rowActions === undefined ? {} : { actions: rowActions })}
        />
      )}
      {mayManage ? (
        <>
          <AddDialog
            open={asking === "add"}
            create={create}
            onCancel={() => {
              closing(create.reset)(false);
            }}
            onAdded={(name) => {
              closing(create.reset)(false);
              setAdded(name);
            }}
          />
          <EditDialog
            open={asking === "edit"}
            provider={target}
            update={update}
            onCancel={() => {
              closing(update.reset)(false);
            }}
            onDone={() => {
              closing(update.reset)(false);
              toast(DONE.update);
            }}
          />
          <ConfirmDialog
            open={asking === "delete"}
            onOpenChange={closing(remove.reset)}
            title={`Delete ${target?.name ?? "this provider"}?`}
            body={DELETE_CONSEQUENCE}
            confirmLabel="Delete"
            tone="danger"
            onConfirm={async () => {
              if (target === null) return;
              await remove.mutateAsync({ id: target.id });
              // The notice about a provider that is gone would say what is not so.
              if (target.name === added) setAdded(null);
              toast(DONE.delete);
            }}
          />
        </>
      ) : null}
    </>
  );
}
