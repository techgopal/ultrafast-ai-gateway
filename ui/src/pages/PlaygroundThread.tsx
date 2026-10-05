import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { formatDollars } from "@/lib/money";
import type { Message, ToolCall, Usage } from "@/lib/playground";
import { formatTokens } from "@/lib/usage";

const WHO = { user: "You", assistant: "Assistant", tool: "Tool result" } as const;

export const RESULT_MISSING = "Give a result for every call.";

/** The arguments of a call as they are shown: indented when they are JSON, as they came when not. */
export function prettyArguments(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

/** The calls the assistant has made that no message answers yet; empty when there are none. */
export function pendingCalls(messages: readonly Message[]): readonly ToolCall[] {
  const last = messages.at(-1);
  return last?.role === "assistant" ? (last.tool_calls ?? []) : [];
}

function nameOf(messages: readonly Message[], id: string): string | undefined {
  for (const message of messages) {
    if (message.role !== "assistant") continue;
    const call = message.tool_calls?.find((candidate) => candidate.id === id);
    if (call !== undefined) return call.function.name;
  }
  return undefined;
}

interface CallProps {
  call: ToolCall;
  onCopy: (text: string) => void;
  /** The field for the result of the call, while it is waited for. */
  children?: React.ReactNode;
}

function Call({ call, onCopy, children }: CallProps) {
  const text = prettyArguments(call.function.arguments);
  return (
    <div
      role="group"
      aria-label={`Tool call ${call.function.name}`}
      className="flex min-w-0 flex-col gap-2 rounded-md border bg-background p-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="font-mono text-sm break-all">{call.function.name}</span>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className={control}
          onClick={() => {
            onCopy(text);
          }}
        >
          Copy arguments
        </Button>
      </div>
      <pre className="max-w-full overflow-x-auto rounded bg-muted/40 p-2 font-mono text-xs whitespace-pre-wrap break-all">
        {text}
      </pre>
      {children}
    </div>
  );
}

interface TurnProps {
  message: Message;
  /** Every message of the thread, to name the call a result answers. */
  messages: readonly Message[];
  streaming?: boolean;
  onCopy: (text: string) => void;
  /** Fields for the results of the calls of this message, when they are waited for. */
  results?: ResultsProps | undefined;
}

function Turn({ message, messages, streaming, onCopy, results }: TurnProps) {
  const who =
    message.role === "tool"
      ? `Tool result: ${nameOf(messages, message.tool_call_id) ?? "call"}`
      : WHO[message.role];
  return (
    <li
      className={`flex min-w-0 flex-col gap-2 rounded-lg border p-3 ${message.role === "assistant" ? "" : "bg-muted/40"}`}
      data-role={message.role}
      aria-busy={streaming === true ? true : undefined}
    >
      <span className="text-xs font-medium text-muted-foreground">{who}</span>
      {message.role === "user" && message.images !== undefined && message.images.length > 0 ? (
        <div className="flex flex-wrap gap-2">
          {message.images.map((image, index) => (
            <img
              key={index}
              src={image.url}
              alt={image.name}
              className="size-24 rounded border object-cover"
            />
          ))}
        </div>
      ) : null}
      {message.content === null || message.content === "" ? null : (
        <p className="break-words whitespace-pre-wrap">{message.content}</p>
      )}
      {message.role === "assistant" && message.tool_calls !== undefined
        ? message.tool_calls.map((call) => (
            <Call key={call.id} call={call} onCopy={onCopy}>
              {results === undefined ? null : (
                <Field label="Tool result" name={`result-${call.id}`}>
                  {({ id, name, ...described }) => (
                    <Textarea
                      {...described}
                      id={id}
                      name={name}
                      value={results.values[call.id] ?? ""}
                      onChange={(event) => {
                        results.onChange(call.id, event.target.value);
                      }}
                    />
                  )}
                </Field>
              )}
            </Call>
          ))
        : null}
      {results === undefined ? null : (
        <div className="flex flex-col gap-2">
          {results.error === null ? null : (
            <p role="alert" className="text-sm text-destructive">
              {results.error}
            </p>
          )}
          <div>
            <Button type="button" className={control} onClick={results.onSend}>
              Send results
            </Button>
          </div>
        </div>
      )}
    </li>
  );
}

/** What the page keeps for the results the model waits for. */
export interface ResultsProps {
  values: Readonly<Record<string, string>>;
  error: string | null;
  onChange: (id: string, text: string) => void;
  onSend: () => void;
}

interface ThreadProps {
  messages: readonly Message[];
  /** The answer that is on its way. */
  partial: string;
  running: boolean;
  onCopy: (text: string) => void;
  results: ResultsProps;
}

/** The conversation so far, and the answer as it arrives. */
export function Thread({ messages, partial, running, onCopy, results }: ThreadProps) {
  if (messages.length === 0 && !running) {
    return <p className="text-sm text-muted-foreground">Nothing has been said yet. Write a message below.</p>;
  }
  const waiting = !running && pendingCalls(messages).length > 0;
  return (
    <ol aria-label="Conversation" className="flex flex-col gap-3">
      {messages.map((message, index) => (
        <Turn
          key={index}
          message={message}
          messages={messages}
          onCopy={onCopy}
          results={waiting && index === messages.length - 1 ? results : undefined}
        />
      ))}
      {running ? (
        partial === "" ? (
          <li role="status" className="text-sm text-muted-foreground">
            Waiting for the answer
          </li>
        ) : (
          <Turn message={{ role: "assistant", content: partial }} messages={messages} streaming onCopy={onCopy} />
        )
      ) : null}
    </ol>
  );
}

export const COST_UNKNOWN = "not known, the model has no price";

interface UsageLineProps {
  usage: Usage;
  /** `null` when the model, or its price, is not known. */
  cost: number | null;
}

/** What the last call used and cost. */
export function UsageLine({ usage, cost }: UsageLineProps) {
  return (
    <p className="text-sm text-muted-foreground">
      Tokens: {formatTokens(usage.input)} in, {formatTokens(usage.output)} out. Cost:{" "}
      {cost === null ? COST_UNKNOWN : formatDollars(cost)}.
    </p>
  );
}
