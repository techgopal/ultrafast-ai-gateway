// Images mode of the playground: a prompt, a size and a number of images, sent
// to `/api/playground/images`. Only images that come back as base64 are
// shown; a URL in the answer is never loaded by the browser.
import { useEffect, useMemo, useRef, useState } from "react";
import { playgroundImages } from "@/api/client";
import { failureText } from "@/pages/PlaygroundRun";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { FilterSelect, type Choice } from "@/components/FilterSelect";
import { useToast } from "@/components/toast";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { refOf } from "@/lib/models";
import {
  checkImageCount,
  costMicros,
  imageCurlOf,
  imageRequestBody,
  imageUrlsOf,
  type ImageSize,
  type Usage,
} from "@/lib/playground";
import { DONE } from "@/pages/Playground";
import { UsageLine } from "@/pages/PlaygroundThread";

type Model = components["schemas"]["ModelView"];

const SIZES: Choice[] = [
  { value: "1024x1024", label: "1024 x 1024" },
  { value: "1536x1024", label: "1536 x 1024 (landscape)" },
  { value: "1024x1536", label: "1024 x 1536 (portrait)" },
  { value: "default", label: "Provider default" },
];

interface Shown {
  urls: readonly string[];
  usage: Usage | null;
  called: string;
}

export function ImagesMode({ models, routes }: { models: readonly Model[]; routes: readonly string[] }) {
  const toast = useToast();
  const [chosen, setChosen] = useState("");
  const [prompt, setPrompt] = useState("");
  const [size, setSize] = useState<ImageSize>("1024x1024");
  const [count, setCount] = useState("1");
  const [countError, setCountError] = useState<string | undefined>(undefined);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [shown, setShown] = useState<Shown | null>(null);
  const controller = useRef<AbortController | null>(null);

  // A call that is on its way is ended with the page.
  useEffect(
    () => () => {
      controller.current?.abort();
    },
    [],
  );

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
  const target = choices.some((choice) => choice.value === chosen) ? chosen : (choices[0]?.value ?? "");
  const typed = prompt.trim();

  /** The body to send, or `null` when the number of images is not valid (the field says why). */
  function bodyOf() {
    const checked = checkImageCount(count);
    setCountError(checked.error);
    if (checked.n === undefined) return null;
    return imageRequestBody(target, typed, checked.n, size);
  }

  async function generate() {
    if (running || target === "" || typed === "") return;
    const body = bodyOf();
    if (body === null) return;
    const abort = new AbortController();
    controller.current = abort;
    setRunning(true);
    setError(null);
    try {
      const answer = await playgroundImages(body, abort.signal);
      const usage = answer.usage as { input_tokens?: unknown; output_tokens?: unknown } | undefined;
      setShown({
        urls: imageUrlsOf(answer),
        usage:
          typeof usage?.input_tokens === "number" && typeof usage.output_tokens === "number"
            ? { input: usage.input_tokens, output: usage.output_tokens }
            : null,
        called: target,
      });
    } catch (caught) {
      if (!abort.signal.aborted) {
        setShown(null);
        setError(failureText(caught));
      }
    }
    if (controller.current === abort) controller.current = null;
    setRunning(false);
  }

  async function copyCurl() {
    const body = bodyOf();
    if (body === null || target === "") return;
    const withPrompt = typed === "" ? { ...body, prompt: "A watercolor of a fox" } : body;
    try {
      await navigator.clipboard.writeText(imageCurlOf(window.location.origin, withPrompt));
      toast(DONE.copied);
    } catch {
      toast(DONE.notCopied, "error");
    }
  }

  const price = shown === null ? null : (models.find((model) => refOf(model) === shown.called) ?? null);
  const cost = shown?.usage == null ? null : costMicros(shown.usage, price);

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
      <section aria-label="Images" className="flex min-w-0 flex-col gap-4">
        <form
          aria-label="Image prompt"
          noValidate
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            void generate();
          }}
        >
          <Field label="Prompt" name="prompt">
            {({ id, name, ...described }) => (
              <Textarea
                {...described}
                id={id}
                name={name}
                value={prompt}
                onChange={(event) => {
                  setPrompt(event.target.value);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                    event.preventDefault();
                    void generate();
                  }
                }}
              />
            )}
          </Field>
          <div className="flex flex-wrap gap-2">
            <Button type="submit" className={control} disabled={target === "" || typed === "" || running}>
              Generate
            </Button>
            <Button type="button" variant="outline" className={control} onClick={() => void copyCurl()}>
              Copy as curl
            </Button>
          </div>
        </form>
        {error === null ? null : (
          <Alert variant="destructive">
            <AlertDescription>
              <p>{error}</p>
            </AlertDescription>
          </Alert>
        )}
        {running ? (
          <p role="status" className="text-sm text-muted-foreground">
            Generating...
          </p>
        ) : null}
        {shown === null || running ? null : (
          <>
            {shown.urls.length === 0 ? (
              <p className="text-sm text-muted-foreground">The answer holds no image to show here.</p>
            ) : (
              <ul aria-label="Generated images" className="grid gap-3 sm:grid-cols-2">
                {shown.urls.map((url, index) => (
                  <li key={index} className="rounded-md border p-1">
                    <img
                      src={url}
                      alt={`Generation ${String(index + 1)}`}
                      className="h-auto w-full rounded"
                    />
                  </li>
                ))}
              </ul>
            )}
            {shown.usage === null ? null : <UsageLine usage={shown.usage} cost={cost} />}
          </>
        )}
      </section>

      <section aria-labelledby="image-settings" className="flex min-w-0 flex-col gap-4">
        <h2 id="image-settings" className="text-base font-medium">
          Image settings
        </h2>
        <div className="flex flex-col gap-2">
          <p className="text-sm font-medium">Model or route</p>
          <FilterSelect label="Model or route" value={target} choices={choices} onChange={setChosen} />
          <p className="text-sm text-muted-foreground">
            Pick a model that draws images. Calls count toward your limits and budgets.
          </p>
        </div>
        <div className="flex flex-col gap-2">
          <p className="text-sm font-medium">Size</p>
          <FilterSelect
            label="Size"
            value={size}
            choices={SIZES}
            onChange={(value) => {
              setSize(value as ImageSize);
            }}
          />
        </div>
        <Field label="Number of images" name="n" hint="From 1 to 4." error={countError}>
          {({ id, name, ...described }) => (
            <Input
              {...described}
              id={id}
              name={name}
              inputMode="numeric"
              autoComplete="off"
              className={control}
              value={count}
              onChange={(event) => {
                setCount(event.target.value);
              }}
            />
          )}
        </Field>
      </section>
    </div>
  );
}
