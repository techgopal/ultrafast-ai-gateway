import createClient, { type Client } from "openapi-fetch";
import type { components, paths } from "./schema.js";

export type { components, paths } from "./schema.js";

/** Every failed call of the admin API: the gateway's `ApiErrorBody`, or a failure to reach it. */
export class AdminApiError extends Error {
  /** The HTTP status; 0 when no answer came (`network_error`, `timeout`). */
  readonly status: number;
  /** The gateway's stable error name, such as `not_found`; `http_<status>` for an answer that is not its error shape. */
  readonly code: string;
  /** For `validation_failed`: a message for each field that is not valid. Empty otherwise. */
  readonly fields: Readonly<Record<string, string>>;

  constructor(
    status: number,
    code: string,
    message: string,
    fields: Record<string, string> = {},
  ) {
    super(message);
    this.name = "AdminApiError";
    this.status = status;
    this.code = code;
    this.fields = fields;
  }
}

export interface AdminClientOptions {
  /** For example `https://gateway.example.com`. */
  baseUrl: string;
  /** An access token (`uf-at-…`, Account, Access tokens). Needs no CSRF header. */
  token: string;
  /** Per request, in milliseconds. Default 30 000. */
  timeoutMs?: number;
  /** In place of the global `fetch`. */
  fetch?: typeof fetch;
}

type Settled<D> = { data?: D; error?: unknown; response: Response };
/** What `call` returns for a data type: nothing for a 204 (whose data type is `never`). */
type Data<D> = [D] extends [never] ? undefined : D;

export interface AdminClient {
  /** The typed `openapi-fetch` client over every operation of `openapi/admin.json`. */
  readonly raw: Client<paths>;
  /**
   * Awaits a call of `raw` and returns its data. An answer that is not a
   * success throws an `AdminApiError`.
   */
  call<D>(request: Promise<Settled<D>>): Promise<Data<D>>;
  /** The database as the bytes of a SQLite file (`GET /api/backup`). */
  downloadBackup(options?: DownloadOptions): Promise<Uint8Array>;
  /** The configuration file (`GET /api/config/export`), parsed. */
  exportConfig(options?: DownloadOptions): Promise<components["schemas"]["ConfigFile"]>;
}

/** Makes sure the token cannot appear in an error, should an answer ever echo it. */
function redact(text: string, token: string): string {
  return token === "" ? text : text.split(token).join("[redacted]");
}

function errorOf(status: number, body: unknown, token: string): AdminApiError {
  if (typeof body === "object" && body !== null && "error" in body) {
    const detail = (body as { error: unknown }).error;
    if (typeof detail === "object" && detail !== null) {
      const { code, message, fields } = detail as Record<string, unknown>;
      if (typeof code === "string" && typeof message === "string") {
        const clean: Record<string, string> = {};
        if (typeof fields === "object" && fields !== null) {
          for (const [name, text] of Object.entries(fields)) {
            if (typeof text === "string") clean[name] = redact(text, token);
          }
        }
        return new AdminApiError(status, code, redact(message, token), clean);
      }
    }
  }
  const text = typeof body === "string" && body !== "" ? body : `HTTP ${String(status)}`;
  return new AdminApiError(status, `http_${String(status)}`, redact(text, token));
}

/** Options of the two downloads, which can take longer than a call. */
export interface DownloadOptions {
  /** For this download only, in milliseconds; the client's `timeoutMs` when left out. */
  timeoutMs?: number;
}

export function createAdminClient(options: AdminClientOptions): AdminClient {
  const { token } = options;
  const defaultTimeoutMs = options.timeoutMs ?? 30_000;
  const base = options.fetch ?? ((input, init) => fetch(input, init));
  const baseUrl = options.baseUrl.replace(/\/+$/, "");
  // Reasons of aborts that the caller's own signals gave: they are rethrown as given.
  // Objects go in a WeakSet; a primitive reason (`abort("stop")`) can only come from an abort,
  // since a network failure is always an Error.
  const callerReasons = new WeakSet<object>();
  const callerPrimitives = new Set<unknown>();
  const remember = (reason: unknown): void => {
    if (typeof reason === "object" && reason !== null) callerReasons.add(reason);
    else callerPrimitives.add(reason);
  };

  const guard = (timeoutMs: number): typeof fetch => async (input, init) => {
    const callers = [init?.signal, input instanceof Request ? input.signal : undefined].filter(
      (signal): signal is AbortSignal => signal !== undefined && signal !== null,
    );
    for (const signal of callers) {
      if (signal.aborted) {
        remember(signal.reason);
        throw signal.reason;
      }
      signal.addEventListener(
        "abort",
        () => {
          remember(signal.reason);
        },
        { once: true },
      );
    }
    try {
      return await base(input, { ...init, signal: AbortSignal.any([AbortSignal.timeout(timeoutMs), ...callers]) });
    } catch (error) {
      throw mapped(error, timeoutMs);
    }
  };

  /** What a failure becomes: the caller's own abort stays as it was, the rest is an `AdminApiError`. */
  function mapped(error: unknown, timeoutMs: number): unknown {
    if (error instanceof AdminApiError) return error;
    if (typeof error === "object" && error !== null ? callerReasons.has(error) : callerPrimitives.has(error)) {
      return error;
    }
    if (error instanceof DOMException && error.name === "TimeoutError") {
      return new AdminApiError(0, "timeout", `No answer within ${String(timeoutMs)} ms.`);
    }
    if (error instanceof DOMException && error.name === "AbortError") return error;
    const reason = error instanceof Error ? error.message : "the request failed";
    return new AdminApiError(0, "network_error", redact(reason, token));
  }

  const build = (timeoutMs: number) =>
    createClient<paths>({
      baseUrl,
      fetch: guard(timeoutMs),
      headers: { authorization: `Bearer ${token}` },
    });
  const raw = build(defaultTimeoutMs);

  async function settle<D>(request: Promise<Settled<D>>, timeoutMs: number): Promise<Data<D>> {
    let settled: Settled<D>;
    try {
      // The timeout also covers reading the body, which fails here.
      settled = await request;
    } catch (error) {
      throw mapped(error, timeoutMs);
    }
    const { data, error, response } = settled;
    if (!response.ok) throw errorOf(response.status, error, token);
    return data as Data<D>;
  }

  return {
    raw,
    call: (request) => settle(request, defaultTimeoutMs),
    async downloadBackup(download) {
      const ms = download?.timeoutMs ?? defaultTimeoutMs;
      const client = download?.timeoutMs === undefined ? raw : build(ms);
      const bytes = await settle(client.GET("/api/backup", { parseAs: "arrayBuffer" }), ms);
      return new Uint8Array(bytes as unknown as ArrayBuffer);
    },
    async exportConfig(download) {
      const ms = download?.timeoutMs ?? defaultTimeoutMs;
      const client = download?.timeoutMs === undefined ? raw : build(ms);
      const text = await settle(client.GET("/api/config/export", { parseAs: "text" }), ms);
      try {
        return JSON.parse(text as unknown as string) as components["schemas"]["ConfigFile"];
      } catch {
        // A proxy's page, say: say what happened, never echo the body.
        throw new AdminApiError(200, "invalid_response", "The answer was not a configuration file (not JSON).");
      }
    },
  };
}
