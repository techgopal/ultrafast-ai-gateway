import { describe, expect, expectTypeOf, test, vi } from "vitest";
import * as fixtures from "@/test/fixtures";
import { errors, fieldMessages, validationFailed, type GatewayError } from "@/test/errors";
import { networkFailure, noContent, override, refuse } from "@/test/handlers";
import { api, onUnauthenticated, playgroundChat, setCsrfToken, type ResponseOf } from "./client";
import { ApiError, NetworkError, SessionOverError } from "./errors";
import type { components } from "./schema";

type Schemas = components["schemas"];

async function failure(call: Promise<unknown>): Promise<unknown> {
  try {
    await call;
  } catch (error) {
    return error;
  }
  throw new Error("The call did not fail.");
}

async function apiFailure(call: Promise<unknown>): Promise<ApiError> {
  const error = await failure(call);
  if (!(error instanceof ApiError)) throw new Error("The call did not fail with an ApiError.");
  return error;
}

/** Everything an error carries, own properties and message, as one text. */
function everythingIn(error: Error): string {
  const own = Object.fromEntries(
    Object.getOwnPropertyNames(error).map((name) => [name, Reflect.get(error, name) as unknown]),
  );
  return JSON.stringify(own);
}

const unauthenticated = () => refuse(errors.unauthenticated);

describe("responses", () => {
  test("get resolves typed data", async () => {
    const list = await api.get("/api/teams");
    expect(list).toEqual({ teams: fixtures.teamList });
    expectTypeOf(list).toEqualTypeOf<Schemas["TeamList"]>();
  });

  test("the types of a path and method are those of that operation", () => {
    expectTypeOf<ResponseOf<"/api/teams", "get">>().toEqualTypeOf<Schemas["TeamList"]>();
    expectTypeOf<ResponseOf<"/api/audit", "get">>().toEqualTypeOf<Schemas["AuditPage"]>();
    expectTypeOf<ResponseOf<"/api/keys", "get">>().toEqualTypeOf<Schemas["KeyList"]>();
    expectTypeOf<ResponseOf<"/api/keys", "post">>().toEqualTypeOf<Schemas["CreatedKey"]>();
    expectTypeOf<ResponseOf<"/api/users/{id}", "get">>().toEqualTypeOf<Schemas["UserView"]>();
    expectTypeOf<ResponseOf<"/api/auth/me", "get">>().toEqualTypeOf<Schemas["MeResponse"]>();
    expectTypeOf<ResponseOf<"/api/providers", "get">>().toEqualTypeOf<Schemas["ProviderList"]>();
    expectTypeOf<ResponseOf<"/api/tokens", "post">>().toEqualTypeOf<Schemas["CreatedToken"]>();
    expectTypeOf<ResponseOf<"/api/users", "post">>().toEqualTypeOf<Schemas["InviteResponse"]>();
    expectTypeOf<ResponseOf<"/api/teams/{id}", "delete">>().toEqualTypeOf<undefined>();
    expectTypeOf(api.patch<"/api/users/{id}">)
      .parameter(1)
      .toHaveProperty("body")
      .toEqualTypeOf<Schemas["UpdateRequest"]>();
    expectTypeOf(api.patch<"/api/providers/{id}">)
      .parameter(1)
      .toHaveProperty("body")
      .toEqualTypeOf<Schemas["UpdateProviderRequest"]>();
  });

  test("204 resolves undefined", async () => {
    await expect(api.delete("/api/teams/{id}", { params: { id: 1 } })).resolves.toBeUndefined();
  });

  test("a success with no body resolves undefined", async () => {
    override("get", "/api/teams", () => new Response(null, { status: 200 }));
    await expect(api.get("/api/teams")).resolves.toBeUndefined();
  });

  test("a success that is not JSON is an unexpected response", async () => {
    override("get", "/api/teams", () => new Response("<html>hello</html>", { status: 200 }));
    const error = await apiFailure(api.get("/api/teams"));
    expect(error.code).toBe("unexpected_response");
    expect(error.message).not.toContain("<");
  });
});

describe("requests", () => {
  test("csrf header on writes only", async () => {
    const seen: [string, string | null][] = [];
    const note = ({ request }: { request: Request }) => {
      seen.push([request.method, request.headers.get("x-csrf-token")]);
      return noContent();
    };
    override("get", "/api/teams", note);
    override("post", "/api/teams", note);
    override("put", "/api/teams/{id}/members/{user_id}", note);
    override("patch", "/api/teams/{id}", note);
    override("delete", "/api/teams/{id}", note);

    const writes = async () => {
      await api.post("/api/teams", { body: { name: "Growth" } });
      await api.put("/api/teams/{id}/members/{user_id}", {
        params: { id: 1, user_id: 3 },
        body: { role: "member" },
      });
      await api.patch("/api/teams/{id}", { params: { id: 1 }, body: { name: "Growth" } });
      await api.delete("/api/teams/{id}", { params: { id: 1 } });
    };

    setCsrfToken(fixtures.csrfToken);
    await api.get("/api/teams");
    await writes();
    setCsrfToken(null);
    await api.get("/api/teams");
    await writes();

    expect(seen).toEqual([
      ["GET", null],
      ["POST", fixtures.csrfToken],
      ["PUT", fixtures.csrfToken],
      ["PATCH", fixtures.csrfToken],
      ["DELETE", fixtures.csrfToken],
      ["GET", null],
      ["POST", null],
      ["PUT", null],
      ["PATCH", null],
      ["DELETE", null],
    ]);
  });

  test.each([
    ["an empty text", ""],
    ["one dot", "."],
    ["two dots", ".."],
    ["not a number", Number.NaN],
    ["infinity", Number.POSITIVE_INFINITY],
  ])("a path parameter that is %s throws before any request", async (_, id) => {
    let calls = 0;
    override("get", "/api/teams/{id}", () => {
      calls += 1;
      return noContent();
    });
    // The description types the id as a number; the check is for what gets past the types.
    const params = { id } as unknown as { id: number };
    await expect(api.get("/api/teams/{id}", { params })).rejects.toThrow(/parameter "id"/);
    expect(calls).toBe(0);
  });

  test("headers, credentials and body", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const seen: { accept: string | null; type: string | null; auth: string | null; body: unknown }[] =
      [];
    const record = async ({ request }: { request: Request }) => {
      const text = await request.text();
      seen.push({
        accept: request.headers.get("accept"),
        type: request.headers.get("content-type"),
        auth: request.headers.get("authorization"),
        body: text === "" ? null : (JSON.parse(text) as unknown),
      });
      return noContent();
    };
    override("post", "/api/teams", record);
    override("get", "/api/teams", record);
    override("post", "/api/auth/logout", record);

    await api.post("/api/teams", { body: { name: "Growth" } });
    await api.get("/api/teams");
    await api.post("/api/auth/logout");

    expect(seen).toEqual([
      { accept: "application/json", type: "application/json", auth: null, body: { name: "Growth" } },
      { accept: "application/json", type: null, auth: null, body: null },
      { accept: "application/json", type: null, auth: null, body: null },
    ]);
    expect(fetchSpy).toHaveBeenCalledTimes(3);
    for (const [url, init] of fetchSpy.mock.calls) {
      expect(url).toMatch(/^\/api\//);
      expect(init?.credentials).toBe("same-origin");
    }
    fetchSpy.mockRestore();
  });

  test("path params are encoded", async () => {
    let path = "";
    override("get", "/api/teams/{id}", ({ request }) => {
      path = new URL(request.url).pathname;
      return noContent();
    });
    // The API takes a number. A text with a slash shows that the value is encoded.
    const id = "a/b" as unknown as number;
    await api.get("/api/teams/{id}", { params: { id } });
    expect(path).toBe("/api/teams/a%2Fb");
  });

  test("every path param is filled", async () => {
    let path = "";
    override("put", "/api/teams/{id}/members/{user_id}", ({ request }) => {
      path = new URL(request.url).pathname;
      return noContent();
    });
    await api.put("/api/teams/{id}/members/{user_id}", {
      params: { id: 1, user_id: 3 },
      body: { role: "lead" },
    });
    expect(path).toBe("/api/teams/1/members/3");
  });

  test("missing path param throws", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    // Code that the compiler did not check could leave the parameter out.
    const params = {} as { id: number };
    const error = await failure(api.get("/api/teams/{id}", { params }));
    expect(error).toBeInstanceOf(Error);
    expect(error).not.toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(NetworkError);
    expect((error as Error).message).toContain('"id"');
    const partly = { id: 1 } as { id: number; user_id: number };
    await failure(api.delete("/api/teams/{id}/members/{user_id}", { params: partly }));
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });

  test("query parameters are sent", async () => {
    let search = "";
    override("get", "/api/audit", ({ request }) => {
      search = new URL(request.url).search;
      return noContent();
    });
    await api.get("/api/audit", { query: { limit: 50, before: 120 } });
    expect(search).toBe("?limit=50&before=120");
    await api.get("/api/audit");
    expect(search).toBe("");
  });
});

describe("errors", () => {
  test("api error is parsed", async () => {
    override("post", "/api/teams", () => refuse(errors.team_exists));
    const error = await apiFailure(api.post("/api/teams", { body: { name: "Platform" } }));
    expect(error.status).toBe(409);
    expect(error.code).toBe("team_exists");
    expect(error.message).toBe("A team with this name already exists.");
    expect(error.fields).toEqual({});
  });

  test("field errors are kept", async () => {
    override("post", "/api/users", () =>
      refuse(validationFailed({ email: fieldMessages.email })),
    );
    const error = await apiFailure(
      api.post("/api/users", { body: { email: "x", name: "X", role: "member" } }),
    );
    expect(error.status).toBe(422);
    expect(error.code).toBe("validation_failed");
    expect(error.fields.email).toBe("email is not valid");
  });

  test("html error body is not shown", async () => {
    override(
      "get",
      "/api/teams",
      () =>
        new Response("<html><body><h1>502 Bad Gateway</h1>proxy-internal-name</body></html>", {
          status: 502,
          headers: { "Content-Type": "text/html" },
        }),
    );
    const error = await apiFailure(api.get("/api/teams"));
    expect(error.status).toBe(502);
    expect(error.code).toBe("unexpected_response");
    expect(error.message).toContain("502");
    expect(error.message).not.toContain("<");
    expect(everythingIn(error)).not.toContain("proxy-internal-name");
  });

  test.each([
    ["an empty body", ""],
    ["invalid JSON", "{not json"],
    ["JSON of another shape", JSON.stringify({ message: "other-shape-text" })],
    ["an error without a code", JSON.stringify({ error: { message: "other-shape-text" } })],
    ["a JSON array", "[1]"],
  ])("%s is an unexpected response", async (_, body) => {
    override("get", "/api/teams", () => new Response(body === "" ? null : body, { status: 500 }));
    const error = await apiFailure(api.get("/api/teams"));
    expect(error.status).toBe(500);
    expect(error.code).toBe("unexpected_response");
    expect(error.fields).toEqual({});
    expect(everythingIn(error)).not.toContain("other-shape-text");
  });

  test("network failure", async () => {
    override("get", "/api/teams", networkFailure);
    const error = await failure(api.get("/api/teams"));
    expect(error).toBeInstanceOf(NetworkError);
    expect((error as Error).message).toBe("Could not reach the gateway.");
  });

  test("errors keep nothing of the request or the response", async () => {
    const password = "correct-horse-battery";
    setCsrfToken(fixtures.csrfToken);
    override("post", "/api/auth/login", () =>
      withHeader(errors.invalid_credentials),
    );
    const refused = await apiFailure(
      api.post("/api/auth/login", { body: { email: "maya@example.test", password } }),
    );
    expect(Object.getOwnPropertyNames(refused).sort()).toEqual(
      ["code", "fields", "message", "name", "retryAfter", "stack", "status"].sort(),
    );
    expect(everythingIn(refused)).not.toContain(password);
    expect(everythingIn(refused)).not.toContain(fixtures.csrfToken);
    expect(everythingIn(refused)).not.toContain("header-value-of-the-response");
    expect(refused.cause).toBeUndefined();

    override("post", "/api/auth/login", networkFailure);
    const unreachable = await failure(
      api.post("/api/auth/login", { body: { email: "maya@example.test", password } }),
    );
    expect(unreachable).toBeInstanceOf(NetworkError);
    expect(Object.getOwnPropertyNames(unreachable).sort()).toEqual(["message", "name", "stack"]);
    expect((unreachable as Error).cause).toBeUndefined();
    expect(everythingIn(unreachable as Error)).not.toContain(password);
  });

  test("nothing is logged", async () => {
    const spies = (["log", "info", "warn", "error", "debug"] as const).map((level) =>
      vi.spyOn(console, level).mockImplementation(() => undefined),
    );
    override("post", "/api/teams", () => refuse(errors.team_exists));
    await api.get("/api/teams");
    await failure(api.post("/api/teams", { body: { name: "Platform" } }));
    override("get", "/api/teams", networkFailure);
    await failure(api.get("/api/teams"));
    for (const spy of spies) {
      expect(spy).not.toHaveBeenCalled();
      spy.mockRestore();
    }
  });
});

/** The error of the gateway, with a header the client must not keep. */
function withHeader(error: GatewayError): Response {
  return new Response(JSON.stringify(error.body), {
    status: error.status,
    headers: { "Content-Type": "application/json", "x-test": "header-value-of-the-response" },
  });
}

describe("the end of the session", () => {
  test("401 signs out once", async () => {
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    override("get", "/api/teams", unauthenticated);
    override("get", "/api/users", unauthenticated);

    const errors = await Promise.all([
      apiFailure(api.get("/api/teams")),
      apiFailure(api.get("/api/users")),
    ]);

    expect(handler).toHaveBeenCalledTimes(1);
    expect(errors.map((e) => e.status)).toEqual([401, 401]);
    expect(errors.map((e) => e.code)).toEqual(["unauthenticated", "unauthenticated"]);
    unsubscribe();
  });

  test("every handler is called, and none after it unsubscribed", async () => {
    const first = vi.fn();
    const second = vi.fn();
    const gone = vi.fn();
    const stops = [onUnauthenticated(first), onUnauthenticated(second)];
    onUnauthenticated(gone)();
    override("get", "/api/teams", unauthenticated);
    await failure(api.get("/api/teams"));
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
    expect(gone).not.toHaveBeenCalled();
    for (const stop of stops) stop();
  });

  test("a new session is told of its end again", async () => {
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    override("get", "/api/teams", unauthenticated);
    await failure(api.get("/api/teams"));
    await failure(api.get("/api/teams"));
    expect(handler).toHaveBeenCalledTimes(1);
    setCsrfToken(fixtures.csrfToken);
    await failure(api.get("/api/teams"));
    expect(handler).toHaveBeenCalledTimes(2);
    await failure(api.get("/api/teams"));
    expect(handler).toHaveBeenCalledTimes(2);
    unsubscribe();
  });

  test("only a token starts a new session: answers and a cleared token do not", async () => {
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    override("get", "/api/teams", unauthenticated);
    await failure(api.get("/api/teams"));
    expect(handler).toHaveBeenCalledTimes(1);
    // An answer of the session that ended may still arrive.
    await api.get("/api/auth/me");
    await api.post("/api/auth/login", { body: { email: "a@example.test", password: "x" } });
    setCsrfToken(null);
    await failure(api.get("/api/teams"));
    expect(handler).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  test.each([
    ["a 401", unauthenticated],
    ["a 200", () => noContent()],
    ["a 500", () => refuse(errors.internal_error)],
  ])("%s for a session that is over is an error of its own and tells nobody", async (_, answer) => {
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    let open: () => void = () => undefined;
    const opened = new Promise<void>((resolve) => {
      open = resolve;
    });
    override("post", "/api/teams", async () => {
      await opened;
      return answer();
    });
    setCsrfToken(fixtures.csrfToken);
    const call = failure(api.post("/api/teams", { body: { name: "x" } }));
    setCsrfToken(null);
    setCsrfToken("the-token-of-the-next-session");
    open();
    const error = await call;
    expect(error).toBeInstanceOf(SessionOverError);
    expect(handler).not.toHaveBeenCalled();
    unsubscribe();
  });

  test("the token of `me` does not make what was asked beside it an old answer", async () => {
    const teams = api.get("/api/teams");
    setCsrfToken(fixtures.csrfToken);
    // The same token again, as `me` gives it every time it is asked.
    setCsrfToken(fixtures.csrfToken);
    await expect(teams).resolves.toEqual({ teams: fixtures.teamList });
  });

  test("a 401 that nobody hears is not counted", async () => {
    override("get", "/api/teams", unauthenticated);
    await failure(api.get("/api/teams"));
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    await failure(api.get("/api/teams"));
    expect(handler).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  test("a handler that throws stops neither the others nor the ApiError", async () => {
    const after = vi.fn();
    const stops = [
      onUnauthenticated(() => {
        throw new Error("a handler failed");
      }),
      onUnauthenticated(after),
    ];
    override("get", "/api/teams", unauthenticated);
    const error = await apiFailure(api.get("/api/teams"));
    expect(error.status).toBe(401);
    expect(error.code).toBe("unauthenticated");
    expect(after).toHaveBeenCalledTimes(1);
    for (const stop of stops) stop();
  });

  const exceptions: [string, () => Promise<unknown>][] = [
    [
      "POST /api/auth/login",
      () => {
        override("post", "/api/auth/login", unauthenticated);
        return api.post("/api/auth/login", { body: { email: "a@example.test", password: "x" } });
      },
    ],
    [
      "POST /api/auth/accept-invite",
      () => {
        override("post", "/api/auth/accept-invite", unauthenticated);
        return api.post("/api/auth/accept-invite", { body: { token: "t", password: "x" } });
      },
    ],
    [
      "POST /api/auth/password",
      () => {
        override("post", "/api/auth/password", unauthenticated);
        return api.post("/api/auth/password", {
          body: { current_password: "x", new_password: "y" },
        });
      },
    ],
    [
      "GET /api/auth/me",
      () => {
        override("get", "/api/auth/me", unauthenticated);
        return api.get("/api/auth/me");
      },
    ],
  ];

  test.each(exceptions)("401 on %s does not sign out", async (_, call) => {
    setCsrfToken(fixtures.csrfToken);
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    const error = await apiFailure(call());
    expect(error.status).toBe(401);
    expect(handler).not.toHaveBeenCalled();
    unsubscribe();
  });

  const others: [string, () => Promise<unknown>][] = [
    [
      "GET /api/teams",
      () => {
        override("get", "/api/teams", unauthenticated);
        return api.get("/api/teams");
      },
    ],
    [
      "GET /api/users/{id}",
      () => {
        override("get", "/api/users/{id}", unauthenticated);
        return api.get("/api/users/{id}", { params: { id: 1 } });
      },
    ],
    [
      "POST /api/keys",
      () => {
        override("post", "/api/keys", unauthenticated);
        return api.post("/api/keys", { body: { name: "k" } });
      },
    ],
    [
      "PATCH /api/providers/{id}",
      () => {
        override("patch", "/api/providers/{id}", unauthenticated);
        return api.patch("/api/providers/{id}", { params: { id: 1 }, body: {} });
      },
    ],
    [
      "PUT /api/teams/{id}/members/{user_id}",
      () => {
        override("put", "/api/teams/{id}/members/{user_id}", unauthenticated);
        return api.put("/api/teams/{id}/members/{user_id}", {
          params: { id: 1, user_id: 2 },
          body: { role: "member" },
        });
      },
    ],
    [
      "DELETE /api/tokens/{id}",
      () => {
        override("delete", "/api/tokens/{id}", unauthenticated);
        return api.delete("/api/tokens/{id}", { params: { id: 1 } });
      },
    ],
    [
      "GET /api/audit",
      () => {
        override("get", "/api/audit", unauthenticated);
        return api.get("/api/audit");
      },
    ],
    [
      "POST /api/auth/logout",
      () => {
        override("post", "/api/auth/logout", unauthenticated);
        return api.post("/api/auth/logout");
      },
    ],
    [
      "POST /api/users/{id}/invite",
      () => {
        override("post", "/api/users/{id}/invite", unauthenticated);
        return api.post("/api/users/{id}/invite", { params: { id: 6 } });
      },
    ],
  ];

  test.each(others)("401 on %s signs out", async (_, call) => {
    setCsrfToken(fixtures.csrfToken);
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    const error = await apiFailure(call());
    expect(error.status).toBe(401);
    expect(handler).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  test("other errors do not sign out", async () => {
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    override("get", "/api/teams", () => refuse(errors.forbidden));
    await failure(api.get("/api/teams"));
    override("get", "/api/teams", networkFailure);
    await failure(api.get("/api/teams"));
    expect(handler).not.toHaveBeenCalled();
    unsubscribe();
  });
});

describe("the playground call", () => {
  const body = {
    model: "openai/gpt-4o-mini",
    stream: true,
    messages: [{ role: "user", content: "hi" }],
  };
  const openAiError = (status: number, type: string, message: string, headers = {}) =>
    new Response(JSON.stringify({ error: { message, type, param: null, code: null } }), {
      status,
      headers: { "content-type": "application/json", ...headers },
    });

  test("it is sent as a write: JSON, the session cookie and the CSRF token", async () => {
    setCsrfToken(fixtures.csrfToken);
    let seen: Request | undefined;
    override("post", "/api/playground/chat", ({ request }) => {
      seen = request.clone();
      return new Response("data: [DONE]\n\n", { headers: { "content-type": "text/event-stream" } });
    });
    const controller = new AbortController();
    const response = await playgroundChat(body, controller.signal);
    expect(await response.text()).toBe("data: [DONE]\n\n");
    expect(seen?.method).toBe("POST");
    expect(seen?.headers.get("x-csrf-token")).toBe(fixtures.csrfToken);
    expect(seen?.headers.get("content-type")).toBe("application/json");
    expect(await seen?.json()).toEqual(body);
    setCsrfToken(null);
  });

  test("a refusal of the pipeline keeps its type, its message and the wait", async () => {
    override("post", "/api/playground/chat", () =>
      openAiError(429, "rate_limit_error", "rate limit 'rpm' of user reached", { "retry-after": "30" }),
    );
    const error = await apiFailure(playgroundChat(body));
    expect([error.status, error.code, error.message, error.retryAfter]).toEqual([
      429,
      "rate_limit_error",
      "rate limit 'rpm' of user reached",
      30,
    ]);
  });

  test("the code of the body wins over its type", async () => {
    override("post", "/api/playground/chat", () => {
      const refused = new Response(
        JSON.stringify({ error: { message: "budget spent", type: "rate_limit_error", code: "budget_exceeded" } }),
        { status: 429, headers: { "retry-after": "oops" } },
      );
      return refused;
    });
    const error = await apiFailure(playgroundChat(body));
    expect([error.code, error.retryAfter]).toEqual(["budget_exceeded", null]);
  });

  test("an answer that is not an error body is an unexpected response", async () => {
    override("post", "/api/playground/chat", () => new Response("<html>", { status: 502 }));
    const error = await apiFailure(playgroundChat(body));
    expect([error.status, error.code]).toEqual([502, "unexpected_response"]);
    expect(error.message).not.toContain("<");
  });

  test("a 401 ends the session, as for every call", async () => {
    const heard = vi.fn();
    const stop = onUnauthenticated(heard);
    override("post", "/api/playground/chat", unauthenticated);
    const error = await apiFailure(playgroundChat(body));
    expect(error.status).toBe(401);
    expect(heard).toHaveBeenCalledTimes(1);
    stop();
  });

  test("a network failure is a NetworkError, an abort is the abort", async () => {
    override("post", "/api/playground/chat", networkFailure);
    expect(await failure(playgroundChat(body))).toBeInstanceOf(NetworkError);
    const controller = new AbortController();
    controller.abort();
    const error = await failure(playgroundChat(body, controller.signal));
    expect(error).not.toBeInstanceOf(NetworkError);
    expect(error).toBeInstanceOf(Error);
  });
});
