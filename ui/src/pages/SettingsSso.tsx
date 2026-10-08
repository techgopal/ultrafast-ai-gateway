import { useForm } from "@tanstack/react-form";
import { useId, useRef, useState } from "react";
import { useOidcSettings, useTestOidc, useUpdateOidcSettings } from "@/api/queries";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormError } from "@/components/FormError";
import { QueryProblem } from "@/components/QueryProblem";
import { useToast } from "@/components/toast";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";

type Oidc = components["schemas"]["OidcView"];
type TestResult = components["schemas"]["OidcTestResult"];

export const SSO_DONE = "Single sign-on settings saved.";
export const ADDRESS_COPIED = "Address copied.";
export const ADDRESS_NOT_COPIED = "Could not copy. Select the address and copy it.";
export const SECRET_UNREADABLE =
  "The stored client secret cannot be read, perhaps because the master key changed. Single sign-on stays off until you enter the client secret again and save.";
export const NEEDS_PUBLIC_URL =
  "Not available. Start the gateway with UF_PUBLIC_URL (for example https://gateway.example.com) to get the address to register at your identity provider.";

/** Where the issuer of each provider comes from: text only, the console links to nothing outside. */
const ISSUER_EXAMPLES = [
  "Google: https://accounts.google.com",
  "Microsoft Entra ID: https://login.microsoftonline.com/<tenant id>/v2.0",
  "Okta: https://<your org>.okta.com",
  "Keycloak: https://<host>/realms/<realm>",
] as const;

interface Values {
  enabled: boolean;
  label: string;
  issuer: string;
  client_id: string;
  client_secret: string;
  scopes: string;
  groups_claim: string;
  admin_group: string;
  link_by_email: boolean;
  auto_create: boolean;
  allowed_domains: string;
}

/** The form's values for what is saved. The secret is never among it. */
function valuesOf(view: Oidc): Values {
  return {
    enabled: view.enabled,
    label: view.label,
    issuer: view.issuer,
    client_id: view.client_id,
    client_secret: "",
    scopes: view.scopes,
    groups_claim: view.groups_claim,
    admin_group: view.admin_group,
    link_by_email: view.link_by_email,
    auto_create: view.auto_create,
    allowed_domains: view.allowed_domains.join(", "),
  };
}

/** Domains are separated by commas or spaces. */
function domainsOf(text: string): string[] {
  return text.split(/[\s,]+/).filter((domain) => domain !== "");
}

function sayResult(result: TestResult): string {
  if (!result.ok) return `Not working: ${result.error ?? "the provider did not answer as expected."}`;
  const keys = result.jwks_keys ?? 0;
  const who = result.issuer === undefined || result.issuer === null ? "" : ` ${result.issuer},`;
  return `The provider answers:${who} ${String(keys)} signing ${keys === 1 ? "key" : "keys"}.`;
}

function RedirectUri({ uri }: { uri: string | null | undefined }) {
  const toast = useToast();
  const id = useId();
  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast(ADDRESS_COPIED);
    } catch {
      toast(ADDRESS_NOT_COPIED, "error");
    }
  }
  return (
    <div className="flex flex-col gap-2">
      <span id={id} className="text-sm leading-none font-medium">
        Address to register at the provider (redirect URI)
      </span>
      {uri === null || uri === undefined ? (
        <p className="text-sm text-muted-foreground">{NEEDS_PUBLIC_URL}</p>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          <code aria-labelledby={id} className="min-w-0 break-all rounded-md bg-muted px-2 py-1 text-sm">
            {uri}
          </code>
          <Button
            type="button"
            variant="outline"
            className={control}
            aria-label="Copy address"
            onClick={() => {
              void copy(uri);
            }}
          >
            Copy
          </Button>
        </div>
      )}
    </div>
  );
}

function SsoForm({ view }: { view: Oidc }) {
  const update = useUpdateOidcSettings();
  const test = useTestOidc();
  const toast = useToast();
  const { mutateAsync, reset } = update;
  const { mutateAsync: runTest, reset: resetTest } = test;
  const [result, setResult] = useState<TestResult | null>(null);
  const ids = {
    link: useId(),
    create: useId(),
    enabled: useId(),
    enabledHint: useId(),
    enabledError: useId(),
  };
  const form = useForm({
    defaultValues: valuesOf(view),
    onSubmit: async ({ value }) => {
      try {
        const { client_secret: secret, allowed_domains: domains, ...rest } = value;
        const saved = await mutateAsync({
          ...rest,
          allowed_domains: domainsOf(domains),
          // Write only: left out, the stored secret stays.
          ...(secret === "" ? {} : { client_secret: secret }),
        });
        reset();
        // What the gateway kept (domains in lower case), and no secret in the field.
        form.reset(valuesOf(saved));
        toast(SSO_DONE);
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

  async function testIt() {
    setResult(null);
    try {
      const issuer = form.state.values.issuer.trim();
      setResult(await runTest(issuer === "" ? {} : { issuer }));
    } catch (error) {
      applyApiError(form, error);
    } finally {
      resetTest();
    }
  }

  return (
    <form
      ref={formRef}
      aria-label="Single sign-on"
      noValidate
      className="flex max-w-md flex-col gap-4"
      onSubmit={onSubmit}
    >
      {view.client_secret_unreadable ? <Alert variant="destructive">{SECRET_UNREADABLE}</Alert> : null}
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="enabled">
        {(field) => {
          const error = failure.fieldError(field.name);
          return (
            <div className="flex flex-col gap-1">
              <Label htmlFor={ids.enabled} className={`${control} gap-2`}>
                <Switch
                  id={ids.enabled}
                  name={field.name}
                  aria-describedby={error === undefined ? ids.enabledHint : `${ids.enabledError} ${ids.enabledHint}`}
                  {...(error === undefined ? {} : { "aria-invalid": true })}
                  checked={field.state.value}
                  disabled={!view.public_url_set && !field.state.value}
                  onCheckedChange={field.handleChange}
                />
                Single sign-on
              </Label>
              {error === undefined ? null : (
                <p id={ids.enabledError} role="alert" className="text-sm text-destructive">
                  {error}
                </p>
              )}
              <p id={ids.enabledHint} className="text-sm text-muted-foreground">
                {view.public_url_set
                  ? "Offers a button on the sign-in page. Passwords keep working."
                  : "Needs the gateway to be started with UF_PUBLIC_URL."}
              </p>
            </div>
          );
        }}
      </form.Field>
      <form.Field name="label">
        {(field) => (
          <Field
            label="Label on the sign-in button"
            name={field.name}
            hint="The button says “Sign in with” and this name. 1 to 40 characters."
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                autoComplete="off"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            )}
          </Field>
        )}
      </form.Field>
      <form.Field name="issuer">
        {(field) => (
          <Field
            label="Issuer"
            name={field.name}
            hint="The address of your identity provider, https only."
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                inputMode="url"
                autoComplete="off"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  setResult(null);
                  field.handleChange(event.target.value);
                }}
              />
            )}
          </Field>
        )}
      </form.Field>
      <ul aria-label="Provider examples" className="flex flex-col gap-1 text-sm text-muted-foreground">
        {ISSUER_EXAMPLES.map((example) => (
          <li key={example} className="break-words">
            {example}
          </li>
        ))}
      </ul>
      <form.Field name="client_id">
        {(field) => (
          <Field label="Client ID" name={field.name} error={failure.fieldError(field.name)}>
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                autoComplete="off"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            )}
          </Field>
        )}
      </form.Field>
      <form.Field name="client_secret">
        {(field) => (
          <Field
            label={view.client_secret_set ? "Replace client secret" : "Client secret"}
            name={field.name}
            hint={
              view.client_secret_set
                ? "Set. It is never shown again. Enter a new secret to replace it; leave this empty to keep it."
                : "Not set. It is never shown again after saving."
            }
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                type="password"
                autoComplete="new-password"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            )}
          </Field>
        )}
      </form.Field>
      <form.Field name="scopes">
        {(field) => (
          <Field
            label="Extra scopes"
            name={field.name}
            hint="Asked for besides openid, email and profile. Separate with spaces."
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                autoComplete="off"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            )}
          </Field>
        )}
      </form.Field>
      <form.Field name="groups_claim">
        {(field) => (
          <Field
            label="Groups claim"
            name={field.name}
            hint="The claim of the ID token that lists the groups of the user."
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                autoComplete="off"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            )}
          </Field>
        )}
      </form.Field>
      <form.Field name="admin_group">
        {(field) => (
          <Field
            label="Admin group"
            name={field.name}
            hint="Members of this group become admins when they sign in. Empty: signing in never changes a role."
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                autoComplete="off"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            )}
          </Field>
        )}
      </form.Field>
      <form.Field name="link_by_email">
        {(field) => (
          <div className="flex flex-col gap-1">
            <Label htmlFor={ids.link} className={control}>
              <Checkbox
                id={ids.link}
                checked={field.state.value}
                onCheckedChange={(checked) => {
                  field.handleChange(checked === true);
                }}
              />
              Link users by email
            </Label>
            <p className="text-sm text-muted-foreground">
              A person who signs in is matched to the user with the same verified email address.
            </p>
          </div>
        )}
      </form.Field>
      <form.Field name="auto_create">
        {(field) => (
          <div className="flex flex-col gap-1">
            <Label htmlFor={ids.create} className={control}>
              <Checkbox
                id={ids.create}
                checked={field.state.value}
                onCheckedChange={(checked) => {
                  field.handleChange(checked === true);
                }}
              />
              Create users on first sign-in
            </Label>
            <p className="text-sm text-muted-foreground">
              A person from an allowed domain gets a member account. Needs at least one domain.
            </p>
            {failure.fieldError(field.name) === undefined ? null : (
              <p role="alert" className="text-sm text-destructive">
                {failure.fieldError(field.name)}
              </p>
            )}
          </div>
        )}
      </form.Field>
      <form.Field name="allowed_domains">
        {(field) => (
          <Field
            label="Allowed email domains"
            name={field.name}
            hint="Separate with commas or spaces, for example example.com, corp.example.com."
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                autoComplete="off"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            )}
          </Field>
        )}
      </form.Field>
      <RedirectUri uri={view.redirect_uri} />
      {result === null ? null : (
        <p role="status" className="text-sm">
          {sayResult(result)}
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <Button type="submit" className={control} disabled={update.isPending}>
          {update.isPending ? "Saving" : "Save single sign-on"}
        </Button>
        <Button
          type="button"
          variant="outline"
          className={control}
          disabled={test.isPending}
          onClick={() => {
            void testIt();
          }}
        >
          {test.isPending ? "Testing" : "Test configuration"}
        </Button>
      </div>
    </form>
  );
}

/** Single sign-on with an OpenID Connect provider: only an admin gets this page. */
export function SsoSection() {
  const headingId = useId();
  const oidc = useOidcSettings();
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-4">
      <h2 id={headingId} className="text-lg font-medium">
        Single sign-on (OIDC)
      </h2>
      {oidc.data !== undefined ? (
        <SsoForm view={oidc.data} />
      ) : oidc.error !== null ? (
        <QueryProblem
          part
          error={oidc.error}
          onRetry={() => {
            void oidc.refetch();
          }}
        />
      ) : (
        <div
          role="status"
          aria-busy="true"
          aria-label="Loading single sign-on"
          className="flex max-w-md flex-col gap-4"
        >
          <Skeleton className="h-4 w-48" />
          <Skeleton className="h-8 w-full" />
        </div>
      )}
    </section>
  );
}
