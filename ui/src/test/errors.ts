// The errors the gateway's `/api` answers with: status, code and message as
// the gateway source has them (`crates/gateway/src/api`). A test that needs an
// error takes it from here, so that no test invents one the gateway never sends.
// `errors.test.ts` pins them.
import type { components } from "@/api/schema";

type ApiErrorBody = components["schemas"]["ApiErrorBody"];

export interface GatewayError {
  readonly status: number;
  readonly body: ApiErrorBody;
}

function error(status: number, code: string, message: string): GatewayError {
  return { status, body: { error: { code, message } } };
}

/**
 * The messages the gateway gives for a field that is not valid. The name of
 * an entry is the field, then what is wrong with it.
 */
export const fieldMessages = {
  name: "name must be 1 to 100 characters",
  teamName: "name must be 1 to 60 characters",
  email: "email is not valid",
  password: "password must be at least 12 characters",
  passwordTooLong: "password must be at most 256 characters",
  userRole: "role must be admin or member",
  teamRole: "role must be lead or member",
  userStatus: "status must be active or disabled",
  expiresAtForm: "expires_at must be a UTC time in the form YYYY-MM-DD HH:MM:SS",
  expiresAtPast: "expires_at must be in the future",
  ownerId: "owner must be an active user",
  teamId: "team does not exist",
  teamIdOwner: "owner is not a member of this team",
  providerKind: "kind must be openai or anthropic",
  baseUrl: "base URL must start with http:// or https://",
  apiKey: "must not be empty",
  positive: "must be a positive integer",
} as const;

/** A 422 of the gateway for these fields. */
export function validationFailed(fields: Readonly<Record<string, string>>): GatewayError {
  return {
    status: 422,
    body: {
      error: {
        code: "validation_failed",
        message: "Some fields are not valid.",
        fields: { ...fields },
      },
    },
  };
}

export const errors = {
  // The general ones (`api/mod.rs`).
  bad_request: error(
    400,
    "bad_request",
    "The body must be JSON, sent as application/json, with exactly the expected fields.",
  ),
  validation_failed: validationFailed({
    email: fieldMessages.email,
    name: fieldMessages.name,
  }),
  unauthenticated: error(401, "unauthenticated", "Sign in to continue."),
  invalid_credentials: error(401, "invalid_credentials", "Email or password is incorrect."),
  csrf_failed: error(403, "csrf_failed", "The CSRF token is missing or does not match."),
  forbidden: error(403, "forbidden", "You are not allowed to do this."),
  not_found: error(404, "not_found", "Not found."),
  method_not_allowed: error(405, "method_not_allowed", "This method is not supported here."),
  payload_too_large: error(413, "payload_too_large", "The request body is too large."),
  too_many_attempts: error(
    429,
    "too_many_attempts",
    "Too many failed attempts. Try again later.",
  ),
  internal_error: error(500, "internal_error", "Something went wrong."),
  // The ones of one resource, all 409.
  already_set_up: error(409, "already_set_up", "The gateway is already set up."),
  last_admin: error(409, "last_admin", "At least one active admin is required."),
  cannot_delete_self: error(409, "cannot_delete_self", "You cannot delete your own account."),
  no_password: error(
    409,
    "no_password",
    "This user has no password yet. Send them an invite instead.",
  ),
  not_invited: error(
    409,
    "not_invited",
    "Only a user who has not accepted an invite can get a new one.",
  ),
  user_exists: error(409, "user_exists", "A user with this email already exists."),
  user_disabled: error(409, "user_disabled", "A disabled user cannot be added to a team."),
  team_exists: error(409, "team_exists", "A team with this name already exists."),
  provider_exists: error(409, "provider_exists", "A provider with this name already exists."),
} as const satisfies Record<string, GatewayError>;

export type ErrorName = keyof typeof errors;
