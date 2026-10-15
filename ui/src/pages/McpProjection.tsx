import { useForm } from "@tanstack/react-form";
import { useRef, useState } from "react";
import type { usePreviewMcpProjection, usePutMcpProjection } from "@/api/queries";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { messageOfError } from "@/api/errors";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  lastResultOf,
  projectionOf,
  projectionValues,
  sampleOf,
} from "@/lib/mcp";
import { blocksOf } from "@/lib/playgroundMcp";

type Tool = components["schemas"]["McpToolView"];

export const KEEP_HINT =
  "JSONPath queries, one on each line, of what to keep: $.items[*].id. Empty keeps everything.";
export const DROP_HINT = "JSONPath queries of what to remove after that: $..body";
export const SAMPLE_HINT =
  "A result of the tool as tools/call returns it. The last one you ran in the playground is filled in.";

interface Props {
  tool: Tool;
  /** The name of the tool's server, to find the last result of the playground. */
  server: string;
  put: ReturnType<typeof usePutMcpProjection>;
  preview: ReturnType<typeof usePreviewMcpProjection>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts from the projection as it is.
function ProjectionForm({ tool, server, put, preview, onDone, onCancel }: Props) {
  const last = lastResultOf(server, tool.name);
  const [sample, setSample] = useState(last === undefined ? "" : JSON.stringify(last, null, 2));
  const [shown, setShown] = useState<{ text: string; before: number; after: number; cut: boolean } | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const form = useForm({
    defaultValues: projectionValues(tool),
    onSubmit: async ({ value }) => {
      try {
        await put.mutateAsync({ id: tool.id, body: projectionOf(value) });
        onDone();
      } catch (error) {
        applyApiError(form, error);
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const onSubmit = useSubmit(form);

  async function tryIt() {
    setProblem(null);
    const checked = sampleOf(sample);
    if ("error" in checked) {
      setProblem(checked.error);
      setShown(null);
      return;
    }
    try {
      const answer = await preview.mutateAsync({
        id: tool.id,
        body: {
          result: checked.value as never,
          projection: projectionOf(form.state.values),
        },
      });
      const blocks = blocksOf(answer.result);
      const text = blocks.map((b) => (b.kind === "text" ? b.text : b.label)).join("\n");
      setShown({ text, before: answer.bytes_before, after: answer.bytes_after, cut: answer.truncated });
    } catch (error) {
      setShown(null);
      setProblem(messageOfError(error));
    }
  }

  const text = (name: "keep" | "drop", label: string, hint: string) => (
    <form.Field name={name}>
      {(field) => (
        <Field label={label} name={field.name} hint={hint} error={failure.fieldError(field.name)}>
          <Textarea
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            className="font-mono"
            value={field.state.value}
            onBlur={field.handleBlur}
            onChange={(event) => {
              field.handleChange(event.target.value);
            }}
          />
        </Field>
      )}
    </form.Field>
  );
  const number = (name: "max_items" | "max_bytes", label: string, hint: string) => (
    <form.Field name={name}>
      {(field) => (
        <Field label={label} name={field.name} hint={hint} error={failure.fieldError(field.name)}>
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
  );

  return (
    <form
      ref={formRef}
      aria-label="Edit projection"
      noValidate
      className="flex min-w-0 flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      {text("keep", "Keep", KEEP_HINT)}
      {text("drop", "Drop", DROP_HINT)}
      {number("max_items", "Most items in a list", "Longer lists are cut. Empty: no limit.")}
      {number("max_bytes", "Most bytes of text", "Longer text is cut and says so. Empty: no limit.")}
      <Field label="Sample result" name="sample" hint={SAMPLE_HINT}>
        {({ id, name, ...described }) => (
          <Textarea
            {...described}
            id={id}
            name={name}
            className="font-mono"
            spellCheck={false}
            value={sample}
            onChange={(event) => {
              setSample(event.target.value);
            }}
          />
        )}
      </Field>
      <div>
        <Button
          type="button"
          variant="outline"
          className={control}
          disabled={preview.isPending}
          onClick={() => {
            void tryIt();
          }}
        >
          Try it on the sample
        </Button>
      </div>
      {problem === null ? null : (
        <Alert variant="destructive">
          <AlertDescription>
            <p>{problem}</p>
          </AlertDescription>
        </Alert>
      )}
      {shown === null ? null : (
        <section aria-label="Preview" className="flex flex-col gap-2">
          <p className="text-sm text-muted-foreground">
            {`${String(shown.before)} bytes become ${String(shown.after)}.${shown.cut ? " Something was cut." : ""}`}
          </p>
          <pre className="max-h-60 overflow-auto rounded-md border bg-muted p-3 font-mono text-xs whitespace-pre-wrap break-all">
            {shown.text}
          </pre>
        </section>
      )}
      <FormDialogFooter
        running={put.isPending}
        submit="Save projection"
        submitting="Saving"
        onCancel={onCancel}
      />
    </form>
  );
}

/** Sets what of a tool's result the agent is given. */
export function ProjectionDialog({ open, ...form }: Props & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.put.isPending}
      title="Edit projection"
      description="What of the result of this tool the agent is given. It is cut before the guardrails read it. Saving with every field empty takes the projection off."
      onCancel={form.onCancel}
    >
      <ProjectionForm {...form} />
    </FormDialog>
  );
}
