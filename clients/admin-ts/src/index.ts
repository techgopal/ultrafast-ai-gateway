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
  downloadBackup(): Promise<Uint8Array>;
  /** The configuration file (`GET /api/config/export`), parsed. */
  exportConfig(): Promise<components["schemas"]["ConfigFile"]>;
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

export function createAdminClient(options: AdminClientOptions): AdminClient {
  const { token } = options;
  const timeoutMs = options.timeoutMs ?? 30_000;
  const base = options.fetch ?? ((input, init) => fetch(input, init));
  const baseUrl = options.baseUrl.replace(/\/+$/, "");

  const guarded: typeof fetch = async (input, init) => {
    const signals = [AbortSignal.timeout(timeoutMs)];
    if (init?.signal) signals.push(init.signal);
    if (input instanceof Request) signals.push(input.signal);
    try {
      return await base(input, { ...init, signal: AbortSignal.any(signals) });
    } catch (error) {
      if (error instanceof DOMException && error.name === "TimeoutError") {
        throw new AdminApiError(0, "timeout", `No answer within ${String(timeoutMs)} ms.`);
      }
      if (error instanceof AdminApiError) throw error;
      // A caller's own abort stays an abort.
      if (error instanceof DOMException && error.name === "AbortError") throw error;
      const reason = error instanceof Error ? error.message : "the request failed";
      throw new AdminApiError(0, "network_error", redact(reason, token));
    }
  };

  const raw = createClient<paths>({
    baseUrl,
    fetch: guarded,
    headers: { authorization: `Bearer ${token}` },
  });

  async function call<D>(request: Promise<Settled<D>>): Promise<Data<D>> {
    const { data, error, response } = await request;
    if (!response.ok) throw errorOf(response.status, error, token);
    return data as Data<D>;
  }

  return {
    raw,
    call,
    async downloadBackup() {
      const bytes = await call(raw.GET("/api/backup", { parseAs: "arrayBuffer" }));
      return new Uint8Array(bytes as unknown as ArrayBuffer);
    },
    async exportConfig() {
      const text = await call(raw.GET("/api/config/export", { parseAs: "text" }));
      return JSON.parse(text as unknown as string) as components["schemas"]["ConfigFile"];
    },
  };
}
