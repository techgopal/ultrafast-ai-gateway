// Errors of the API client. Neither keeps the request, the response body or a
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
