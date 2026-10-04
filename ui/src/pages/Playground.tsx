import { useMemo, useRef, useState } from "react";
import { useModels, useRoutes } from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control } from "@/components/classes";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { Field } from "@/components/Field";
import { FilterSelect, type Choice } from "@/components/FilterSelect";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { PageHeader } from "@/components/PageHeader";
import { useToast } from "@/components/toast";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { refOf } from "@/lib/models";
import { checkParams, costMicros, curlOf, type Params, type Prices } from "@/lib/playground";
import { bodyOf, useRun, type Call } from "@/pages/PlaygroundRun";
import { Thread, UsageLine } from "@/pages/PlaygroundThread";

type Model = components["schemas"]["ModelView"];

export const DONE = {
  copied: "Copied.",
  notCopied: "Could not copy. Select the command and copy it by hand.",
} as const;

export const NOTHING_TO_CALL = {
  title: "Nothing to call",
  description:
    "No model or route is open to you. Ask an admin to give you access to a model.",
} as const;

export const CURL_SAMPLE = "Hello";

const empty: Params = { maxTokens: "", temperature: "", topP: "", stop: "" };

/** The price of the model that answered: the one called, or for a route the only model of that name. */
function pricesOf(target: string, answered: string | null, models: readonly Model[]): Prices | null {
  const direct = models.find((model) => refOf(model) === target);
  if (direct !== undefined) return direct;
  if (answered === null) return null;
  const named = models.filter((model) => model.name === answered);
  return named.length === 1 ? (named[0] ?? null) : null;
}

function PlaygroundOf({ models, routes }: { models: readonly Model[]; routes: readonly string[] }) {
  const run = useRun();
  const toast = useToast();
  const [chosen, setChosen] = useState("");
  const [system, setSystem] = useState("");
  const [params, setParams] = useState<Params>(empty);
  const [fieldErrors, setFieldErrors] = useState<ReturnType<typeof checkParams>["errors"]>({});
  const [text, setText] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);

  const choices = useMemo<Choice[]>(
    () => [
      ...models
        .filter((model) => model.enabled)
        .map((model) => ({ value: refOf(model), label: refOf(model) }))
        .sort((a, b) => a.label.localeCompare(b.label)),
      ...[...routes].sort().map((name) => ({ value: name, label: `${name} (route)` })),
    ],
    [models, routes],
  );
  // A choice that is no longer offered is never sent.
  const target = choices.some((choice) => choice.value === chosen)
    ? chosen
    : (choices[0]?.value ?? "");

  function setParam(name: keyof Params, value: string) {
    setParams((before) => ({ ...before, [name]: value }));
  }

  function callOf(): Call | null {
    const checked = checkParams(params);
    setFieldErrors(checked.errors);
    if (Object.keys(checked.errors).length > 0) return null;
    return { model: target, system, text: text.trim(), values: checked.values };
  }

  async function send() {
    const call = callOf();
    if (call === null || call.text === "" || target === "" || run.running) return;
    setText("");
    const putBack = await run.send(call);
    if (putBack !== null) setText(putBack);
    box.current?.focus();
  }

  async function copyCurl() {
    const checked = checkParams(params);
    setFieldErrors(checked.errors);
    if (Object.keys(checked.errors).length > 0 || target === "") return;
    const call: Call = {
      model: target,
      system,
      text: text.trim() === "" ? CURL_SAMPLE : text.trim(),
      values: checked.values,
    };
    try {
      await navigator.clipboard.writeText(curlOf(window.location.origin, bodyOf(call, run.messages)));
      toast(DONE.copied);
    } catch {
      toast(DONE.notCopied, "error");
    }
  }

  const cost =
    run.finished === null
      ? null
      : costMicros(run.finished.usage, pricesOf(target, run.finished.model, models));

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
      <section aria-label="Chat" className="flex min-w-0 flex-col gap-4">
        <Thread messages={run.messages} partial={run.partial} running={run.running} />
        {run.error === null ? null : (
          <Alert variant="destructive">
            <AlertDescription>
              <p>{run.error}</p>
            </AlertDescription>
          </Alert>
        )}
        {run.finished === null || run.running ? null : <UsageLine usage={run.finished.usage} cost={cost} />}
        <form
          aria-label="Message"
          noValidate
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            void send();
          }}
        >
          <Field label="Message" name="message">
            {({ id, name, ...described }) => (
              <Textarea
                {...described}
                id={id}
                name={name}
                ref={box}
                value={text}
                onChange={(event) => {
                  setText(event.target.value);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                    event.preventDefault();
                    void send();
                  }
                }}
              />
            )}
          </Field>
          <div className="flex flex-wrap gap-2">
            {run.running ? (
              <Button type="button" variant="outline" className={control} onClick={run.stop}>
                Stop
              </Button>
            ) : (
              <Button type="submit" className={control} disabled={target === "" || text.trim() === ""}>
                Send
              </Button>
            )}
            <Button
              type="button"
              variant="outline"
              className={control}
              disabled={run.running || (run.messages.length === 0 && run.error === null)}
              onClick={run.clear}
            >
              New conversation
            </Button>
            <Button type="button" variant="outline" className={control} onClick={() => void copyCurl()}>
              Copy as curl
            </Button>
          </div>
        </form>
      </section>

      <section aria-labelledby="call-settings" className="flex min-w-0 flex-col gap-4">
        <h2 id="call-settings" className="text-base font-medium">
          Call settings
        </h2>
        <div className="flex flex-col gap-2">
          <p className="text-sm font-medium">Model or route</p>
          <FilterSelect label="Model or route" value={target} choices={choices} onChange={setChosen} />
          <p className="text-sm text-muted-foreground">Calls count toward your limits and budgets.</p>
        </div>
        <Field label="System prompt" name="system">
          {({ id, name, ...described }) => (
            <Textarea
              {...described}
              id={id}
              name={name}
              value={system}
              onChange={(event) => {
                setSystem(event.target.value);
              }}
            />
          )}
        </Field>
        {(
          [
            ["maxTokens", "Max tokens", "A whole number. Empty: the provider's default.", "numeric"],
            ["temperature", "Temperature", "From 0 to 2. Empty: the provider's default.", "decimal"],
            ["topP", "Top P", "From 0 to 1. Empty: the provider's default.", "decimal"],
            ["stop", "Stop sequences", "Up to 4, separated by commas.", "text"],
          ] as const
        ).map(([name, label, hint, mode]) => (
          <Field key={name} label={label} name={name} hint={hint} error={fieldErrors[name]}>
            {({ id, name: fieldName, ...described }) => (
              <Input
                {...described}
                id={id}
                name={fieldName}
                inputMode={mode}
                autoComplete="off"
                className={control}
                value={params[name]}
                onChange={(event) => {
                  setParam(name, event.target.value);
                }}
              />
            )}
          </Field>
        ))}
      </section>
    </div>
  );
}

function Loading() {
  return (
    <div role="status" aria-busy="true" aria-label="Loading the playground" className="flex flex-col gap-3">
      <Skeleton className="h-8 w-64" />
      <Skeleton className="h-32 w-full" />
    </div>
  );
}

function PlaygroundLoaded() {
  const models = useModels();
  const routes = useRoutes();
  const failure = models.error ?? routes.error;
  const title = <PageHeader title="Playground" subtitle="Talk to a model or a route as one of your own keys would. The calls are logged and counted like any call." />;
  if (models.data === undefined || routes.data === undefined) {
    return (
      <>
        {title}
        {failure === null ? (
          <Loading />
        ) : (
          <ErrorState
            error={failure}
            onRetry={() => {
              void models.refetch();
              void routes.refetch();
            }}
          />
        )}
      </>
    );
  }
  const names = routes.data.routes.map((route) => route.name);
  const callable = models.data.models.some((model) => model.enabled) || names.length > 0;
  return (
    <>
      {title}
      {callable ? (
        <PlaygroundOf models={models.data.models} routes={names} />
      ) : (
        <EmptyState {...NOTHING_TO_CALL} />
      )}
    </>
  );
}

/** Chat with a model or a route through the gateway, as the signed-in user. */
export function Playground() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  if (!can(session.me, { type: "usePlayground" })) return <NotAvailableContent />;
  return <PlaygroundLoaded />;
}
