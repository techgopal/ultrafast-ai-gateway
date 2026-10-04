import { formatDollars } from "@/lib/money";
import type { Message, Usage } from "@/lib/playground";
import { formatTokens } from "@/lib/usage";

const WHO = { user: "You", assistant: "Assistant" } as const;

function Turn({ message, streaming }: { message: Message; streaming?: boolean }) {
  return (
    <li
      className={`flex flex-col gap-1 rounded-lg border p-3 ${message.role === "user" ? "bg-muted/40" : ""}`}
      data-role={message.role}
      aria-busy={streaming === true ? true : undefined}
    >
      <span className="text-xs font-medium text-muted-foreground">{WHO[message.role]}</span>
      <p className="break-words whitespace-pre-wrap">{message.content}</p>
    </li>
  );
}

interface ThreadProps {
  messages: readonly Message[];
  /** The answer that is on its way. */
  partial: string;
  running: boolean;
}

/** The conversation so far, and the answer as it arrives. */
export function Thread({ messages, partial, running }: ThreadProps) {
  if (messages.length === 0 && !running) {
    return <p className="text-sm text-muted-foreground">Nothing has been said yet. Write a message below.</p>;
  }
  return (
    <ol aria-label="Conversation" className="flex flex-col gap-3">
      {messages.map((message, index) => (
        <Turn key={index} message={message} />
      ))}
      {running ? (
        partial === "" ? (
          <li role="status" className="text-sm text-muted-foreground">
            Waiting for the answer
          </li>
        ) : (
          <Turn message={{ role: "assistant", content: partial }} streaming />
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
