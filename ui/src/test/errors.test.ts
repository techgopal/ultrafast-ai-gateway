// @vitest-environment node
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import {
  badRequest,
  errors,
  fieldMessages,
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
  user_disabled: 409,
  team_exists: 409,
  provider_exists: 409,
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
