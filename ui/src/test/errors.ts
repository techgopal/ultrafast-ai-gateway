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
  // `crates/gateway/src/tags.rs`: what is wrong with the tags of a key.
  tags: "it has more than 20 entries",
  providerKind: "kind must be openai, anthropic, gemini or azure",
  // `api/providers.rs` and `config.rs`: an API version is for Azure only, and has a form.
  apiVersionKind: "only Azure OpenAI providers have an API version",
  apiVersionForm: "API version must look like 2024-10-21 or 2025-03-01-preview",
  // `api/keys.rs`: what a caller who is not an admin is told of a name in the allowlist of a key.
  allowedHidden: "'gpt-secret' is not a model or route you can use",
  // The name and the base URL of a provider (`crates/gateway/src/config.rs`).
  providerName:
    "provider name must be 1 to 40 characters of a-z, 0-9, '-' and '_', starting with a letter or a digit",
  baseUrl: "base URL must start with http:// or https://",
  baseUrlWhitespace: "base URL must not contain whitespace",
  baseUrlQuery: "base URL must not contain a query string",
  baseUrlFragment: "base URL must not contain a fragment",
  baseUrlCredentials:
    "base URL must not contain credentials; give the key with --api-key or UF_PROVIDER_API_KEY",
  baseUrlHost: "base URL must include a host",
  apiKey: "must not be empty",
  positive: "must be a positive integer",
  // The name of a model (`crates/gateway/src/catalog/mod.rs`) and its grants (`api/models.rs`).
  modelName: "name must be 1 to 200 characters",
  modelNameWhitespace: "name must not contain whitespace or control characters",
  providerMissing: "provider does not exist",
  grantsEveryone: "must not be combined with teams or users",
  teamsMissing: "a team does not exist",
  usersMissing: "a user does not exist",
  // A route (`api/routes.rs`).
  routeName:
    "must be 1 to 64 characters of a-z, 0-9, '.', '_' and '-', starting with a letter or digit",
  routePrimariesNeeded: "needs at least one model",
  routeWeight: "a weight must be 1 to 1000",
  routeOnce: "a model may appear only once in a route",
  routeModelMissing: "a model does not exist",
  routeTeamMissing: "a team does not exist",
  routeRetries: "must be 0 to 5",
  routeFirstToken: "must be 1000 to 300000",
  routeTotalBelowFirst: "must not be below the first token timeout",
  // The alerts (`api/alerts.rs`, `config.rs`).
  channelUrlScheme: "URL must start with http:// or https://",
  channelUrlInvalid: "URL is not valid",
  channelMissing: "No such channel.",
  needsUrl: "Set a URL before enabling this channel.",
  budgetMissing: "no such budget",
  // The guardrails (`api/guardrails.rs`).
  guardrailRulesNeeded: "add at least one rule",
  guardrailMatcherKind: "matcher must be keywords, regex or pii",
  guardrailPiiType:
    "unknown PII type; use EMAIL, PHONE, CREDIT_CARD, IBAN, US_SSN, IPV4, IPV6 or SECRET",
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

/**
 * A 400 of the gateway with this message. The code has several messages in
 * the gateway: the one of a body that is not valid is `errors.bad_request`,
 * and a handler has its own, such as "Send at least one of base_url and api_key."
 */
export function badRequest(message: string): GatewayError {
  return error(400, "bad_request", message);
}

export const errors = {
  // The general ones (`api/mod.rs`).
  bad_request: badRequest(
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
  setup_code_invalid: error(
    403,
    "setup_code_invalid",
    "The setup code is missing or wrong. It is printed in the gateway's log when it starts.",
  ),
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
  admin_target: error(
    409,
    "admin_target",
    "Admins get a password through their own account, not a link.",
  ),
  not_sso_user: error(
    409,
    "not_sso_user",
    "This user signs in with a password. They can change it in their account.",
  ),
  has_password: error(409, "has_password", "This user has a password already."),
  not_active: error(409, "not_active", "Only an active user can get a password link."),
  user_exists: error(409, "user_exists", "A user with this email already exists."),
  user_not_found: error(404, "user_not_found", "No active user with that email."),
  already_member: error(409, "already_member", "Already in this team."),
  user_disabled: error(409, "user_disabled", "A disabled user cannot be added to a team."),
  team_exists: error(409, "team_exists", "A team with this name already exists."),
  provider_exists: error(409, "provider_exists", "A provider with this name already exists."),
  model_exists: error(
    409,
    "model_exists",
    "This provider already has a model of this name.",
  ),
  route_exists: error(409, "route_exists", "A route of this name already exists."),
  alert_channel_exists: error(
    409,
    "alert_channel_exists",
    "An alert channel with this name already exists.",
  ),
  alert_rule_exists: error(409, "alert_rule_exists", "An alert rule with this name already exists."),
  guardrail_exists: error(409, "guardrail_exists", "A guardrail with this name already exists."),
  prompt_exists: error(409, "prompt_exists", "A prompt template with this name already exists."),
  export_blocked: error(
    409,
    "export_blocked",
    "The export was not made: a version of the prompt template(s) 'welcome' cannot be read, so the file would leave them out. Fix or delete them first.",
  ),
  sync_unsupported: error(422, "sync_unsupported", "Add Azure deployments as models by name."),
  sync_failed: error(502, "sync_failed", "The provider did not return its models."),
} as const satisfies Record<string, GatewayError>;

export type ErrorName = keyof typeof errors;

/**
 * What the shared `/v1` pipeline answers when the playground's call is
 * refused: the OpenAI shape (`crates/gateway/src/proxy.rs`, `errors.rs`),
 * not the `/api` one. `retryAfter` is the `Retry-After` header, in seconds.
 */
export interface PipelineError {
  readonly status: number;
  readonly body: { error: { message: string; type: string; param: null; code: string | null } };
  readonly retryAfter?: number;
}

function pipelineError(
  status: number,
  type: string,
  message: string,
  extra: { code?: string; retryAfter?: number } = {},
): PipelineError {
  return {
    status,
    body: { error: { message, type, param: null, code: extra.code ?? null } },
    ...(extra.retryAfter === undefined ? {} : { retryAfter: extra.retryAfter }),
  };
}

export const pipelineErrors = {
  forbidden: pipelineError(403, "permission_error", "You do not have access to model 'openai/gpt-4o'."),
  unknown: pipelineError(404, "not_found_error", "Unknown model 'nothing'."),
  rateLimited: pipelineError(
    429,
    "rate_limit_error",
    "rate limit 'requests per minute' of user 'lena@example.com' reached",
    { retryAfter: 30 },
  ),
  budget: pipelineError(
    429,
    "rate_limit_error",
    "budget 'monthly' of user 'lena@example.com' reached",
    { code: "budget_exceeded", retryAfter: 7200 },
  ),
  unavailable: pipelineError(503, "upstream_error", "No provider could serve this request."),
  tooLarge: pipelineError(413, "invalid_request_error", "The audio file is too large."),
  guardrail: pipelineError(400, "invalid_request_error", "Blocked by guardrail 'house-rules'.", {
    code: "guardrail_blocked",
  }),
} as const satisfies Record<string, PipelineError>;
