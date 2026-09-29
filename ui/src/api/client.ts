// The one place that talks to the gateway. Requests go to the console's own
// origin, under `/api`, with the session cookie the browser holds. Nothing
// here logs a request or a response.
import { ApiError, NetworkError } from "./errors";
import type { paths } from "./schema";

export type Method = "get" | "post" | "put" | "patch" | "delete";

/** The paths that have an operation for the method. */
export type PathFor<M extends Method> = {
  [P in keyof paths]: paths[P] extends Record<M, { responses: unknown }> ? P : never;
}[keyof paths];

export type GetPath = PathFor<"get">;
export type PostPath = PathFor<"post">;
export type PutPath = PathFor<"put">;
export type PatchPath = PathFor<"patch">;
export type DeletePath = PathFor<"delete">;

type OperationOf<P extends keyof paths, M extends Method> = paths[P] extends Record<M, infer Op>
  ? Op
  : never;

type Success = 200 | 201 | 204;

type SuccessBody<Op> = Op extends { responses: infer R }
  ? {
      [S in keyof R & Success]: R[S] extends { content: { "application/json": infer B } }
        ? B
        : undefined;
    }[keyof R & Success]
  : never;

/** What a call resolves to: the body of the 200 or 201, `undefined` for a 204. */
export type ResponseOf<P extends keyof paths, M extends Method> = SuccessBody<OperationOf<P, M>>;

/** The JSON body the operation takes; `never` when it takes none. */
export type BodyOf<P extends keyof paths, M extends Method> =
  OperationOf<P, M> extends { requestBody: { content: { "application/json": infer B } } }
    ? B
    : never;

/** The path parameters of the operation; `never` when it has none. */
export type ParamsOf<P extends keyof paths, M extends Method> =
  OperationOf<P, M> extends { parameters: { path: infer T } } ? T : never;

/** The query parameters of the operation; `never` when it has none. */
export type QueryOf<P extends keyof paths, M extends Method> =
  OperationOf<P, M> extends { parameters: { query?: infer Q } } ? NonNullable<Q> : never;

type Field<K extends string, T> = [T] extends [never] ? Partial<Record<K, never>> : Record<K, T>;
type OptionalField<K extends string, T> = [T] extends [never]
  ? Partial<Record<K, never>>
  : Partial<Record<K, T>>;

export type OptionsOf<P extends keyof paths, M extends Method> = Field<"params", ParamsOf<P, M>> &
  Field<"body", BodyOf<P, M>> &
  OptionalField<"query", QueryOf<P, M>> & { signal?: AbortSignal };

/** The options may be left out when the operation needs neither parameters nor a body. */
type Args<P extends keyof paths, M extends Method> =
  Partial<OptionsOf<P, M>> extends OptionsOf<P, M>
    ? [opts?: OptionsOf<P, M>]
    : [opts: OptionsOf<P, M>];

let csrfToken: string | null = null;
let signedOutTold = false;
const unauthenticatedHandlers = new Set<() => void>();

/**
 * The CSRF token of the session, kept in memory only.
 *
 * The `onUnauthenticated` handlers are told of the end of a session once.
 * They are told again after a new session began: a token was set here, or
 * sign-in or `/api/auth/me` succeeded.
 */
export function setCsrfToken(token: string | null): void {
  csrfToken = token;
  if (token !== null) signedOutTold = false;
}

/** Registers a handler for the end of the session. Returns the unsubscribe. */
export function onUnauthenticated(handler: () => void): () => void {
  unauthenticatedHandlers.add(handler);
  return () => {
    unauthenticatedHandlers.delete(handler);
  };
}

// A 401 from these says something else than "the session ended": wrong
// credentials, a wrong current password, or nobody signed in yet.
const NOT_A_SIGN_OUT: ReadonlySet<string> = new Set([
  "post /api/auth/login",
  "post /api/auth/accept-invite",
  "post /api/auth/password",
  "get /api/auth/me",
]);

const SHOWS_A_SESSION: ReadonlySet<string> = new Set([
  "post /api/auth/login",
  "get /api/auth/me",
]);

function tellUnauthenticated(): void {
  if (signedOutTold) return;
  signedOutTold = true;
  for (const handler of [...unauthenticatedHandlers]) handler();
}

interface RequestOptions {
  params?: unknown;
  query?: unknown;
  body?: unknown;
  signal?: AbortSignal;
}

function fillPath(path: string, params: unknown): string {
  const given = new Map(isRecord(params) ? Object.entries(params) : []);
  return path.replace(/\{([^}]+)\}/g, (_, name: string) => {
    const value = given.get(name);
    if (typeof value !== "string" && typeof value !== "number") {
      throw new Error(`The path ${path} needs the parameter "${name}".`);
    }
    return encodeURIComponent(String(value));
  });
}

function queryString(query: unknown): string {
  const search = new URLSearchParams();
  for (const [name, value] of isRecord(query) ? Object.entries(query) : []) {
    if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
      search.set(name, String(value));
    }
  }
  const text = search.toString();
  return text === "" ? "" : `?${text}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return undefined;
  }
}

function unexpected(status: number): ApiError {
  return new ApiError(
    status,
    "unexpected_response",
    `The gateway answered with status ${status}, in a form the console does not know.`,
  );
}

/** The error of a response that is not a success. The body is shown only in the `/api` error shape. */
function errorOf(status: number, text: string): ApiError {
  const body = parseJson(text);
  if (!isRecord(body) || !isRecord(body.error)) return unexpected(status);
  const { code, message, fields } = body.error;
  if (typeof code !== "string" || typeof message !== "string") return unexpected(status);
  const kept: Record<string, string> = {};
  if (isRecord(fields)) {
    for (const [name, value] of Object.entries(fields)) {
      if (typeof value === "string") kept[name] = value;
    }
  }
  return new ApiError(status, code, message, kept);
}

async function request(method: Method, path: string, opts: RequestOptions = {}): Promise<unknown> {
  const url = fillPath(path, opts.params) + queryString(opts.query);
  const headers: Record<string, string> = { Accept: "application/json" };
  const init: RequestInit = {
    method: method.toUpperCase(),
    credentials: "same-origin",
    headers,
  };
  if (opts.body !== undefined) {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(opts.body);
  }
  if (method !== "get" && csrfToken !== null) headers["x-csrf-token"] = csrfToken;
  if (opts.signal !== undefined) init.signal = opts.signal;

  let response: Response;
  let text: string;
  try {
    response = await fetch(url, init);
    text = await response.text();
  } catch (error) {
    // A cancelled request is not a failure of the network.
    if (opts.signal?.aborted === true) throw error;
    throw new NetworkError();
  }

  if (!response.ok) {
    const error = errorOf(response.status, text);
    if (response.status === 401 && !NOT_A_SIGN_OUT.has(`${method} ${path}`)) {
      tellUnauthenticated();
    }
    throw error;
  }
  // An answer to these two with a success says that there is a session.
  if (SHOWS_A_SESSION.has(`${method} ${path}`)) signedOutTold = false;
  if (response.status === 204 || text === "") return undefined;
  const body = parseJson(text);
  if (body === undefined) throw unexpected(response.status);
  return body;
}

function caller<M extends Method>(method: M) {
  return <P extends PathFor<M>>(path: P, ...args: Args<P, M>): Promise<ResponseOf<P, M>> =>
    // The gateway's answer is trusted to have the shape its description gives.
    request(method, path, args[0]) as Promise<ResponseOf<P, M>>;
}

export const api = {
  get: caller("get"),
  post: caller("post"),
  put: caller("put"),
  patch: caller("patch"),
  delete: caller("delete"),
};
