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
import {
  checkParams,
  checkTools,
  costMicros,
  curlOf,
  type Attachment,
  type Message,
  type Params,
  type Prices,
  type ToolChoice,
} from "@/lib/playground";
import { bodyOf, useRun, type Call } from "@/pages/PlaygroundRun";
import { pendingCalls, RESULT_MISSING, Thread, UsageLine } from "@/pages/PlaygroundThread";

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

export const IMAGE_TOO_BIG = "Images over 5 MB are not sent.";
export const IMAGE_KIND = "Only PNG, JPEG, GIF and WebP images are sent.";
export const IMAGE_UNREADABLE = "That image could not be read.";
export const WAITING_FOR_RESULTS = "Send the results of the tool calls above to go on.";
const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
const MAX_IMAGE_BYTES = 5 * 1024 * 1024;

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((done, fail) => {
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string") done(reader.result);
      else fail(new Error(IMAGE_UNREADABLE));
    };
    reader.onerror = () => {
      fail(new Error(IMAGE_UNREADABLE));
    };
    reader.readAsDataURL(file);
  });
}

/** The choices of a tool choice: `required` and a named function only when there are tools. */
function toolChoices(names: readonly string[], defined: boolean): Choice[] {
  return [
    { value: "auto", label: "auto" },
    { value: "none", label: "none" },
    ...(defined
      ? [
          { value: "required", label: "required" },
          ...names.map((name) => ({ value: `fn:${name}`, label: name })),
        ]
      : []),
  ];
}

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
  const [images, setImages] = useState<readonly Attachment[]>([]);
  const [imageError, setImageError] = useState<string | null>(null);
  const [toolsOpen, setToolsOpen] = useState(false);
  const [toolsText, setToolsText] = useState("");
  const [toolsError, setToolsError] = useState<string | undefined>(undefined);
  const [toolChoice, setToolChoice] = useState("auto");
  const [results, setResults] = useState<Readonly<Record<string, string>>>({});
  const [resultsError, setResultsError] = useState<string | null>(null);

  const tools = useMemo(() => checkTools(toolsText), [toolsText]);
  const offered = useMemo(() => toolChoices(tools.names, tools.tools !== undefined), [tools]);
  // A choice that is no longer offered is never sent.
  const effectiveChoice = offered.some((choice) => choice.value === toolChoice) ? toolChoice : "auto";
  const waiting = pendingCalls(run.messages);

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

  /** The call with `add`, or `null` when what is typed is not valid. */
  function callOf(add: readonly Message[]): Call | null {
    const checked = checkParams(params);
    setFieldErrors(checked.errors);
    const toolsChecked = checkTools(toolsText);
    setToolsError(toolsChecked.error);
    if (Object.keys(checked.errors).length > 0 || toolsChecked.error !== undefined) return null;
    return { model: target, system, add, values: { ...checked.values, ...toolValues(toolsChecked) } };
  }

  function toolValues(checked: ReturnType<typeof checkTools>): { tools?: NonNullable<typeof checked.tools>; tool_choice?: ToolChoice } {
    if (checked.tools === undefined) return {};
    if (effectiveChoice === "auto") return { tools: checked.tools };
    const choice: ToolChoice =
      effectiveChoice === "none" || effectiveChoice === "required"
        ? effectiveChoice
        : { type: "function", function: { name: effectiveChoice.slice(3) } };
    return { tools: checked.tools, tool_choice: choice };
  }

  const typed = text.trim();

  async function send() {
    if (target === "" || run.running || waiting.length > 0) return;
    if (typed === "" && images.length === 0) return;
    const kept = images;
    const call = callOf([{ role: "user", content: typed, ...(kept.length > 0 ? { images: kept } : {}) }]);
    if (call === null) return;
    setText("");
    setImages([]);
    const putBack = await run.send(call);
    if (putBack) {
      setText(typed);
      setImages(kept);
    }
    box.current?.focus();
  }

  function resultMessages(): Message[] {
    return waiting.map((call) => ({
      role: "tool" as const,
      content: (results[call.id] ?? "").trim(),
      tool_call_id: call.id,
    }));
  }

  async function sendResults() {
    if (target === "" || run.running || waiting.length === 0) return;
    const add = resultMessages();
    if (add.some((message) => message.content === "")) {
      setResultsError(RESULT_MISSING);
      return;
    }
    setResultsError(null);
    const call = callOf(add);
    if (call === null) return;
    const putBack = await run.send(call);
    if (!putBack) setResults({});
    box.current?.focus();
  }

  async function attach(files: FileList | null) {
    if (files === null) return;
    setImageError(null);
    const added: Attachment[] = [];
    for (const file of Array.from(files)) {
      if (!IMAGE_TYPES.includes(file.type)) {
        setImageError(IMAGE_KIND);
        continue;
      }
      if (file.size > MAX_IMAGE_BYTES) {
        setImageError(IMAGE_TOO_BIG);
        continue;
      }
      try {
        added.push({ name: file.name, url: await readAsDataUrl(file) });
      } catch {
        setImageError(IMAGE_UNREADABLE);
      }
    }
    if (added.length > 0) setImages((before) => [...before, ...added]);
  }

  async function copyText(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      toast(DONE.copied);
    } catch {
      toast(DONE.notCopied, "error");
    }
  }

  async function copyCurl() {
    const checked = checkParams(params);
    setFieldErrors(checked.errors);
    const toolsChecked = checkTools(toolsText);
    setToolsError(toolsChecked.error);
    if (Object.keys(checked.errors).length > 0 || toolsChecked.error !== undefined || target === "") return;
    const add: readonly Message[] =
      waiting.length > 0
        ? resultMessages()
        : [{ role: "user", content: typed === "" && images.length === 0 ? CURL_SAMPLE : typed, ...(images.length > 0 ? { images } : {}) }];
    const call: Call = { model: target, system, add, values: { ...checked.values, ...toolValues(toolsChecked) } };
    await copyText(curlOf(window.location.origin, bodyOf(call, run.messages)));
  }

  const cost =
    run.finished === null
      ? null
      : costMicros(run.finished.usage, pricesOf(run.finished.called, run.finished.model, models));

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
      <section aria-label="Chat" className="flex min-w-0 flex-col gap-4">
        <Thread
          messages={run.messages}
          partial={run.partial}
          running={run.running}
          onCopy={(value) => void copyText(value)}
          results={{
            values: results,
            error: resultsError,
            onChange: (id, value) => {
              setResults((before) => ({ ...before, [id]: value }));
            },
            onSend: () => void sendResults(),
          }}
        />
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
          {images.length === 0 ? null : (
            <ul aria-label="Attached images" className="flex flex-wrap gap-2">
              {images.map((image, index) => (
                <li key={index} className="flex items-center gap-2 rounded-md border p-1">
                  <img src={image.url} alt={image.name} className="size-12 rounded object-cover" />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className={control}
                    aria-label={`Remove ${image.name}`}
                    onClick={() => {
                      setImages((before) => before.filter((_, at) => at !== index));
                    }}
                  >
                    Remove
                  </Button>
                </li>
              ))}
            </ul>
          )}
          {imageError === null ? null : (
            <p role="alert" className="text-sm text-destructive">
              {imageError}
            </p>
          )}
          {waiting.length > 0 && !run.running ? (
            <p className="text-sm text-muted-foreground">{WAITING_FOR_RESULTS}</p>
          ) : null}
          <div className="flex flex-wrap gap-2">
            {/*
              Two buttons that are never one element, whatever React could
              reuse: a click on Stop that ends the call changes the page
              before the click is over, and a Stop that had become a submit
              button would send the message again.
            */}
            {run.running ? (
              <Button key="stop" type="button" variant="outline" className={control} onClick={run.stop}>
                Stop
              </Button>
            ) : (
              <Button key="send" type="submit" className={control} disabled={target === "" || (typed === "" && images.length === 0) || waiting.length > 0}
              >
                Send
              </Button>
            )}
            <Button
              type="button"
              variant="outline"
              className={control}
              disabled={run.running || (run.messages.length === 0 && run.error === null)}
              onClick={() => {
                run.clear();
                setResults({});
                setResultsError(null);
              }}
            >
              New conversation
            </Button>
            <label
              className={`${control} inline-flex cursor-pointer items-center justify-center rounded-md border bg-background px-3 text-sm font-medium shadow-xs hover:bg-accent focus-within:ring-[3px] focus-within:ring-ring/50`}
            >
              Attach image
              <input
                type="file"
                accept="image/png,image/jpeg,image/gif,image/webp"
                multiple
                className="sr-only"
                onChange={(event) => {
                  void attach(event.target.files);
                  event.target.value = "";
                }}
              />
            </label>
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
        <div className="flex flex-col gap-2">
          <Button
            type="button"
            variant="outline"
            className={control}
            aria-expanded={toolsOpen}
            aria-controls="tools-section"
            onClick={() => {
              setToolsOpen((before) => !before);
            }}
          >
            Tools
          </Button>
          <div id="tools-section" hidden={!toolsOpen} className="flex flex-col gap-4">
            <Field
              label="Tools"
              name="tools"
              hint="A JSON array of functions, as in the OpenAI API. Empty: no tools."
              error={toolsError}
            >
              {({ id, name, ...described }) => (
                <Textarea
                  {...described}
                  id={id}
                  name={name}
                  className="font-mono text-xs"
                  spellCheck={false}
                  value={toolsText}
                  onChange={(event) => {
                    setToolsText(event.target.value);
                  }}
                />
              )}
            </Field>
            <div className="flex flex-col gap-2">
              <p className="text-sm font-medium">Tool choice</p>
              <FilterSelect
                label="Tool choice"
                value={effectiveChoice}
                choices={offered}
                onChange={setToolChoice}
              />
            </div>
          </div>
        </div>
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
