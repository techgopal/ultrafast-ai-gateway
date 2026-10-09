// The gateway of the tests: MSW answers every operation of `/api` from the
// fixtures. A test changes the answer of one operation with `override`.
import { http, HttpResponse, type JsonBodyType } from "msw";
import { setupServer } from "msw/node";
import type { Method, PathFor, ResponseOf } from "@/api/client";
import type { components } from "@/api/schema";
import { errors, type GatewayError, type PipelineError } from "./errors";
import * as fixtures from "./fixtures";

type ApiErrorBody = components["schemas"]["ApiErrorBody"];

export interface Call {
  request: Request;
  /** The path parameters, as the URL has them. */
  params: Readonly<Record<string, string>>;
}

type Resolver = (call: Call) => Response | Promise<Response>;

/** The gateway's answer for an error of `./errors`. */
export function refuse(error: GatewayError): Response {
  const body: ApiErrorBody = error.body;
  return HttpResponse.json(body, { status: error.status });
}

/** A success of the operation; the body has the type the API description gives. */
export function ok<M extends Method, P extends PathFor<M>>(
  _method: M,
  _path: P,
  status: 200 | 201,
  body: ResponseOf<P, M> & JsonBodyType,
): Response {
  return HttpResponse.json(body, { status });
}

/** The refusal of the shared pipeline, in the OpenAI shape, with its `Retry-After`. */
export function refusePipeline(error: PipelineError): Response {
  return HttpResponse.json(error.body, {
    status: error.status,
    headers: error.retryAfter === undefined ? {} : { "retry-after": String(error.retryAfter) },
  });
}

/** An answer of server-sent events, in the chunks given: each is one piece of the body. */
export function eventStream(chunks: readonly string[]): Response {
  const encoder = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
      controller.close();
    },
  });
  return new Response(body, { headers: { "content-type": "text/event-stream" } });
}

export function noContent(): Response {
  return new HttpResponse(null, { status: 204 });
}

/** The answer of a gateway that cannot be reached. */
export function networkFailure(): Response {
  return HttpResponse.error();
}

const notFound = () => refuse(errors.not_found);

function handler<M extends Method>(method: M, path: PathFor<M>, resolver: Resolver) {
  // `{id}` in the API description is `:id` for MSW.
  const pattern = path.replace(/\{([^}]+)\}/g, ":$1");
  return http[method](pattern, ({ request, params }) => {
    const given: Record<string, string> = {};
    for (const [name, value] of Object.entries(params)) {
      if (typeof value === "string") given[name] = value;
    }
    return resolver({ request, params: given });
  });
}

function byId<T extends { id: number }>(list: readonly T[], call: Call): T | undefined {
  const id = Number(call.params.id);
  return list.find((item) => item.id === id);
}

export const handlers = [
  // auth
  handler("get", "/api/setup", () => ok("get", "/api/setup", 200, { needs_setup: false })),
  handler("post", "/api/setup", () => ok("post", "/api/setup", 201, fixtures.users.maya)),
  handler("post", "/api/auth/login", () =>
    ok("post", "/api/auth/login", 200, {
      user: fixtures.users.maya,
      csrf_token: fixtures.csrfToken,
    }),
  ),
  handler("post", "/api/auth/logout", noContent),
  handler("get", "/api/auth/me", () => ok("get", "/api/auth/me", 200, fixtures.me.maya)),
  handler("get", "/api/auth/methods", () =>
    ok("get", "/api/auth/methods", 200, fixtures.signInMethods.passwordOnly),
  ),
  handler("post", "/api/auth/accept-invite", noContent),
  handler("post", "/api/auth/password", noContent),

  // users
  handler("get", "/api/users", () => ok("get", "/api/users", 200, { users: fixtures.userList })),
  handler("post", "/api/users", () =>
    ok("post", "/api/users", 201, {
      user: fixtures.users.sam,
      invite_link: fixtures.newInviteLink,
    }),
  ),
  handler("get", "/api/users/{id}", (call) => {
    const user = byId(fixtures.userList, call);
    return user === undefined ? notFound() : ok("get", "/api/users/{id}", 200, user);
  }),
  handler("patch", "/api/users/{id}", (call) => {
    const user = byId(fixtures.userList, call);
    return user === undefined ? notFound() : ok("patch", "/api/users/{id}", 200, user);
  }),
  handler("delete", "/api/users/{id}", (call) =>
    byId(fixtures.userList, call) === undefined ? notFound() : noContent(),
  ),
  handler("post", "/api/users/{id}/invite", (call) =>
    byId(fixtures.userList, call) === undefined
      ? notFound()
      : ok("post", "/api/users/{id}/invite", 201, { invite_link: fixtures.newInviteLink }),
  ),

  // teams
  handler("get", "/api/teams", () => ok("get", "/api/teams", 200, { teams: fixtures.teamList })),
  handler("post", "/api/teams", () => ok("post", "/api/teams", 201, fixtures.teams.growth)),
  handler("get", "/api/teams/{id}", (call) => {
    const id = Number(call.params.id);
    const detail = fixtures.teamDetailList.find((d) => d.team.id === id);
    return detail === undefined ? notFound() : ok("get", "/api/teams/{id}", 200, detail);
  }),
  handler("patch", "/api/teams/{id}", (call) => {
    const team = byId(fixtures.teamList, call);
    return team === undefined ? notFound() : ok("patch", "/api/teams/{id}", 200, team);
  }),
  handler("delete", "/api/teams/{id}", (call) =>
    byId(fixtures.teamList, call) === undefined ? notFound() : noContent(),
  ),
  handler("post", "/api/teams/{id}/members", async (call) => {
    if (byId(fixtures.teamList, call) === undefined) return notFound();
    const body: unknown = await call.request.json();
    const email: unknown = typeof body === "object" && body !== null ? Reflect.get(body, "email") : "";
    const user = fixtures.userList.find((one) => one.email === email && one.status === "active");
    if (user === undefined) return refuse(errors.user_not_found);
    return ok("post", "/api/teams/{id}/members", 201, {
      user_id: user.id,
      email: user.email,
      name: user.name,
      role: "member",
    });
  }),
  handler("put", "/api/teams/{id}/members/{user_id}", (call) =>
    byId(fixtures.teamList, call) === undefined ? notFound() : noContent(),
  ),
  handler("delete", "/api/teams/{id}/members/{user_id}", (call) =>
    byId(fixtures.teamList, call) === undefined ? notFound() : noContent(),
  ),

  // keys
  handler("get", "/api/keys", () => ok("get", "/api/keys", 200, { keys: fixtures.keyList })),
  handler("post", "/api/keys", () =>
    ok("post", "/api/keys", 201, { key: fixtures.keys.active, secret: fixtures.newKeySecret }),
  ),
  handler("get", "/api/keys/{id}", (call) => {
    const key = byId(fixtures.keyList, call);
    return key === undefined ? notFound() : ok("get", "/api/keys/{id}", 200, key);
  }),
  handler("patch", "/api/keys/{id}", (call) => {
    const key = byId(fixtures.keyList, call);
    return key === undefined ? notFound() : ok("patch", "/api/keys/{id}", 200, key);
  }),
  handler("delete", "/api/keys/{id}", (call) =>
    byId(fixtures.keyList, call) === undefined ? notFound() : noContent(),
  ),

  // providers
  handler("get", "/api/providers", () =>
    ok("get", "/api/providers", 200, { providers: fixtures.providerList }),
  ),
  handler("post", "/api/providers", () =>
    ok("post", "/api/providers", 201, fixtures.providers.withCredential),
  ),
  handler("patch", "/api/providers/{id}", (call) => {
    const provider = byId(fixtures.providerList, call);
    return provider === undefined
      ? notFound()
      : ok("patch", "/api/providers/{id}", 200, provider);
  }),
  handler("delete", "/api/providers/{id}", (call) =>
    byId(fixtures.providerList, call) === undefined ? notFound() : noContent(),
  ),

  handler("post", "/api/providers/{id}/sync", (call) =>
    byId(fixtures.providerList, call) === undefined
      ? notFound()
      : ok("post", "/api/providers/{id}/sync", 200, fixtures.syncResult),
  ),

  // models
  handler("get", "/api/models", () =>
    ok("get", "/api/models", 200, { models: fixtures.modelList }),
  ),
  handler("post", "/api/models", () => ok("post", "/api/models", 201, fixtures.models.openaiMini)),
  handler("patch", "/api/models/{id}", (call) => {
    const model = byId(fixtures.modelList, call);
    return model === undefined ? notFound() : ok("patch", "/api/models/{id}", 200, model);
  }),
  handler("put", "/api/models/{id}/grants", (call) => {
    const model = byId(fixtures.modelList, call);
    return model === undefined ? notFound() : ok("put", "/api/models/{id}/grants", 200, model);
  }),
  handler("delete", "/api/models/{id}", (call) =>
    byId(fixtures.modelList, call) === undefined ? notFound() : noContent(),
  ),

  // routes
  handler("get", "/api/routes", () =>
    ok("get", "/api/routes", 200, { routes: fixtures.routeList }),
  ),
  handler("post", "/api/routes", () => ok("post", "/api/routes", 201, fixtures.routes.support)),
  handler("get", "/api/routes/{id}", (call) => {
    const route = byId(fixtures.routeList, call);
    return route === undefined ? notFound() : ok("get", "/api/routes/{id}", 200, route);
  }),
  handler("put", "/api/routes/{id}", (call) => {
    const route = byId(fixtures.routeList, call);
    return route === undefined ? notFound() : ok("put", "/api/routes/{id}", 200, route);
  }),
  handler("delete", "/api/routes/{id}", (call) =>
    byId(fixtures.routeList, call) === undefined ? notFound() : noContent(),
  ),
  handler("get", "/api/routing/health", () =>
    ok("get", "/api/routing/health", 200, { targets: fixtures.healthList }),
  ),

  // tokens
  handler("get", "/api/tokens", () =>
    ok("get", "/api/tokens", 200, { tokens: fixtures.tokenList }),
  ),
  handler("post", "/api/tokens", () =>
    ok("post", "/api/tokens", 201, {
      token: fixtures.tokens.active,
      secret: fixtures.newTokenSecret,
    }),
  ),
  handler("delete", "/api/tokens/{id}", (call) =>
    byId(fixtures.tokenList, call) === undefined ? notFound() : noContent(),
  ),

  // limits, budgets and settings
  handler("get", "/api/limits", () => ok("get", "/api/limits", 200, { limits: fixtures.limitList })),
  handler("put", "/api/limits", () => ok("put", "/api/limits", 200, fixtures.limits.gateway)),
  handler("delete", "/api/limits/{id}", (call) =>
    byId(fixtures.limitList, call) === undefined ? notFound() : noContent(),
  ),
  handler("get", "/api/budgets", () =>
    ok("get", "/api/budgets", 200, { budgets: fixtures.budgetList }),
  ),
  handler("put", "/api/budgets", () => ok("put", "/api/budgets", 200, fixtures.budgets.gateway)),
  handler("delete", "/api/budgets/{id}", (call) =>
    byId(fixtures.budgetList, call) === undefined ? notFound() : noContent(),
  ),
  handler("get", "/api/settings", () => ok("get", "/api/settings", 200, fixtures.settings)),
  handler("patch", "/api/settings", () => ok("patch", "/api/settings", 200, fixtures.settings)),
  handler("get", "/api/settings/oidc", () =>
    ok("get", "/api/settings/oidc", 200, fixtures.oidc.fresh),
  ),
  handler("put", "/api/settings/oidc", () =>
    ok("put", "/api/settings/oidc", 200, fixtures.oidc.configured),
  ),
  handler("post", "/api/settings/oidc/test", () =>
    ok("post", "/api/settings/oidc/test", 200, {
      ok: true,
      issuer: "https://idp.example.test",
      jwks_keys: 2,
    }),
  ),

  // logs and usage
  handler("get", "/api/logs", () => ok("get", "/api/logs", 200, { logs: fixtures.logList })),
  handler("get", "/api/logs/{id}", (call) => {
    const detail = fixtures.logDetail(Number(call.params.id));
    return detail === undefined ? notFound() : ok("get", "/api/logs/{id}", 200, detail);
  }),
  handler("get", "/api/usage", ({ request }) =>
    ok(
      "get",
      "/api/usage",
      200,
      fixtures.usageOf(new URL(request.url).searchParams.get("group") ?? "day"),
    ),
  ),

  // playground
  handler("post", "/api/playground/chat", () => eventStream(fixtures.playgroundChunks)),
  handler("post", "/api/playground/images", () =>
    ok("post", "/api/playground/images", 200, fixtures.playgroundImages),
  ),

  // configuration and backup
  handler("get", "/api/config/export", () =>
    ok("get", "/api/config/export", 200, fixtures.configFile),
  ),
  handler("post", "/api/config/import", () =>
    ok("post", "/api/config/import", 200, fixtures.importReports.changes),
  ),
  handler(
    "get",
    "/api/backup",
    () =>
      new Response(new Uint8Array([83, 81, 76, 105, 116, 101]), {
        headers: { "content-type": "application/vnd.sqlite3" },
      }),
  ),

  // alerts
  handler("get", "/api/alerts/channels", () =>
    ok("get", "/api/alerts/channels", 200, { channels: fixtures.alertChannelList }),
  ),
  handler("post", "/api/alerts/channels", () =>
    ok("post", "/api/alerts/channels", 201, {
      channel: fixtures.alertChannels.ops,
      secret: fixtures.newChannelSecret,
    }),
  ),
  handler("patch", "/api/alerts/channels/{id}", (call) => {
    const channel = byId(fixtures.alertChannelList, call);
    return channel === undefined
      ? notFound()
      : ok("patch", "/api/alerts/channels/{id}", 200, channel);
  }),
  handler("delete", "/api/alerts/channels/{id}", (call) =>
    byId(fixtures.alertChannelList, call) === undefined ? notFound() : noContent(),
  ),
  handler("post", "/api/alerts/channels/{id}/rotate-secret", (call) =>
    byId(fixtures.alertChannelList, call) === undefined
      ? notFound()
      : ok("post", "/api/alerts/channels/{id}/rotate-secret", 200, {
          secret: fixtures.rotatedChannelSecret,
        }),
  ),
  handler("post", "/api/alerts/channels/{id}/test", (call) =>
    byId(fixtures.alertChannelList, call) === undefined
      ? notFound()
      : ok("post", "/api/alerts/channels/{id}/test", 200, { ok: true, status: 200, error: null }),
  ),
  handler("get", "/api/alerts/rules", () =>
    ok("get", "/api/alerts/rules", 200, { rules: fixtures.alertRuleList }),
  ),
  handler("post", "/api/alerts/rules", () =>
    ok("post", "/api/alerts/rules", 201, fixtures.alertRules.budget),
  ),
  handler("patch", "/api/alerts/rules/{id}", (call) => {
    const rule = byId(fixtures.alertRuleList, call);
    return rule === undefined ? notFound() : ok("patch", "/api/alerts/rules/{id}", 200, rule);
  }),
  handler("delete", "/api/alerts/rules/{id}", (call) =>
    byId(fixtures.alertRuleList, call) === undefined ? notFound() : noContent(),
  ),
  handler("get", "/api/alerts/events", () =>
    ok("get", "/api/alerts/events", 200, { events: fixtures.alertEventList }),
  ),

  // guardrails
  handler("get", "/api/guardrails", () =>
    ok("get", "/api/guardrails", 200, { guardrails: fixtures.guardrailList }),
  ),
  handler("post", "/api/guardrails", () =>
    ok("post", "/api/guardrails", 201, {
      guardrail: fixtures.guardrails.external,
      secret: fixtures.newGuardrailSecret,
    }),
  ),
  // Declared before `{id}`: "test" is no id.
  handler("post", "/api/guardrails/test", () =>
    ok("post", "/api/guardrails/test", 200, {
      redacted_text: "Write to [REDACTED:EMAIL].",
      outcome: { blocked_by: null, flags: [], redactions: { EMAIL: 1 } },
    }),
  ),
  handler("get", "/api/guardrails/{id}", (call) => {
    const one = byId(fixtures.guardrailList, call);
    return one === undefined ? notFound() : ok("get", "/api/guardrails/{id}", 200, one);
  }),
  handler("patch", "/api/guardrails/{id}", (call) => {
    const one = byId(fixtures.guardrailList, call);
    return one === undefined ? notFound() : ok("patch", "/api/guardrails/{id}", 200, one);
  }),
  handler("delete", "/api/guardrails/{id}", (call) =>
    byId(fixtures.guardrailList, call) === undefined ? notFound() : noContent(),
  ),
  handler("post", "/api/guardrails/{id}/rotate-secret", (call) =>
    byId(fixtures.guardrailList, call) === undefined
      ? notFound()
      : ok("post", "/api/guardrails/{id}/rotate-secret", 200, {
          secret: fixtures.rotatedGuardrailSecret,
        }),
  ),

  // audit
  handler("get", "/api/audit", () =>
    ok("get", "/api/audit", 200, { entries: fixtures.auditEntries }),
  ),
];

export const server = setupServer(...handlers);

/** Until the end of the test, the operation is answered by `resolver`. */
export function override<M extends Method>(method: M, path: PathFor<M>, resolver: Resolver): void {
  server.use(handler(method, path, resolver));
}
