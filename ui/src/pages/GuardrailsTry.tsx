import { useId, useState } from "react";
import { useTestGuardrail } from "@/api/queries";
import { ApiError, messageOfError } from "@/api/errors";
import type { components } from "@/api/schema";
import { control, selectList } from "@/components/classes";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { fieldProblem, segmentsOf, verdictOf } from "@/lib/guardrails";

type Guardrail = components["schemas"]["GuardrailView"];
type RuleSpec = components["schemas"]["RuleSpec"];
type Direction = components["schemas"]["Direction"];
type Result = components["schemas"]["GuardrailTestResult"];

export const TRY_HINT =
  "Nothing is stored or logged. The text is checked as a call's input or output would be.";
export const EXTERNAL_WARNING =
  "This sends the text to the guardrail's URL, signed with its secret, as a call would.";
export const SAVE_FIRST = "Save the guardrail to try it: the gateway calls a saved URL only.";
export const NEEDS_URL = "This guardrail has no URL yet. Set one and save to try it.";
export const FIX_RULES = "Fix the rules marked above to try them.";

interface TryProps {
  /** The guardrail as saved, or `null` for a new one. */
  guardrail: Guardrail | null;
  kind: "rules" | "external";
  /** The rules as the form has them now; `null` while a rule is incomplete. */
  rules: RuleSpec[] | null;
}

/**
 * The faults of a test the gateway names, in words: what it says of single
 * fields, each in its own sentence with no field path, or its message when it
 * names none ("They are marked below" would point at nothing here).
 */
function problemText(error: unknown): string | null {
  const message = messageOfError(error);
  if (message === null) return null;
  if (!(error instanceof ApiError)) return message;
  const sentence = (text: string) => (/[.!?]$/.test(text) ? text : `${text}.`);
  const fields = Object.entries(error.fields).map(([name, text]) =>
    sentence(fieldProblem(name, text)),
  );
  return fields.length === 0 ? message : fields.join(" ");
}

/**
 * Tries the guardrail on a text typed here. Rules are tried as the form has
 * them, saved or not; an external guardrail is called for real, only when it
 * is saved. The redacted text is shown with its placeholders marked.
 */
export function Try({ guardrail, kind, rules }: TryProps) {
  const test = useTestGuardrail();
  const base = useId();
  const [direction, setDirection] = useState<Direction>("input");
  const [text, setText] = useState("");
  const [result, setResult] = useState<Result | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  const external = kind === "external";
  const reason = external
    ? guardrail === null || guardrail.kind !== "external"
      ? SAVE_FIRST
      : guardrail.url_host === ""
        ? NEEDS_URL
        : null
    : rules === null
      ? FIX_RULES
      : null;
  const empty = text.trim() === "";

  async function send() {
    setProblem(null);
    setResult(null);
    try {
      const answer = await test.mutateAsync(
        external
          ? { direction, text, guardrail_id: guardrail?.id ?? 0, call_external: true }
          : { direction, text, rules: rules ?? [] },
      );
      setResult(answer);
    } catch (error) {
      setProblem(problemText(error));
    } finally {
      // The text typed here is nowhere else once the answer is shown.
      test.reset();
    }
  }

  return (
    <section aria-labelledby={`${base}-title`} className="flex flex-col gap-3 rounded-lg border p-4">
      <h2 id={`${base}-title`} className="text-base font-medium">
        Try it
      </h2>
      <p className="text-sm text-muted-foreground">{external ? EXTERNAL_WARNING : TRY_HINT}</p>
      <div className="flex flex-col gap-2">
        <Label htmlFor={`${base}-direction`}>Check as</Label>
        <Select
          value={direction}
          onValueChange={(next) => {
            setDirection(next === "output" ? "output" : "input");
          }}
        >
          <SelectTrigger id={`${base}-direction`} className={`${control} w-full sm:w-64`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent className={selectList}>
            <SelectItem value="input">Input of a call</SelectItem>
            <SelectItem value="output">Output of a call</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor={`${base}-text`}>Text to check</Label>
        <Textarea
          id={`${base}-text`}
          autoComplete="off"
          spellCheck={false}
          className="min-h-24"
          value={text}
          onChange={(event) => {
            setText(event.target.value);
          }}
        />
      </div>
      {reason === null ? null : <p className="text-sm text-muted-foreground">{reason}</p>}
      <div>
        <Button
          type="button"
          variant="outline"
          className={control}
          disabled={test.isPending || empty || reason !== null}
          onClick={() => {
            void send();
          }}
        >
          {test.isPending ? "Checking" : external ? "Send to the guardrail" : "Check the text"}
        </Button>
      </div>
      <div role="status" className="not-empty:contents">
        {problem === null ? null : (
          <Alert variant="destructive">
            <AlertDescription>
              <p>{problem}</p>
            </AlertDescription>
          </Alert>
        )}
        {result === null ? null : (
          <div className="flex flex-col gap-2">
            <ul aria-label="What was found" className="list-disc pl-5 text-sm">
              {verdictOf(result.outcome).map((line) => (
                <li key={line}>{line}</li>
              ))}
            </ul>
            <div role="group" aria-labelledby={`${base}-after`} className="flex flex-col gap-2">
              <p id={`${base}-after`} className="text-sm font-medium">
                Text after the guardrail
              </p>
              <pre className="rounded-md bg-muted p-3 font-mono text-xs wrap-anywhere whitespace-pre-wrap">
                {segmentsOf(result.redacted_text).map((part, index) =>
                  part.redacted ? (
                    <mark
                      key={index}
                      className="rounded-sm bg-primary/20 px-0.5 font-semibold text-foreground"
                    >
                      {part.text}
                    </mark>
                  ) : (
                    <span key={index}>{part.text}</span>
                  ),
                )}
              </pre>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
