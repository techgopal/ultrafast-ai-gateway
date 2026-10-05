// One conversation of the playground, kept in memory only: it is gone when
// the page is left. Each send is a streamed call to `/api/playground/chat`.
import { useCallback, useEffect, useRef, useState } from "react";
import { playgroundChat } from "@/api/client";
import { ApiError, messageOfError } from "@/api/errors";
import {
  chunkOf,
  requestBody,
  retryText,
  SseReader,
  ToolCallAssembler,
  type ChatRequestBody,
  type Message,
  type Usage,
} from "@/lib/playground";

export const NO_ANSWER = "The gateway sent no answer.";

export interface Finished {
  usage: Usage;
  /** The model or route the call was made with, as it was sent. */
  called: string;
  /** The model that answered, as the provider names it. */
  model: string | null;
}

/** What the user is told of a call that failed; `null` when there is nothing to tell. */
export function failureText(error: unknown): string | null {
  const message = messageOfError(error);
  if (message === null) return null;
  return error instanceof ApiError ? message + retryText(error.retryAfter) : message;
}

export interface Run {
  messages: readonly Message[];
  /** What the answer that is on its way has said so far. */
  partial: string;
  running: boolean;
  error: string | null;
  finished: Finished | null;
  /** Sends the call. Resolves with `true` when nothing came of it and what was added is taken back. */
  send: (call: Call) => Promise<boolean>;
  stop: () => void;
  clear: () => void;
}

export interface Call {
  model: string;
  system: string;
  /** What this call adds to the conversation: a message of the user, or the results of tool calls. */
  add: readonly Message[];
  values: Parameters<typeof requestBody>[3];
}

/** The body the next send would have: for the call itself and for "Copy as curl". */
export function bodyOf(call: Call, history: readonly Message[]): ChatRequestBody {
  return requestBody(call.model, call.system, [...history, ...call.add], call.values);
}

export function useRun(): Run {
  const [messages, setMessages] = useState<readonly Message[]>([]);
  const [partial, setPartial] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [finished, setFinished] = useState<Finished | null>(null);
  const controller = useRef<AbortController | null>(null);
  const history = useRef<readonly Message[]>([]);

  // A call that is on its way is ended with the page.
  useEffect(
    () => () => {
      controller.current?.abort();
    },
    [],
  );

  const send = useCallback(async (call: Call): Promise<boolean> => {
    const abort = new AbortController();
    controller.current = abort;
    const asked = bodyOf(call, history.current);
    const sent: readonly Message[] = [...history.current, ...call.add];
    history.current = sent;
    setMessages(sent);
    setError(null);
    setFinished(null);
    setPartial("");
    setRunning(true);

    let answer = "";
    const calls = new ToolCallAssembler();
    let usage: Usage | null = null;
    let model: string | null = null;
    let failure: string | null = null;
    let failed = false;
    try {
      // The generated type of free-form JSON (tools, parts) is `Record<string, never>`.
      const response = await playgroundChat(asked as unknown as Parameters<typeof playgroundChat>[0], abort.signal);
      const reader = response.body?.getReader();
      if (reader === undefined) throw new Error(NO_ANSWER);
      // Stopping ends the read at once, whatever the connection does.
      abort.signal.addEventListener("abort", () => {
        reader.cancel().catch(() => undefined);
      });
      const decoder = new TextDecoder();
      const events = new SseReader();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        for (const data of events.feed(decoder.decode(value, { stream: true }))) {
          const chunk = chunkOf(data);
          if (chunk.text !== undefined) {
            answer += chunk.text;
            setPartial(answer);
          }
          if (chunk.toolCalls !== undefined) calls.add(chunk.toolCalls);
          if (chunk.usage !== undefined) {
            usage = chunk.usage;
            model = chunk.model ?? model;
          }
          if (chunk.error !== undefined) failure = chunk.error;
        }
      }
    } catch (caught) {
      // Stopping is the user's own doing: what was said stays, nothing is wrong.
      if (!abort.signal.aborted) {
        failed = true;
        failure = failureText(caught);
      }
    }
    if (controller.current === abort) controller.current = null;

    let putBack = false;
    const toolCalls = calls.calls();
    if (answer !== "" || toolCalls.length > 0) {
      const kept: readonly Message[] = [
        ...sent,
        toolCalls.length > 0
          ? { role: "assistant", content: answer === "" ? null : answer, tool_calls: toolCalls }
          : { role: "assistant", content: answer },
      ];
      history.current = kept;
      setMessages(kept);
    } else if (failed || failure !== null || abort.signal.aborted) {
      // Nothing was answered: what was added goes back, to be sent again.
      history.current = history.current.slice(0, history.current.length - call.add.length);
      setMessages(history.current);
      putBack = true;
    }
    setPartial("");
    setRunning(false);
    setError(failure);
    setFinished(usage === null ? null : { usage, called: call.model, model });
    return putBack;
  }, []);

  const stop = useCallback(() => {
    controller.current?.abort();
  }, []);

  const clear = useCallback(() => {
    controller.current?.abort();
    history.current = [];
    setMessages([]);
    setPartial("");
    setError(null);
    setFinished(null);
  }, []);

  return { messages, partial, running, error, finished, send, stop, clear };
}
