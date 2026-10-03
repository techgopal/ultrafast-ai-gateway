// A fake fetch that records requests and answers from a script, deterministic.
export interface Seen {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: string;
  redirect: string | undefined;
}

export type Answer = Response | Error | ((seen: Seen, init: RequestInit) => Response | Promise<Response>);

export function fakeFetch(answer: Answer): { fetch: typeof fetch; seen: Seen[] } {
  const seen: Seen[] = [];
  const f = async (input: string | URL | Request, init: RequestInit = {}): Promise<Response> => {
    const rec: Seen = {
      url: String(input),
      method: init.method ?? "GET",
      headers: Object.fromEntries(Object.entries((init.headers ?? {}) as Record<string, string>)),
      body: typeof init.body === "string" ? init.body : "",
      redirect: init.redirect,
    };
    seen.push(rec);
    if (answer instanceof Error) throw answer;
    if (typeof answer === "function") return answer(rec, init);
    // A cloned body is a tee: cancelling one branch would not cancel the source. Streams are used once.
    return answer.headers.get("content-type") === "text/event-stream" ? answer : answer.clone();
  };
  return { fetch: f as typeof fetch, seen };
}

export const json = (status: number, body: unknown, headers: Record<string, string> = {}): Response =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", ...headers } });

const enc = new TextEncoder();

/** A body delivered in the given pieces (one `read()` each). */
export function pieces(parts: Uint8Array[], opts: { stall?: boolean; fail?: Error } = {}): ReadableStream<Uint8Array> {
  let i = 0;
  return new ReadableStream<Uint8Array>({
    pull(c) {
      if (i < parts.length) {
        c.enqueue(parts[i++]);
      } else if (opts.fail) {
        c.error(opts.fail);
      } else if (opts.stall) {
        return new Promise(() => {});
      } else {
        c.close();
      }
      return undefined;
    },
  }, { highWaterMark: 0 });
}

export const sse = (o: unknown): string => `data: ${JSON.stringify(o)}\n\n`;

export function streamResponse(text: string, split: "whole" | "byte" = "whole", opts: { stall?: boolean; fail?: Error } = {}): Response {
  const bytes = enc.encode(text);
  const parts = split === "byte" ? Array.from(bytes, (b) => Uint8Array.of(b)) : [bytes];
  return new Response(pieces(parts, opts), { status: 200, headers: { "content-type": "text/event-stream" } });
}

export const OPENAI_CHAT = {
  id: "c1",
  model: "gpt-4o",
  choices: [{ message: { role: "assistant", content: "hello" }, finish_reason: "stop" }],
  usage: { prompt_tokens: 3, completion_tokens: 2 },
};

export const OPENAI_STREAM = [
  sse({ choices: [{ delta: { content: "he" } }] }),
  sse({ choices: [{ delta: { content: "llo" } }] }),
  sse({ choices: [{ delta: {}, finish_reason: "stop" }], usage: { prompt_tokens: 3, completion_tokens: 2 } }),
  "data: [DONE]\n\n",
].join("");

export const KEY = "uf-secret-key-12345";
export const MSGS = [{ role: "user" as const, content: "hi" }];
