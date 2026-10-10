import { useForm, useSelector } from "@tanstack/react-form";
import { Link, useNavigate } from "@tanstack/react-router";
import { useId, useMemo, useRef, useState } from "react";
import { useCreateGuardrail, useGuardrails, useUpdateGuardrail } from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import {
  applyApiError,
  onField,
  useFocusOnFailure,
  useFormFailure,
  useSubmit,
} from "@/components/form";
import { FormError } from "@/components/FormError";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { NotFoundContent } from "@/components/NotFoundContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
  check,
  DIRECTIONS,
  emptyForm,
  formOf,
  hasProblems,
  requestOf,
  ruleErrorsOf,
  ruleOf,
  TIMEOUT_MAX,
  TIMEOUT_MIN,
  type FailMode,
  type Kind,
  type RuleErrors,
  type RuleRow,
} from "@/lib/guardrails";
import { idOf } from "@/lib/id";
import { Rules } from "@/pages/GuardrailsRules";
import { Try } from "@/pages/GuardrailsTry";
import { SecretNote, SECRET_SHOWN_ONCE } from "@/pages/GuardrailsSecret";

type Guardrail = components["schemas"]["GuardrailView"];

export const DONE = {
  create: "Guardrail created.",
  update: "Guardrail saved.",
} as const;

export const FIX_THE_FIELDS = "Some fields are not valid. They are marked below.";
export const NAME_HINT = "Shown in logs and in the message a blocked call gets.";
export const ENABLED_HINT = "A guardrail that is off checks nothing, wherever it is attached.";
export const DEFAULT_HINT =
  "Checks every call of the gateway, before the guardrails of the route and of the key. Routes and keys attach guardrails in their own forms.";
export const KIND_HINT = "The kind cannot be changed once the guardrail is made.";
export const URL_HINT =
  "Where the gateway posts the text. It is stored encrypted and only its host is shown again.";
export const KEEP_URL_HINT = "Leave empty to keep the current URL. The gateway never shows it.";
export const NEEDS_URL_HINT = "This guardrail has no URL yet. Set one to be able to enable it.";
export const DIRECTIONS_HINT = "Which texts are sent to the guardrail.";
export const FAIL_HINT =
  "What happens when the guardrail cannot be reached or answers badly: open lets the text through and flags the call, closed blocks it.";
export const TIMEOUT_HINT = `${String(TIMEOUT_MIN)} to ${String(TIMEOUT_MAX)} milliseconds.`;

const KIND_CHOICES: readonly [Kind, string][] = [
  ["rules", "Rules in the gateway"],
  ["external", "External webhook"],
];

const FAIL_CHOICES: readonly [FailMode, string][] = [
  ["open", "Open: let the text through and flag the call"],
  ["closed", "Closed: block the call"],
];

interface SwitchFieldProps {
  label: string;
  name: string;
  hint: string;
  checked: boolean;
  error: string | undefined;
  onChange: (on: boolean) => void;
}

/** A switch inside its label, which is the target to touch, with its hint and its error under it. */
function SwitchField({ label, name, hint, checked, error, onChange }: SwitchFieldProps) {
  const id = useId();
  const hintId = `${id}-hint`;
  const errorId = `${id}-error`;
  return (
    <div className="flex flex-col gap-1">
      <Label htmlFor={id} className={`${control} gap-2`}>
        <Switch
          id={id}
          name={name}
          aria-describedby={error === undefined ? hintId : `${errorId} ${hintId}`}
          {...(error === undefined ? {} : { "aria-invalid": true })}
          checked={checked}
          onCheckedChange={onChange}
        />
        {label}
      </Label>
      {error === undefined ? null : (
        <p id={errorId} role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <p id={hintId} className="text-sm text-muted-foreground">
        {hint}
      </p>
    </div>
  );
}

interface EditorProps {
  /** `null` for a new guardrail. */
  guardrail: Guardrail | null;
}

function Editor({ guardrail }: EditorProps) {
  const create = useCreateGuardrail();
  const update = useUpdateGuardrail();
  const once = useSecretOnce(create);
  const toast = useToast();
  const navigate = useNavigate();
  const [attempted, setAttempted] = useState(false);
  // What the gateway said of the rules, and the rules it said it of.
  const [named, setNamed] = useState<{ errors: RuleErrors; rules: readonly RuleRow[] } | null>(null);
  const pending = create.isPending || update.isPending;

  // Made once: the rows of a form have keys that differ each time one is made, and a form whose
  // default values change resets itself.
  const [start] = useState(() => (guardrail === null ? emptyForm() : formOf(guardrail)));
  const form = useForm({
    defaultValues: start,
    onSubmit: async ({ value }) => {
      setNamed(null);
      if (hasProblems(check(value, guardrail))) {
        setAttempted(true);
        failed();
        return;
      }
      const body = requestOf(value, guardrail);
      try {
        if (guardrail === null) {
          const made = await create.mutateAsync({ ...body, name: body.name, kind: value.kind });
          if (made.secret !== null) {
            // The signing secret is shown once; the page leaves when it is closed.
            form.setFieldValue("url", "");
            once.show(made.secret);
            return;
          }
          create.reset();
          toast(DONE.create);
        } else {
          await update.mutateAsync({ id: guardrail.id, body });
          form.setFieldValue("url", "");
          update.reset();
          toast(DONE.update);
        }
        await navigate({ to: "/guardrails" });
      } catch (error) {
        create.reset();
        update.reset();
        const found = ruleErrorsOf(error);
        setNamed({ errors: found, rules: value.rules });
        applyApiError(form, onField(found.rest, "guardrail_exists", "name"));
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const failed = useFocusOnFailure(formRef, errorRef);
  const onSubmit = useSubmit(form);
  const values = useSelector(form.store, (state) => state.values);

  // After a first attempt the form says what is wrong while it is mended.
  const live = useMemo(
    () => (attempted ? check(values, guardrail) : null),
    [attempted, values, guardrail],
  );
  const errorOf = (name: "name" | "url" | "timeout_ms"): string | undefined =>
    failure.fieldError(name) ?? live?.fields[name];
  // The gateway's word on the rules holds until they are changed.
  const fromGateway = named !== null && named.rules === values.rules ? named.errors : null;
  const rowErrors: Record<number, string> = { ...live?.rows, ...fromGateway?.rows };
  const setError = fromGateway?.set ?? live?.fields.rules;
  const tryable = useMemo(
    () => (hasProblems({ fields: {}, rows: check(values, guardrail).rows }) ? null : values.rules.map(ruleOf)),
    [values, guardrail],
  );
  const external = values.kind === "external";

  return (
    <>
      <form
        ref={formRef}
        aria-label="Guardrail"
        noValidate
        className="flex max-w-2xl flex-col gap-6"
        onSubmit={onSubmit}
      >
        <FormError
          ref={errorRef}
          messages={
            failure.messages.length > 0 || live === null || !hasProblems(live)
              ? failure.messages
              : [FIX_THE_FIELDS]
          }
        />
        <form.Field name="name">
          {(field) => (
            <Field label="Name" name={field.name} required hint={NAME_HINT} error={errorOf("name")}>
              <Input
                autoComplete="off"
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
        <form.Field name="description">
          {(field) => (
            <Field label="Description" name={field.name} error={failure.fieldError(field.name)}>
              <Textarea
                autoComplete="off"
                className="min-h-16"
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
          )}
        </form.Field>
        {guardrail === null ? (
          <form.Field name="kind">
            {(field) => (
              <Field group label="Kind" name={field.name} hint={KIND_HINT}>
                {({ id, name, ...described }) => (
                  <RadioGroup
                    {...described}
                    id={id}
                    name={name}
                    value={field.state.value}
                    onValueChange={(next) => {
                      field.handleChange(next === "external" ? "external" : "rules");
                    }}
                  >
                    {KIND_CHOICES.map(([value, label]) => (
                      <Label key={value} htmlFor={`${id}-${value}`} className={control}>
                        <RadioGroupItem id={`${id}-${value}`} value={value} />
                        {label}
                      </Label>
                    ))}
                  </RadioGroup>
                )}
              </Field>
            )}
          </form.Field>
        ) : null}
        <form.Field name="enabled">
          {(field) => (
            <SwitchField
              label="Enabled"
              name={field.name}
              hint={
                guardrail !== null && external && guardrail.url_host === ""
                  ? `${ENABLED_HINT} ${NEEDS_URL_HINT}`
                  : ENABLED_HINT
              }
              checked={field.state.value}
              error={failure.fieldError(field.name)}
              onChange={field.handleChange}
            />
          )}
        </form.Field>
        <form.Field name="is_default">
          {(field) => (
            <SwitchField
              label="Applies to every call"
              name={field.name}
              hint={DEFAULT_HINT}
              checked={field.state.value}
              error={failure.fieldError(field.name)}
              onChange={field.handleChange}
            />
          )}
        </form.Field>

        {external ? (
          <>
            <form.Field name="url">
              {(field) => (
                <Field
                  label="URL"
                  name={field.name}
                  required={guardrail === null}
                  hint={
                    guardrail === null
                      ? URL_HINT
                      : guardrail.url_host === ""
                        ? NEEDS_URL_HINT
                        : `${KEEP_URL_HINT} It posts to ${guardrail.url_host ?? ""}.`
                  }
                  error={errorOf("url")}
                >
                  <Input
                    inputMode="url"
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
            <form.Field name="directions">
              {(field) => (
                <Field group label="Asked about" name={field.name} hint={DIRECTIONS_HINT}>
                  {({ id, name, ...described }) => (
                    <RadioGroup
                      {...described}
                      id={id}
                      name={name}
                      value={field.state.value}
                      onValueChange={(next) => {
                        field.handleChange(
                          DIRECTIONS.find(([value]) => value === next)?.[0] ?? "both",
                        );
                      }}
                    >
                      {DIRECTIONS.map(([value, label]) => (
                        <Label key={value} htmlFor={`${id}-${value}`} className={control}>
                          <RadioGroupItem id={`${id}-${value}`} value={value} />
                          {label}
                        </Label>
                      ))}
                    </RadioGroup>
                  )}
                </Field>
              )}
            </form.Field>
            <form.Field name="fail_mode">
              {(field) => (
                <Field group label="When it fails" name={field.name} hint={FAIL_HINT}>
                  {({ id, name, ...described }) => (
                    <RadioGroup
                      {...described}
                      id={id}
                      name={name}
                      value={field.state.value}
                      onValueChange={(next) => {
                        field.handleChange(next === "closed" ? "closed" : "open");
                      }}
                    >
                      {FAIL_CHOICES.map(([value, label]) => (
                        <Label key={value} htmlFor={`${id}-${value}`} className={control}>
                          <RadioGroupItem id={`${id}-${value}`} value={value} />
                          {label}
                        </Label>
                      ))}
                    </RadioGroup>
                  )}
                </Field>
              )}
            </form.Field>
            <form.Field name="timeout_ms">
              {(field) => (
                <Field
                  label="Timeout (ms)"
                  name={field.name}
                  hint={TIMEOUT_HINT}
                  error={errorOf("timeout_ms")}
                >
                  <Input
                    inputMode="numeric"
                    autoComplete="off"
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
          </>
        ) : (
          <form.Field name="rules">
            {(field) => (
              <Field group label="Rules" name={field.name} error={setError}>
                {(wiring) => (
                  <div role="group" aria-labelledby={wiring["aria-labelledby"]} id={wiring.id}>
                    <Rules rows={field.state.value} errors={rowErrors} onChange={field.handleChange} />
                  </div>
                )}
              </Field>
            )}
          </form.Field>
        )}

        <div className="flex flex-wrap gap-2">
          <Button type="submit" className={control} disabled={pending}>
            {pending ? "Saving" : guardrail === null ? "Create guardrail" : "Save guardrail"}
          </Button>
          <Button asChild variant="outline" className={control}>
            <Link to="/guardrails">Cancel</Link>
          </Button>
        </div>
      </form>
      <div className="max-w-2xl">
        <Try guardrail={guardrail} kind={values.kind} rules={tryable} />
      </div>
      <SecretDialog
        title="Signing secret"
        description={SECRET_SHOWN_ONCE}
        secret={once.secret}
        onClose={() => {
          once.clear();
          toast(DONE.create);
          void navigate({ to: "/guardrails" });
        }}
      >
        <SecretNote />
      </SecretDialog>
    </>
  );
}

function BackLink() {
  return (
    <Link
      to="/guardrails"
      className="inline-flex min-h-11 w-fit items-center rounded-sm text-sm text-muted-foreground underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-8"
    >
      Back to guardrails
    </Link>
  );
}

function Loading({ title }: { title: string }) {
  return (
    <>
      <PageHeader title={title} />
      <div role="status" aria-busy="true" aria-label="Loading the guardrail" className="flex flex-col gap-4">
        <Skeleton className="h-4 w-full max-w-md" />
        <Skeleton className="h-4 w-full max-w-md" />
        <Skeleton className="h-4 w-full max-w-md" />
      </div>
    </>
  );
}

function Attachments({ guardrail }: { guardrail: Guardrail }) {
  const routes = guardrail.routes.map((route) => route.name);
  return (
    <p className="max-w-2xl text-sm text-muted-foreground">
      {guardrail.is_default ? "Applies to every call. " : ""}
      {routes.length === 0 ? "On no route" : `On the routes ${routes.join(", ")}`}
      {`, and on ${String(guardrail.key_count)} ${guardrail.key_count === 1 ? "key" : "keys"}.`}
    </p>
  );
}

function Existing({ id }: { id: number }) {
  const list = useGuardrails();
  if (list.data === undefined) {
    if (list.error !== null) {
      return (
        <QueryProblem
          title="Guardrails"
          error={list.error}
          onRetry={() => {
            void list.refetch();
          }}
        />
      );
    }
    return <Loading title="Edit guardrail" />;
  }
  const guardrail = list.data.guardrails.find((one) => one.id === id);
  if (guardrail === undefined) return <NotFoundContent />;
  return (
    <>
      <BackLink />
      <PageHeader title="Edit guardrail" subtitle={guardrail.name} />
      <Attachments guardrail={guardrail} />
      {/* Another guardrail is another form. */}
      <Editor key={`${String(guardrail.id)}-${guardrail.created_at}`} guardrail={guardrail} />
    </>
  );
}

/**
 * The page of a new guardrail (`id` is `null`) or of one guardrail, as the
 * address has its id. Only an admin changes guardrails.
 */
export function GuardrailsEdit({ id }: { id: string | null }) {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  if (!can(session.me, { type: "manageGuardrails" })) return <NotAvailableContent />;
  if (id === null) {
    return (
      <>
        <BackLink />
        <PageHeader title="New guardrail" />
        <Editor guardrail={null} />
      </>
    );
  }
  const number = idOf(id);
  // Not an id: the API is not asked.
  if (number === null) return <NotFoundContent />;
  return <Existing id={number} />;
}
