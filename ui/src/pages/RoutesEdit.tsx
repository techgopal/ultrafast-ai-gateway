import { useForm, useSelector } from "@tanstack/react-form";
import { Link, useNavigate } from "@tanstack/react-router";
import { useMemo, useRef, useState } from "react";
import {
  useCreateRoute,
  useModels,
  useRoute,
  useTeams,
  useUpdateRoute,
} from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { Checks } from "@/components/CheckList";
import { control } from "@/components/classes";
import { ErrorState } from "@/components/ErrorState";
import { Field } from "@/components/Field";
import {
  applyApiError,
  onField,
  useFocusOnFailure,
  useFormFailure,
  useSubmit,
} from "@/components/form";
import { FormError } from "@/components/FormError";
import { GuardrailPicker, ROUTE_HINT } from "@/components/GuardrailPicker";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { NotFoundContent } from "@/components/NotFoundContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { attachmentOf } from "@/lib/guardrails";
import { idOf } from "@/lib/id";
import { sortModels } from "@/lib/models";
import {
  CACHE_SCOPES,
  check,
  DEFAULTS,
  emptyForm,
  formOf,
  hasProblems,
  inFormWords,
  requestOf,
  teamsErrorAside,
  type Audience,
  type FieldName,
} from "@/lib/routes";
import { Fallbacks, Primaries } from "@/pages/RoutesEditTargets";
import { RoutesHealth } from "@/pages/RoutesHealth";

type Route = components["schemas"]["RouteView"];

export const DONE = {
  create: "Route created.",
  update: "Route saved.",
} as const;

export const NAME_HINT = "Clients call the route by this name. It cannot contain a slash.";
export const PRIMARIES_HINT = "Calls are spread over the primary targets by their weights.";
export const FALLBACKS_HINT = "Tried in this order when the primary targets fail.";
export const RENAME_HINT = "Keys that list this route by name stop working when it is renamed.";
export const BREAKER_HINT =
  "Circuit breaker settings apply per provider model and are shared by every route that uses it.";
export const CACHE_HINT = "Streams and requests with temperature above 0.5 are never cached.";
export const CACHE_SCOPE_HINT = "Whose calls share a cached answer.";
export const FIX_THE_FIELDS = "Some fields are not valid. They are marked below.";

const AUDIENCES: readonly [Audience, string][] = [
  ["all", "All teams"],
  ["chosen", "Chosen teams"],
  ["admins", "Admins only"],
];

/** The settings: the field of the form, its label, and what is said under it. */
const SETTINGS = [
  ["retries", "Retries", `Default ${DEFAULTS.retries}. 0 to 5.`, "numeric"],
  [
    "first_token_s",
    "First token timeout (s)",
    `Default ${DEFAULTS.first_token_s}. 1 to 300 seconds.`,
    "decimal",
  ],
  [
    "total_s",
    "Total timeout (s)",
    `Default ${DEFAULTS.total_s}. 1 to 3600 seconds, not below the first token timeout.`,
    "decimal",
  ],
  [
    "breaker_failures",
    "Breaker failures",
    `Default ${DEFAULTS.breaker_failures}. Failures within the window that open the breaker. 1 to 100.`,
    "numeric",
  ],
  [
    "breaker_window_s",
    "Breaker window (s)",
    `Default ${DEFAULTS.breaker_window_s}. 5 to 3600 seconds.`,
    "numeric",
  ],
  [
    "breaker_open_s",
    "Breaker open (s)",
    `Default ${DEFAULTS.breaker_open_s}. How long a target is left alone once the breaker opens. 5 to 3600 seconds.`,
    "numeric",
  ],
] as const;

interface EditorProps {
  /** `null` for a new route. */
  route: Route | null;
}

function Editor({ route }: EditorProps) {
  const models = useModels();
  const teams = useTeams();
  const create = useCreateRoute();
  const update = useUpdateRoute();
  const toast = useToast();
  const navigate = useNavigate();
  const [attempted, setAttempted] = useState(false);
  const [advanced, setAdvanced] = useState(false);

  // The models a target can be: the enabled ones, and those the route has already.
  const choices = useMemo(() => {
    const own = new Set([...(route?.primaries ?? []), ...(route?.fallbacks ?? [])].map((t) => t.model_id));
    return sortModels((models.data?.models ?? []).filter((m) => m.enabled || own.has(m.id)));
  }, [models.data, route]);
  const teamList = useMemo(() => teams.data?.teams ?? null, [teams.data]);
  const offered = useMemo(
    () => ({ models: choices.map((m) => m.id), teams: (teamList ?? []).map((t) => t.id) }),
    [choices, teamList],
  );
  const pending = create.isPending || update.isPending;

  const form = useForm({
    defaultValues: {
      ...(route === null ? emptyForm() : formOf(route)),
      guardrail_ids: route?.guardrails.map((one) => one.id) ?? [],
    },
    onSubmit: async ({ value }) => {
      if (hasProblems(check(value, offered))) {
        setAttempted(true);
        failed();
        return;
      }
      const body = requestOf(value, offered);
      // Only an admin edits a route; the guardrails are left alone unless they were changed.
      const attached = attachmentOf(
        value.guardrail_ids,
        route === null ? null : route.guardrails.map((one) => one.id),
      );
      if (attached !== undefined) body.guardrail_ids = attached;
      try {
        if (route === null) {
          await create.mutateAsync(body);
          create.reset();
          toast(DONE.create);
        } else {
          await update.mutateAsync({ id: route.id, body });
          update.reset();
          toast(DONE.update);
        }
        await navigate({ to: "/routes" });
      } catch (error) {
        create.reset();
        update.reset();
        applyApiError(
          form,
          onField(
            teamsErrorAside(inFormWords(error), value.audience),
            "route_exists",
            "name",
          ),
        );
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
  const live = useMemo(() => (attempted ? check(values, offered) : null), [attempted, values, offered]);
  const errorOf = (name: FieldName | "audience"): string | undefined =>
    failure.fieldError(name) ?? (live === null || name === "audience" ? undefined : live.fields[name]);
  const settingProblem =
    SETTINGS.some(([name]) => errorOf(name) !== undefined) ||
    errorOf("cache_ttl_s") !== undefined ||
    errorOf("cache_scope") !== undefined;
  const showAdvanced = advanced || settingProblem;
  const waiting = values.audience === "chosen" && teamList === null;

  return (
    <form
      ref={formRef}
      aria-label="Route"
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
          <Field
            label="Name"
            name={field.name}
            required
            hint={
              route !== null && field.state.value.trim() !== route.name
                ? `${NAME_HINT} ${RENAME_HINT}`
                : NAME_HINT
            }
            error={errorOf("name")}
          >
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                autoComplete="off"
                autoCapitalize="none"
                spellCheck={false}
                className={`${control} font-mono`}
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

      <form.Field name="primaries">
        {(field) => (
          <Field
            group
            required
            label="Primary targets"
            name={field.name}
            hint={PRIMARIES_HINT}
            error={errorOf("primaries")}
          >
            {(wiring) => (
              <Primaries
                wiring={wiring}
                models={choices}
                rows={field.state.value}
                fallbacks={values.fallbacks}
                problems={live?.primaries ?? []}
                onChange={field.handleChange}
              />
            )}
          </Field>
        )}
      </form.Field>

      <form.Field name="fallbacks">
        {(field) => (
          <Field
            group
            label="Fallbacks"
            name={field.name}
            hint={FALLBACKS_HINT}
            error={failure.fieldError(field.name)}
          >
            {(wiring) => (
              <Fallbacks
                wiring={wiring}
                models={choices}
                rows={field.state.value}
                primaries={values.primaries.map((row) => row.model)}
                problems={live?.fallbacks ?? []}
                onChange={field.handleChange}
              />
            )}
          </Field>
        )}
      </form.Field>

      <form.Field name="audience">
        {(field) => (
          <Field group label="Teams" name={field.name} error={errorOf("audience")}>
            {({ id, name, ...described }) => (
              <RadioGroup
                {...described}
                id={id}
                name={name}
                value={field.state.value}
                onValueChange={(next) => {
                  field.handleChange(next === "all" || next === "chosen" ? next : "admins");
                }}
              >
                {AUDIENCES.map(([value, label]) => (
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
      {values.audience === "chosen" ? (
        <form.Field name="team_ids">
          {(field) => (
            <Field group label="Chosen teams" name={field.name} error={errorOf("team_ids")}>
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
      ) : null}

      <form.Field name="guardrail_ids">
        {(field) => (
          <Field
            group
            label="Guardrails"
            name={field.name}
            hint={ROUTE_HINT}
            error={failure.fieldError(field.name)}
          >
            {(wiring) => (
              <GuardrailPicker
                wiring={wiring}
                value={field.state.value}
                onChange={field.handleChange}
              />
            )}
          </Field>
        )}
      </form.Field>

      <section className="flex flex-col gap-4">
        <h2 className="text-base font-medium">
          <button
            type="button"
            aria-expanded={showAdvanced}
            aria-controls="route-advanced"
            className={`${control} -mx-2 rounded-md px-2 outline-none focus-visible:ring-3 focus-visible:ring-ring/50`}
            onClick={() => {
              setAdvanced(!showAdvanced);
            }}
          >
            Advanced
          </button>
        </h2>
        <div id="route-advanced" hidden={!showAdvanced} className="grid gap-4 sm:grid-cols-2">
          {SETTINGS.map(([name, label, hint, mode]) => (
            <form.Field key={name} name={name}>
              {(field) => (
                <Field label={label} name={field.name} hint={hint} error={errorOf(name)}>
                  {({ id, name: fieldName, ...described }) => (
                    <Input
                      {...described}
                      id={id}
                      name={fieldName}
                      inputMode={mode}
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
          ))}
          <p className="text-sm text-muted-foreground sm:col-span-2">{BREAKER_HINT}</p>
          <div className="sm:col-span-2">
            <form.Field name="cache_enabled">
              {(field) => (
                <Field
                  label="Cache answers"
                  name={field.name}
                  hint={CACHE_HINT}
                  error={failure.fieldError(field.name)}
                >
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
          </div>
          <form.Field name="cache_ttl_s">
            {(field) => (
              <Field
                label="TTL (s)"
                name={field.name}
                hint={`Default ${DEFAULTS.cache_ttl_s}. How long an answer is kept. 1 to 86400 seconds.`}
                error={errorOf("cache_ttl_s")}
              >
                {({ id, name, ...described }) => (
                  <Input
                    {...described}
                    id={id}
                    name={name}
                    inputMode="numeric"
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
          <form.Field name="cache_scope">
            {(field) => (
              <Field
                label="Scope"
                name={field.name}
                hint={CACHE_SCOPE_HINT}
                error={errorOf("cache_scope")}
              >
                {({ id, name, ...described }) => (
                  <Select name={name} value={field.state.value} onValueChange={field.handleChange}>
                    <SelectTrigger id={id} {...described} className={`${control} w-full`}>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {CACHE_SCOPES.map(([value, label]) => (
                        <SelectItem key={value} value={value}>
                          {label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              </Field>
            )}
          </form.Field>
        </div>
      </section>

      <div className="flex flex-wrap gap-2">
        <Button type="submit" className={control} disabled={pending || waiting}>
          {pending ? "Saving" : route === null ? "Create route" : "Save route"}
        </Button>
        <Button asChild variant="outline" className={control}>
          <Link to="/routes">Cancel</Link>
        </Button>
      </div>
      {models.error !== null && models.data === undefined ? (
        <ErrorState error={models.error} />
      ) : null}
    </form>
  );
}

function BackLink() {
  return (
    <Link
      to="/routes"
      className="inline-flex min-h-11 w-fit items-center rounded-sm text-sm text-muted-foreground underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-8"
    >
      Back to routing
    </Link>
  );
}

function Loading({ label, title }: { label: string; title: string }) {
  return (
    <>
      <PageHeader title={title} />
      <div role="status" aria-busy="true" aria-label={label} className="flex flex-col gap-4">
        <Skeleton className="h-4 w-full max-w-md" />
        <Skeleton className="h-4 w-full max-w-md" />
        <Skeleton className="h-4 w-full max-w-md" />
      </div>
    </>
  );
}

/** The models are needed to choose targets: until they are read there is nothing to choose from. */
function WithModels({ route }: { route: Route | null }) {
  const models = useModels();
  if (models.data === undefined) {
    if (models.error !== null) {
      return (
        <QueryProblem
          title="Routing"
          error={models.error}
          onRetry={() => {
            void models.refetch();
          }}
        />
      );
    }
    return <Loading label="Loading the models" title={route === null ? "New route" : "Edit route"} />;
  }
  return (
    <>
      <BackLink />
      <PageHeader
        title={route === null ? "New route" : "Edit route"}
        {...(route === null ? {} : { subtitle: route.name })}
      />
      <Editor route={route} />
      {route === null ? null : <RoutesHealth title="Health of this route" route={route} />}
    </>
  );
}

function Existing({ id }: { id: number }) {
  const route = useRoute(id);
  if (route.data === undefined) {
    if (route.error !== null) {
      return (
        <QueryProblem
          notFound
          title="Routing"
          error={route.error}
          onRetry={() => {
            void route.refetch();
          }}
        />
      );
    }
    return <Loading label="Loading the route" title="Edit route" />;
  }
  return <WithModels route={route.data} />;
}

/**
 * The page of a new route (`id` is `null`) or of one route, as the address
 * has its id. Only an admin changes routes.
 */
export function RoutesEdit({ id }: { id: string | null }) {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  if (!can(session.me, { type: "manageRoutes" })) return <NotAvailableContent />;
  if (id === null) return <WithModels route={null} />;
  const number = idOf(id);
  // Not an id: the API is not asked.
  if (number === null) return <NotFoundContent />;
  return <Existing id={number} />;
}
