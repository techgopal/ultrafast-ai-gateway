import { useForm } from "@tanstack/react-form";
import { useMemo, useRef } from "react";
import {
  useAlertChannels,
  useBudgets,
  useCreateAlertRule,
  useKeys,
  useModels,
  useProviders,
  useRoutes,
  useUpdateAlertRule,
} from "@/api/queries";
import type { components } from "@/api/schema";
import { Checks } from "@/components/CheckList";
import { control, cutLongChoice, selectList } from "@/components/classes";
import { ErrorState } from "@/components/ErrorState";
import { Field } from "@/components/Field";
import { applyApiError, onField, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
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
import {
  ANY,
  budgetName,
  DEFAULT_PERCENT,
  emptyRuleForm,
  KINDS,
  kindText,
  paramsOnFields,
  ruleChangesOf,
  ruleFormOf,
  ruleRequestOf,
  SCOPES,
  type Offered,
} from "@/lib/alerts";

type Rule = components["schemas"]["RuleView"];

export const RULE_DESCRIPTION =
  "A rule watches one thing and tells its channels when it fires and when it resolves.";
export const PERCENT_HINT = "From 1 to 100.";
export const CHANNELS_HINT =
  "Who is told. A rule with no channel still records its events in History.";
export const ERROR_RATE_HELP =
  "It resolves after the rate stays below the threshold for a full window. That can take up to about two windows after the last error.";

const selectTrigger = `${control} w-full ${cutLongChoice}`;

interface Choice {
  value: string;
  label: string;
}

interface ChooseProps {
  label: string;
  name: string;
  value: string;
  choices: readonly Choice[];
  onChange: (value: string) => void;
  hint?: string;
  error?: string | undefined;
  /** What the rule has, when it is not among the choices: it is shown while it is untouched. */
  stored?: { value: string; label: string } | undefined;
}

/** A select among the choices; the choice that leaves a parameter out is first. */
function Choose({ label, name, value: wanted, choices: offered, onChange, hint, error, stored }: ChooseProps) {
  // What the select shows is what the rule has, and what the request sends.
  const choices =
    stored === undefined || wanted !== stored.value || offered.some((one) => one.value === wanted)
      ? offered
      : [...offered, { value: stored.value, label: `${stored.label} (not available)` }];
  const value = choices.some((one) => one.value === wanted) ? wanted : ANY;
  return (
    <Field label={label} name={name} error={error} hint={hint}>
      {({ id, name: fieldName, ...described }) => (
        <Select name={fieldName} value={value} onValueChange={onChange}>
          <SelectTrigger id={id} {...described} className={selectTrigger}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent className={selectList}>
            {choices.map((choice) => (
              <SelectItem key={choice.value} value={choice.value}>
                {choice.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </Field>
  );
}

/** What the form offers, read once from the lists. */
interface Lists {
  budgets: components["schemas"]["BudgetView"][];
  routes: components["schemas"]["RouteView"][];
  providers: components["schemas"]["ProviderView"][];
  keys: components["schemas"]["KeyView"][];
  models: components["schemas"]["ModelView"][];
  channels: components["schemas"]["ChannelView"][];
}

function offeredOf(lists: Lists): Offered {
  return {
    budgets: lists.budgets.map((one) => one.id),
    routes: lists.routes.map((one) => one.name),
    providers: lists.providers.map((one) => one.name),
    keys: lists.keys.filter((one) => one.status !== "revoked").map((one) => one.id),
    models: lists.models.map((one) => ({ provider: one.provider_name, name: one.name })),
    channels: lists.channels.map((one) => one.id),
  };
}

interface FormProps {
  /** The rule that is changed; `null` for a new one. */
  rule: Rule | null;
  lists: Lists;
  create: ReturnType<typeof useCreateAlertRule>;
  update: ReturnType<typeof useUpdateAlertRule>;
  /** The rule was saved; `changed` is false when there was nothing to send. */
  onDone: (changed: boolean) => void;
  onCancel: () => void;
}

// Mounted while the dialog is open and the lists are there: every opening
// starts from the rule as it is.
function RuleFormBody({ rule, lists, create, update, onDone, onCancel }: FormProps) {
  const offered = useMemo(() => offeredOf(lists), [lists]);
  const running = create.isPending || update.isPending;
  const form = useForm({
    defaultValues: rule === null ? emptyRuleForm() : ruleFormOf(rule),
    onSubmit: async ({ value }) => {
      try {
        const request = ruleRequestOf(value, offered, rule ?? undefined);
        if (rule === null) {
          await create.mutateAsync(request);
          create.reset();
          onDone(true);
          return;
        }
        const changes = ruleChangesOf(rule, request);
        if (Object.keys(changes).length === 0) {
          onDone(false);
          return;
        }
        await update.mutateAsync({ id: rule.id, body: changes });
        update.reset();
        onDone(true);
      } catch (error) {
        create.reset();
        update.reset();
        applyApiError(form, paramsOnFields(onField(error, "alert_rule_exists", "name")));
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const onSubmit = useSubmit(form);

  const was = rule === null ? null : ruleFormOf(rule);
  const keyName = (id: string) => {
    const found = lists.keys.find((one) => String(one.id) === id);
    return found === undefined ? `Key ${id}` : `${found.name} (revoked)`;
  };
  const storedOf = (value: string | undefined, label: (value: string) => string) =>
    value === undefined || value === ANY ? undefined : { value, label: label(value) };
  const budgetChoices: Choice[] = [
    { value: ANY, label: "Any budget" },
    ...lists.budgets.map((one) => ({ value: String(one.id), label: budgetName(one) })),
  ];
  const subjectChoices = (scope: string): { noun: string; choices: Choice[] } => {
    const all = (noun: string) => ({ value: ANY, label: `Each ${noun}` });
    if (scope === "route") {
      return {
        noun: "Route",
        choices: [all("route"), ...lists.routes.map((one) => ({ value: one.name, label: one.name }))],
      };
    }
    if (scope === "provider") {
      return {
        noun: "Provider",
        choices: [
          all("provider"),
          ...lists.providers.map((one) => ({ value: one.name, label: one.name })),
        ],
      };
    }
    return {
      noun: "Key",
      choices: [
        all("key"),
        ...lists.keys
          .filter((one) => one.status !== "revoked")
          .map((one) => ({ value: String(one.id), label: one.name })),
      ],
    };
  };
  const modelChoices = (provider: string): Choice[] => {
    const names = lists.models
      .filter((one) => provider === ANY || one.provider_name === provider)
      .map((one) => one.name);
    return [
      { value: ANY, label: "Any model" },
      ...[...new Set(names)].map((name) => ({ value: name, label: name })),
    ];
  };

  return (
    <form
      ref={formRef}
      aria-label={rule === null ? "Add rule" : "Edit rule"}
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="name">
        {(field) => (
          <Field label="Name" name={field.name} required error={failure.fieldError(field.name)}>
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
      {rule === null ? (
        <form.Field name="kind">
          {(field) => (
            <Field group label="Kind" name={field.name} error={failure.fieldError(field.name)}>
              {({ id, name, ...described }) => (
                <RadioGroup
                  {...described}
                  id={id}
                  name={name}
                  value={field.state.value}
                  onValueChange={(next) => {
                    // The percent that was not typed follows the kind.
                    if (form.getFieldValue("percent") === DEFAULT_PERCENT[field.state.value]) {
                      form.setFieldValue("percent", DEFAULT_PERCENT[next] ?? "");
                    }
                    field.handleChange(next);
                  }}
                >
                  {KINDS.map(([value, label]) => (
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
      ) : (
        <p className="flex flex-col gap-1 text-sm">
          <span className="text-muted-foreground">Kind</span>
          <span>{kindText(rule.kind)}</span>
        </p>
      )}
      <form.Subscribe selector={(state) => state.values.kind}>
        {(kind) =>
          kind === "budget" ? (
            <>
              <form.Field name="budget_id">
                {(field) => (
                  <Choose
                    label="Budget"
                    name={field.name}
                    value={field.state.value}
                    stored={storedOf(was?.budget_id, (id) => `Budget ${id}`)}
                    choices={budgetChoices}
                    onChange={field.handleChange}
                    hint="Fires once per budget period, when the spend reaches the percent."
                    error={failure.fieldError(field.name)}
                  />
                )}
              </form.Field>
              <form.Field name="percent">
                {(field) => (
                  <PercentField
                    label="Percent of the budget"
                    name={field.name}
                    value={field.state.value}
                    onChange={field.handleChange}
                    onBlur={field.handleBlur}
                    error={failure.fieldError(field.name)}
                  />
                )}
              </form.Field>
            </>
          ) : kind === "error_rate" ? (
            <>
              <form.Field name="scope">
                {(field) => (
                  <Choose
                    label="Scope"
                    name={field.name}
                    value={field.state.value}
                    choices={SCOPES.map(([value, label]) => ({ value, label }))}
                    onChange={(next) => {
                      field.handleChange(next);
                      form.setFieldValue("subject", ANY);
                    }}
                    error={failure.fieldError(field.name)}
                  />
                )}
              </form.Field>
              <form.Subscribe selector={(state) => state.values.scope}>
                {(scope) =>
                  scope === "gateway" ? null : (
                    <form.Field name="subject">
                      {(field) => {
                        const { noun, choices } = subjectChoices(scope);
                        return (
                          <Choose
                            label={noun}
                            name={field.name}
                            value={field.state.value}
                            stored={storedOf(
                              was?.scope === scope ? was.subject : undefined,
                              scope === "key" ? keyName : (name) => name,
                            )}
                            choices={choices}
                            onChange={field.handleChange}
                            error={failure.fieldError(field.name)}
                          />
                        );
                      }}
                    </form.Field>
                  )
                }
              </form.Subscribe>
              <form.Field name="percent">
                {(field) => (
                  <PercentField
                    label="Error rate (percent)"
                    name={field.name}
                    value={field.state.value}
                    hint="From 1 to 100. Counts server errors and rate limits from the provider."
                    onChange={field.handleChange}
                    onBlur={field.handleBlur}
                    error={failure.fieldError(field.name)}
                  />
                )}
              </form.Field>
              <form.Field name="window_minutes">
                {(field) => (
                  <PercentField
                    label="Window (minutes)"
                    name={field.name}
                    value={field.state.value}
                    hint="From 5 to 60."
                    onChange={field.handleChange}
                    onBlur={field.handleBlur}
                    error={failure.fieldError(field.name)}
                  />
                )}
              </form.Field>
              <form.Field name="min_requests">
                {(field) => (
                  <PercentField
                    label="Minimum calls in the window"
                    name={field.name}
                    value={field.state.value}
                    hint="From 1 to 100000. With fewer calls the rule never fires."
                    onChange={field.handleChange}
                    onBlur={field.handleBlur}
                    error={failure.fieldError(field.name)}
                  />
                )}
              </form.Field>
              <p className="text-sm text-muted-foreground">{ERROR_RATE_HELP}</p>
            </>
          ) : (
            <>
              <form.Field name="provider">
                {(field) => (
                  <Choose
                    label="Provider"
                    name={field.name}
                    value={field.state.value}
                    stored={storedOf(was?.provider, (name) => name)}
                    choices={[
                      { value: ANY, label: "Any provider" },
                      ...lists.providers.map((one) => ({ value: one.name, label: one.name })),
                    ]}
                    onChange={field.handleChange}
                    hint="Fires when the breaker of a target opens, and resolves when it closes."
                    error={failure.fieldError(field.name)}
                  />
                )}
              </form.Field>
              <form.Subscribe selector={(state) => state.values.provider}>
                {(rawProvider) => (
                  <form.Field name="model">
                    {(field) => {
                      const choices = modelChoices(
                        rawProvider === was?.provider || offered.providers.includes(rawProvider)
                          ? rawProvider
                          : ANY,
                      );
                      return (
                        <Choose
                          label="Model"
                          name={field.name}
                          value={field.state.value}
                          stored={storedOf(
                            rawProvider === was?.provider ? was.model : undefined,
                            (name) => name,
                          )}
                          choices={choices}
                          onChange={field.handleChange}
                          error={failure.fieldError(field.name)}
                        />
                      );
                    }}
                  </form.Field>
                )}
              </form.Subscribe>
            </>
          )
        }
      </form.Subscribe>
      <form.Field name="channel_ids">
        {(field) => (
          <Field
            group
            label="Channels"
            name={field.name}
            hint={CHANNELS_HINT}
            error={failure.fieldError(field.name)}
          >
            {(wiring) => (
              <Checks
                wiring={wiring}
                items={lists.channels}
                error={null}
                retry={() => undefined}
                loading="Loading the channels"
                none="No channels yet. Add one in the Channels view."
                checked={field.state.value.filter((id) =>
                  offered.channels.some((one) => String(one) === id),
                )}
                onChange={field.handleChange}
                label={(channel) => channel.name}
              />
            )}
          </Field>
        )}
      </form.Field>
      <FormDialogFooter
        running={running}
        submit={rule === null ? "Add rule" : "Save rule"}
        submitting={rule === null ? "Adding the rule" : "Saving the rule"}
        onCancel={onCancel}
      />
    </form>
  );
}

interface PercentFieldProps {
  label: string;
  name: string;
  value: string;
  hint?: string;
  onChange: (value: string) => void;
  onBlur: () => void;
  error: string | undefined;
}

/** A whole number, typed. */
function PercentField({
  label,
  name,
  value,
  hint = PERCENT_HINT,
  onChange,
  onBlur,
  error,
}: PercentFieldProps) {
  return (
    <Field label={label} name={name} hint={hint} error={error}>
      {({ id, name: fieldName, ...described }) => (
        <Input
          {...described}
          id={id}
          name={fieldName}
          inputMode="numeric"
          autoComplete="off"
          className={control}
          value={value}
          onBlur={onBlur}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
      )}
    </Field>
  );
}

/** The lists the form offers; the form shows when they are there. */
function RuleForm(props: Omit<FormProps, "lists">) {
  const budgets = useBudgets();
  const routes = useRoutes();
  const providers = useProviders();
  const keys = useKeys();
  const models = useModels();
  const channels = useAlertChannels();
  const queries = [budgets, routes, providers, keys, models, channels];
  const lists = useMemo(
    (): Lists | null =>
      budgets.data === undefined ||
      routes.data === undefined ||
      providers.data === undefined ||
      keys.data === undefined ||
      models.data === undefined ||
      channels.data === undefined
        ? null
        : {
            budgets: budgets.data.budgets,
            routes: routes.data.routes,
            providers: providers.data.providers,
            keys: keys.data.keys,
            models: models.data.models,
            channels: channels.data.channels,
          },
    [budgets.data, routes.data, providers.data, keys.data, models.data, channels.data],
  );
  if (lists !== null) return <RuleFormBody {...props} lists={lists} />;
  const failed = queries.find((query) => query.error !== null);
  if (failed?.error != null) {
    return (
      <ErrorState
        error={failed.error}
        onRetry={() => {
          for (const query of queries) if (query.error !== null) void query.refetch();
        }}
      />
    );
  }
  return (
    <div
      role="status"
      aria-busy="true"
      aria-label="Loading the rule form"
      className="flex flex-col gap-3"
    >
      <Skeleton className="h-4 w-48" />
      <Skeleton className="h-8 w-full" />
      <Skeleton className="h-8 w-full" />
    </div>
  );
}

interface DialogProps extends Omit<FormProps, "lists"> {
  open: boolean;
}

export function RuleDialog({ open, ...form }: DialogProps) {
  return (
    <FormDialog
      open={open}
      running={form.create.isPending || form.update.isPending}
      title={form.rule === null ? "Add rule" : "Edit rule"}
      description={RULE_DESCRIPTION}
      onCancel={form.onCancel}
    >
      <RuleForm {...form} />
    </FormDialog>
  );
}
