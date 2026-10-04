/** What went wrong, in terms a caller can act on. */
export type ErrorKind =
  | "auth"
  | "permission"
  | "not_found"
  | "invalid_request"
  | "rate_limited"
  | "upstream"
  | "network"
  | "timeout"
  | "malformed";

interface ErrorFields {
  kind: ErrorKind;
  retryable: boolean;
  status?: number | null;
  message: string;
  retry_after_secs?: number | null;
}

/**
 * Every failure of the client. The client does not retry: `retryable` says
 * whether trying again could help and `retryAfter` (seconds) how long the
 * server asked to wait. The message never contains the API key.
 */
export class UltrafastError extends Error {
  readonly kind: ErrorKind;
  /** The HTTP status of the answer, when there was one. */
  readonly status: number | undefined;
  readonly retryable: boolean;
  /** Seconds the server asked to wait (`Retry-After`), when it did. */
  readonly retryAfter: number | undefined;

  constructor(fields: ErrorFields) {
    super(fields.message);
    this.name = new.target.name;
    this.kind = fields.kind;
    this.retryable = fields.retryable;
    this.status = fields.status ?? undefined;
    this.retryAfter = fields.retry_after_secs ?? undefined;
  }
}

export class AuthenticationError extends UltrafastError {}
export class PermissionDeniedError extends UltrafastError {}
export class NotFoundError extends UltrafastError {}
export class InvalidRequestError extends UltrafastError {}
export class RateLimitError extends UltrafastError {}
export class UpstreamError extends UltrafastError {}
export class NetworkError extends UltrafastError {}
export class RequestTimeoutError extends UltrafastError {}
export class MalformedError extends UltrafastError {}

const BY_KIND: Record<ErrorKind, new (f: ErrorFields) => UltrafastError> = {
  auth: AuthenticationError,
  permission: PermissionDeniedError,
  not_found: NotFoundError,
  invalid_request: InvalidRequestError,
  rate_limited: RateLimitError,
  upstream: UpstreamError,
  network: NetworkError,
  timeout: RequestTimeoutError,
  malformed: MalformedError,
};

/** The error for the JSON the wasm module produces or throws. */
export function errorFromJson(json: string): UltrafastError {
  const f = JSON.parse(json) as ErrorFields;
  const cls = BY_KIND[f.kind] ?? MalformedError;
  return new cls(f);
}
