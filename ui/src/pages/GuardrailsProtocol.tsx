import { control } from "@/components/classes";
import { VerifySignatures } from "@/components/VerifySignatures";
import { ENDPOINTS } from "@/lib/usage";

const block =
  "rounded-md bg-muted p-3 font-mono text-xs wrap-anywhere whitespace-pre-wrap text-foreground";

/** What `endpoint` can be: every endpoint a call is logged under, and the test of a guardrail. */
export const ENDPOINT_NAMES: readonly string[] = [...ENDPOINTS.map(([value]) => value), "test"];

const REQUEST = `{
  "version": 1,
  "direction": "input",
  "endpoint": "chat",
  "model": "support-chat",
  "texts": ["first text", "second text"],
  "route": "support-chat",
  "key_id": 7,
  "team_id": 2,
  "user_id": 3
}`;

const ANSWERS = `{"action": "allow"}
{"action": "block"}
{"action": "redact", "texts": ["first text, changed", "second text"]}`;

/** What an external guardrail receives and answers, and how it checks the signature. */
export function Protocol() {
  return (
    <>
      <details className="rounded-lg border bg-card px-4 text-card-foreground">
        <summary className={`${control} flex cursor-pointer items-center font-medium`}>
          How external guardrails work
        </summary>
        <div className="flex flex-col gap-3 pb-4 text-sm">
          <p>
            The gateway posts JSON to the guardrail&apos;s URL, signed like an alert delivery.
            Images, keys and credentials are never sent: only the texts and these details.
          </p>
          <pre className={block}>
            <code>{REQUEST}</code>
          </pre>
          <p>
            <code className="font-mono">endpoint</code> is{" "}
            {ENDPOINT_NAMES.map((name, at) => (
              <span key={name}>
                {at === ENDPOINT_NAMES.length - 1 ? "or " : ""}
                <code className="font-mono">{name}</code>
                {at < ENDPOINT_NAMES.length - 1 ? ", " : ""}
              </span>
            ))}
            . Answer with status 200 to 299 and JSON of
            at most 1 MiB, one of:
          </p>
          <pre className={block}>
            <code>{ANSWERS}</code>
          </pre>
          <p>
            A redact answer has one replacement for each text, in order. A timeout, a connection
            error, another status, a redirect, an answer that is too big or not in this form is a
            failure: the call is let through and flagged when the guardrail fails open, and blocked
            when it fails closed. Built-in rules run before external guardrails, whatever the
            order; an external check of an answer holds the whole stream until it is checked.
          </p>
        </div>
      </details>
      <VerifySignatures subject="guardrail" unit="request" />
    </>
  );
}
