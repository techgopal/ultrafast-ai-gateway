// Errors of the API client, what the console refuses itself, and what the
// user is told of an error. None keeps the request, the response body or a
// header: an error may be logged or shown by an error boundary, and requests
// can hold passwords and provider API keys.

/** The gateway answered, and the answer was not a success. */
export class ApiError extends Error {
  override readonly name = "ApiError";
  readonly status: number;
  /** A stable name such as `forbidden`, `last_admin` or `validation_failed`. */
  readonly code: string;
  /** A message per field that is not valid. Empty when there is none. */
  readonly fields: Readonly<Record<string, string>>;

  constructor(
    status: number,
    code: string,
    message: string,
    fields: Readonly<Record<string, string>> = {},
  ) {
    super(message);
    this.status = status;
    this.code = code;
    this.fields = Object.freeze({ ...fields });
  }
}

/** The request did not complete: offline, connection refused, and the like. */
export class NetworkError extends Error {
  override readonly name = "NetworkError";

  constructor() {
    super("Could not reach the gateway.");
  }
}

/**
 * The answer belongs to a session that has ended since the request was made.
 * Whatever it says, it says nothing to whoever is signed in now.
 */
export class SessionOverError extends Error {
  override readonly name = "SessionOverError";

  constructor() {
    super("The session this request was made in is over.");
  }
}

/**
 * The console itself refuses something: a form that holds what cannot be
 * sent, or an answer that cannot be used. It is not an answer of the gateway,
 * and so has no status and no code; `ApiError` is only what the gateway said.
 * The message is a text of the console, written for the user.
 *
 * An answer of the gateway is never turned into one, also not where a form
 * says it in words of its own: it stays an `ApiError`, and `onField` of
 * `components/form.ts` gives the field its text.
 */
export class ConsoleRefusal extends Error {
  override readonly name = "ConsoleRefusal";
  /** The field of the form the refusal is about, when it is about one. */
  readonly field: string | undefined;

  constructor(message: string, field?: string) {
    super(message);
    this.field = field;
  }
}

export const SOMETHING_WENT_WRONG = "Something went wrong.";

/**
 * What to tell the user about a failure, or `null` when there is nothing to
 * tell: an answer of a session that is over says nothing to who is signed in
 * now. Only the message of the gateway, the message of the network error and
 * what the console itself refuses are shown; whatever else was thrown may
 * hold what is not for the user.
 */
export function messageOfError(error: unknown): string | null {
  if (error instanceof SessionOverError) return null;
  if (
    error instanceof ApiError ||
    error instanceof NetworkError ||
    error instanceof ConsoleRefusal
  ) {
    return error.message;
  }
  return SOMETHING_WENT_WRONG;
}
