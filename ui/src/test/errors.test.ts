// @vitest-environment node
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import {
  badRequest,
  errors,
  fieldMessages,
  pipelineErrors,
  validationFailed,
  type ErrorName,
} from "./errors";

/** The status of every code, as the gateway source has it. */
const statuses: Record<ErrorName, number> = {
  bad_request: 400,
  validation_failed: 422,
  unauthenticated: 401,
  invalid_credentials: 401,
  csrf_failed: 403,
  forbidden: 403,
  setup_code_invalid: 403,
  not_found: 404,
  method_not_allowed: 405,
  payload_too_large: 413,
  too_many_attempts: 429,
  internal_error: 500,
  already_set_up: 409,
  last_admin: 409,
  cannot_delete_self: 409,
  no_password: 409,
  not_invited: 409,
  user_exists: 409,
  user_not_found: 404,
  already_member: 409,
  user_disabled: 409,
  team_exists: 409,
  provider_exists: 409,
  model_exists: 409,
  route_exists: 409,
  alert_channel_exists: 409,
  alert_rule_exists: 409,
  guardrail_exists: 409,
  sync_unsupported: 422,
  sync_failed: 502,
};

const all = Object.entries(errors);

test("there is a fixture for every code of the table", () => {
  expect(Object.keys(errors).sort()).toEqual(Object.keys(statuses).sort());
});

test.each(all)("%s has the form of a code, and is named by it", (name, fixture) => {
  expect(fixture.body.error.code).toMatch(/^[a-z]+(_[a-z]+)*$/);
  expect(fixture.body.error.code).toBe(name);
});

test.each(all)("%s has the status the gateway gives it", (name, fixture) => {
  expect(fixture.status).toBe(statuses[name as ErrorName]);
});

test("no two fixtures share a code", () => {
  const codes = all.map(([, fixture]) => fixture.body.error.code);
  expect(new Set(codes).size).toBe(codes.length);
});

test.each(all)("the message of %s is a sentence", (_, fixture) => {
  expect(fixture.body.error.message.trim()).not.toBe("");
  expect(fixture.body.error.message).toMatch(/\.$/);
});

test("only validation_failed names fields, with messages of the gateway", () => {
  for (const [name, fixture] of all) {
    const fields: unknown = Reflect.get(fixture.body.error, "fields");
    if (name !== "validation_failed") {
      expect(fields).toBeUndefined();
      continue;
    }
    expect(fields).toEqual({
      email: "email is not valid",
      name: "name must be 1 to 100 characters",
    });
  }
});

test("a validation error made for a test has the gateway's status, code and message", () => {
  const made = validationFailed({ password: fieldMessages.password });
  expect(made.status).toBe(422);
  expect(made.body.error).toEqual({
    code: "validation_failed",
    message: "Some fields are not valid.",
    fields: { password: "password must be at least 12 characters" },
  });
});

test("a bad request made for a test has the gateway's status and code, and the message given", () => {
  const made = badRequest("Send at least one of base_url and api_key.");
  expect(made.status).toBe(400);
  expect(made.body.error).toEqual({
    code: "bad_request",
    message: "Send at least one of base_url and api_key.",
  });
  expect(Reflect.get(made.body.error, "fields")).toBeUndefined();
  // With the message of a body that is not valid, it is the fixture of the code.
  expect(badRequest(errors.bad_request.body.error.message)).toEqual(errors.bad_request);
});

test("the messages of the name and of the base URL of a provider are the gateway's", () => {
  expect({
    providerName: fieldMessages.providerName,
    baseUrl: fieldMessages.baseUrl,
    baseUrlWhitespace: fieldMessages.baseUrlWhitespace,
    baseUrlQuery: fieldMessages.baseUrlQuery,
    baseUrlFragment: fieldMessages.baseUrlFragment,
    baseUrlCredentials: fieldMessages.baseUrlCredentials,
    baseUrlHost: fieldMessages.baseUrlHost,
  }).toEqual({
    providerName:
      "provider name must be 1 to 40 characters of a-z, 0-9, '-' and '_', starting with a letter or a digit",
    baseUrl: "base URL must start with http:// or https://",
    baseUrlWhitespace: "base URL must not contain whitespace",
    baseUrlQuery: "base URL must not contain a query string",
    baseUrlFragment: "base URL must not contain a fragment",
    baseUrlCredentials:
      "base URL must not contain credentials; give the key with --api-key or UF_PROVIDER_API_KEY",
    baseUrlHost: "base URL must include a host",
  });
});

test("every message of a provider's name and base URL is in the gateway source", () => {
  const path = fileURLToPath(new URL("../../../crates/gateway/src/config.rs", import.meta.url));
  // A Rust string goes on in the next line after a backslash.
  const source = readFileSync(path, "utf8").replace(/\\\n\s*/g, "");
  const names = Object.keys(fieldMessages).filter(
    (name) => name === "providerName" || name.startsWith("baseUrl"),
  );
  expect(names).toHaveLength(7);
  for (const name of names) {
    expect(source).toContain(`"${String(Reflect.get(fieldMessages, name))}"`);
  }
});

test("the messages of the tags are in the gateway source", () => {
  const path = fileURLToPath(new URL("../../../crates/gateway/src/tags.rs", import.meta.url));
  expect(readFileSync(path, "utf8")).toContain(`"${fieldMessages.tags}"`);
});

test("the messages of the models are in the gateway source", () => {
  const dir = fileURLToPath(new URL("../../../crates/gateway/src/", import.meta.url));
  const source = ["api/models.rs", "catalog/mod.rs"]
    .map((file) => readFileSync(dir + file, "utf8"))
    .join("\n");
  const messages = [
    fieldMessages.modelName,
    fieldMessages.modelNameWhitespace,
    fieldMessages.providerMissing,
    fieldMessages.grantsEveryone,
    fieldMessages.teamsMissing,
    fieldMessages.usersMissing,
    errors.model_exists.body.error.message,
    errors.sync_unsupported.body.error.message,
    errors.sync_failed.body.error.message,
  ];
  for (const message of messages) expect(source).toContain(`"${message}`);
});

test("the messages of the routes are in the gateway source", () => {
  const path = fileURLToPath(new URL("../../../crates/gateway/src/api/routes.rs", import.meta.url));
  // A Rust string goes on in the next line after a quote and a break.
  const source = readFileSync(path, "utf8").replace(/\n\s*/g, "");
  const messages = [
    fieldMessages.routeName,
    fieldMessages.routePrimariesNeeded,
    fieldMessages.routeWeight,
    fieldMessages.routeOnce,
    fieldMessages.routeModelMissing,
    fieldMessages.routeTeamMissing,
    fieldMessages.routeTotalBelowFirst,
    fieldMessages.grantsEveryone,
    errors.route_exists.body.error.message,
  ];
  for (const message of messages) expect(source).toContain(`"${message}`);
  // The range messages are made from the limits: "must be {lo} to {hi}".
  expect(source).toContain('"must be {lo} to {hi}"');
  expect(fieldMessages.routeRetries).toBe("must be 0 to 5");
  expect(fieldMessages.routeFirstToken).toBe("must be 1000 to 300000");
});

test("the messages of the alerts are in the gateway source", () => {
  const dir = fileURLToPath(new URL("../../../crates/gateway/src/", import.meta.url));
  const source = ["api/alerts.rs", "config.rs"]
    .map((file) => readFileSync(dir + file, "utf8"))
    .join("\n");
  for (const message of [
    fieldMessages.channelUrlScheme,
    fieldMessages.channelUrlInvalid,
    fieldMessages.channelMissing,
    fieldMessages.needsUrl,
    fieldMessages.budgetMissing,
    errors.alert_channel_exists.body.error.message,
    errors.alert_rule_exists.body.error.message,
  ]) {
    expect(source).toContain(`"${message}`);
  }
});

test("the messages of team members, key allowlists and the API version are in the gateway source", () => {
  const dir = fileURLToPath(new URL("../../../crates/gateway/src/", import.meta.url));
  const source = ["api/teams.rs", "api/keys.rs", "api/providers.rs", "config.rs"]
    .map((file) => readFileSync(dir + file, "utf8"))
    .join("\n");
  for (const message of [
    errors.user_not_found.body.error.message,
    errors.already_member.body.error.message,
    fieldMessages.providerKind,
    fieldMessages.apiVersionForm,
    fieldMessages.apiVersionKind,
  ]) {
    expect(source).toContain(`"${message}`);
  }
  // The message about a name is made with the name: `'{name}' is not a model or route you can use`.
  expect(source).toContain("'{name}' is not a model or route you can use");
  expect(fieldMessages.allowedHidden).toBe("'gpt-secret' is not a model or route you can use");
});

test("the refusals of the pipeline are in the gateway source, in the OpenAI shape", () => {
  const dir = fileURLToPath(new URL("../../../crates/gateway/src/", import.meta.url));
  const source = ["proxy.rs", "errors.rs", "limits/mod.rs", "budgets/mod.rs"]
    .map((file) => readFileSync(dir + file, "utf8"))
    .join("\n");
  // The messages are made with the name asked for, or of the limit.
  expect(source).toContain("You do not have access to model '{model}'.");
  expect(source).toContain("Unknown model '{model}'.");
  expect(source).toContain("rate limit '{}' of {} reached");
  expect(source).toContain("budget '{}' of {} reached");
  expect(source).toContain('const NO_PROVIDER: &str = "No provider could serve this request."');
  const kinds = Object.values(pipelineErrors).map((e) => [e.status, e.body.error.type]);
  expect(kinds).toEqual([
    [403, "permission_error"],
    [404, "not_found_error"],
    [429, "rate_limit_error"],
    [429, "rate_limit_error"],
    [503, "upstream_error"],
    [400, "invalid_request_error"],
  ]);
  for (const e of Object.values(pipelineErrors)) {
    expect(Object.keys(e.body.error)).toEqual(["message", "type", "param", "code"]);
  }
  expect(pipelineErrors.budget.body.error.code).toBe("budget_exceeded");
  // The message names the guardrail and never what matched (`errors.rs`, `guardrail_blocked`).
  expect(source).toContain("Blocked by guardrail '{guardrail}'.");
  expect(pipelineErrors.guardrail.body.error.code).toBe("guardrail_blocked");
});

test("the messages of guardrails are in the gateway source", () => {
  const path = fileURLToPath(new URL("../../../crates/gateway/src/api/guardrails.rs", import.meta.url));
  const source = readFileSync(path, "utf8");
  for (const message of [
    fieldMessages.guardrailRulesNeeded,
    fieldMessages.guardrailMatcherKind,
    fieldMessages.guardrailPiiType,
    errors.guardrail_exists.body.error.message,
  ]) {
    expect(source).toContain(`"${message}`);
  }
});

test("the message of a wrong setup code is in the gateway source", () => {
  const path = fileURLToPath(new URL("../../../crates/gateway/src/api/mod.rs", import.meta.url));
  expect(readFileSync(path, "utf8")).toContain(`"${errors.setup_code_invalid.body.error.message}"`);
});
