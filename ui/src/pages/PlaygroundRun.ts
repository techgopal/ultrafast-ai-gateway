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
  type ChatRequestBody,
  type Message,
  type Usage,
} from "@/lib/playground";

export const NO_ANSWER = "The gateway sent no answer.";

export interface Finished {
  usage: Usage;
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
  /** Sends the message. Resolves with the text to put back in the box when the call failed. */
  send: (call: Call) => Promise<string | null>;
  stop: () => void;
  clear: () => void;
}

export interface Call {
  model: string;
  system: string;
  text: string;
  values: Parameters<typeof requestBody>[3];
}

/** The body the next send would have: for the call itself and for "Copy as curl". */
export function bodyOf(call: Call, history: readonly Message[]): ChatRequestBody {
  return requestBody(call.model, call.system, [...history, { role: "user", content: call.text }], call.values);
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

  const send = useCallback(async (call: Call): Promise<string | null> => {
    const abort = new AbortController();
    controller.current = abort;
    const asked = bodyOf(call, history.current);
    const sent: readonly Message[] = [...history.current, { role: "user", content: call.text }];
    history.current = sent;
    setMessages(sent);
    setError(null);
    setFinished(null);
    setPartial("");
    setRunning(true);

    let answer = "";
    let usage: Usage | null = null;
    let model: string | null = null;
    let failure: string | null = null;
    let failed = false;
    try {
      const response = await playgroundChat(asked, abort.signal);
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

    let putBack: string | null = null;
    if (answer !== "") {
      const kept: readonly Message[] = [...sent, { role: "assistant", content: answer }];
      history.current = kept;
      setMessages(kept);
    } else if (failed || failure !== null) {
      // Nothing was answered: the message goes back to the box, to be sent again.
      history.current = history.current.slice(0, -1);
      setMessages(history.current);
      putBack = call.text;
    } else if (abort.signal.aborted) {
      history.current = history.current.slice(0, -1);
      setMessages(history.current);
      putBack = call.text;
    }
    setPartial("");
    setRunning(false);
    setError(failure);
    setFinished(usage === null ? null : { usage, model });
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
