/** The client's own network handling: the body cap and the timeouts. */

export type HostKind = "network" | "timeout" | "malformed" | "invalid_request";

/** A failure the client itself raises; `hostError` in the wasm module gives it its kind's shape. */
export class HostFailure extends Error {
  constructor(
    readonly hostKind: HostKind,
    message: string,
  ) {
    super(message);
  }
}

/** An abort or timeout raised by the runtime itself. */
function isAbort(e: unknown): boolean {
  const name = (e as { name?: unknown } | null)?.name;
  return name === "AbortError" || name === "TimeoutError";
}

const broke = (e: unknown, message: string): HostFailure =>
  isAbort(e) ? new HostFailure("timeout", "the request timed out") : new HostFailure("network", message);

/** `p`, or a `timeout` failure (and an abort of the request) after `ms`, even if `p` ignores the abort. */
export function timed<T>(p: Promise<T>, ms: number, ctl: AbortController, message = "the request timed out"): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const t = setTimeout(
      () => {
        ctl.abort();
        reject(new HostFailure("timeout", message));
      },
      Math.max(0, ms),
    );
    p.then(
      (v) => {
        clearTimeout(t);
        resolve(v);
      },
      (e: unknown) => {
        clearTimeout(t);
        reject(e instanceof Error ? e : new HostFailure("network", "the request failed"));
      },
    );
  });
}

/** An HTTP status as the wasm module wants it: an integer in 0..=999. */
export function clampStatus(status: unknown): number {
  const n = Math.trunc(Number(status));
  return Number.isFinite(n) ? Math.min(999, Math.max(0, n)) : 0;
}

export function asBytes(chunk: unknown): Uint8Array {
  if (chunk instanceof Uint8Array) return chunk;
  if (ArrayBuffer.isView(chunk)) return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
  if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
  throw new HostFailure("malformed", "the body is not bytes");
}

export interface Request {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: string;
}

/** Sends the request (redirects are never followed) and waits for the headers. */
export async function send(fetchFn: typeof fetch, req: Request, ctl: AbortController, ms: number): Promise<Response> {
  const attempt = (async () => {
    try {
      return await fetchFn(req.url, {
        method: req.method,
        headers: req.headers,
        body: req.body,
        signal: ctl.signal,
        redirect: "manual",
      });
    } catch (e) {
      // The runtime's own message can hold the URL; the caller gets a fixed one
      // (the Rust client's wording).
      if (isAbort(e)) throw new HostFailure("timeout", "the request timed out");
      throw new HostFailure("network", "could not connect to the server");
    }
  })();
  return timed(attempt, ms, ctl);
}

export interface Answer {
  status: number;
  retryAfter: string | undefined;
  body: Uint8Array;
}

const tooBig = (max: number): HostFailure =>
  new HostFailure("malformed", `the response is larger than the limit of ${max} bytes`);

/**
 * The whole body, at most `max` bytes, within `deadline` (a time in ms since
 * the epoch shared with the request). An opaque redirect answers as a 302.
 */
export async function readAnswer(resp: Response, max: number, deadline: number, ctl: AbortController): Promise<Answer> {
  const retryAfter = resp.headers.get("retry-after") ?? undefined;
  if (resp.type === "opaqueredirect") return { status: 302, retryAfter: undefined, body: new Uint8Array(0) };
  const status = clampStatus(resp.status);
  const declared = Number(resp.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > max) {
    ctl.abort();
    throw tooBig(max);
  }
  const chunks: Uint8Array[] = [];
  let size = 0;
  if (resp.body) {
    const reader = resp.body.getReader();
    try {
      for (;;) {
        const next = await timed(
          reader.read().catch((e: unknown) => {
            throw broke(e, "network error: the connection broke while reading the answer");
          }),
          deadline - Date.now(),
          ctl,
        );
        if (next.done) break;
        const chunk = asBytes(next.value);
        size += chunk.length;
        if (size > max) {
          ctl.abort();
          throw tooBig(max);
        }
        chunks.push(chunk);
      }
    } finally {
      reader.cancel().catch(() => undefined);
    }
  }
  const body = new Uint8Array(size);
  let at = 0;
  for (const c of chunks) {
    body.set(c, at);
    at += c.length;
  }
  return { status, retryAfter, body };
}
