import { describe, expect, expectTypeOf, test, vi } from "vitest";
import * as fixtures from "@/test/fixtures";
import { apiError, networkFailure, noContent, override } from "@/test/handlers";
import { api, onUnauthenticated, setCsrfToken, type ResponseOf } from "./client";
import { ApiError, NetworkError } from "./errors";
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

const unauthorized = () => apiError(401, "unauthorized", "Sign in to continue.");

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
    override("post", "/api/teams", () =>
      apiError(409, "team_exists", "A team with this name exists."),
    );
    const error = await apiFailure(api.post("/api/teams", { body: { name: "Platform" } }));
    expect(error.status).toBe(409);
    expect(error.code).toBe("team_exists");
    expect(error.message).toBe("A team with this name exists.");
    expect(error.fields).toEqual({});
  });

  test("field errors are kept", async () => {
    override("post", "/api/users", () =>
      apiError(422, "validation_failed", "Some fields are not valid.", {
        email: "This is not an email address.",
      }),
    );
    const error = await apiFailure(
      api.post("/api/users", { body: { email: "x", name: "X", role: "member" } }),
    );
    expect(error.status).toBe(422);
    expect(error.code).toBe("validation_failed");
    expect(error.fields.email).toBe("This is not an email address.");
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
      HttpResponseWithHeader(401, "invalid_credentials", "The email or the password is wrong."),
    );
    const refused = await apiFailure(
      api.post("/api/auth/login", { body: { email: "maya@example.test", password } }),
    );
    expect(Object.getOwnPropertyNames(refused).sort()).toEqual(
      ["code", "fields", "message", "name", "stack", "status"].sort(),
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
    override("post", "/api/teams", () => apiError(409, "team_exists", "It exists."));
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

function HttpResponseWithHeader(status: number, code: string, message: string): Response {
  return new Response(JSON.stringify({ error: { code, message } }), {
    status,
    headers: { "Content-Type": "application/json", "x-test": "header-value-of-the-response" },
  });
}

describe("the end of the session", () => {
  test("401 signs out once", async () => {
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    override("get", "/api/teams", unauthorized);
    override("get", "/api/users", unauthorized);

    const errors = await Promise.all([
      apiFailure(api.get("/api/teams")),
      apiFailure(api.get("/api/users")),
    ]);

    expect(handler).toHaveBeenCalledTimes(1);
    expect(errors.map((e) => e.status)).toEqual([401, 401]);
    expect(errors.map((e) => e.code)).toEqual(["unauthorized", "unauthorized"]);
    unsubscribe();
  });

  test("every handler is called, and none after it unsubscribed", async () => {
    const first = vi.fn();
    const second = vi.fn();
    const gone = vi.fn();
    const stops = [onUnauthenticated(first), onUnauthenticated(second)];
    onUnauthenticated(gone)();
    override("get", "/api/teams", unauthorized);
    await failure(api.get("/api/teams"));
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
    expect(gone).not.toHaveBeenCalled();
    for (const stop of stops) stop();
  });

  test("a new session is told of its end again", async () => {
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    override("get", "/api/teams", unauthorized);
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
    override("get", "/api/teams", unauthorized);
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

  test("a 401 that nobody hears is not counted", async () => {
    override("get", "/api/teams", unauthorized);
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
    override("get", "/api/teams", unauthorized);
    const error = await apiFailure(api.get("/api/teams"));
    expect(error.status).toBe(401);
    expect(error.code).toBe("unauthorized");
    expect(after).toHaveBeenCalledTimes(1);
    for (const stop of stops) stop();
  });

  const exceptions: [string, () => Promise<unknown>][] = [
    [
      "POST /api/auth/login",
      () => {
        override("post", "/api/auth/login", unauthorized);
        return api.post("/api/auth/login", { body: { email: "a@example.test", password: "x" } });
      },
    ],
    [
      "POST /api/auth/accept-invite",
      () => {
        override("post", "/api/auth/accept-invite", unauthorized);
        return api.post("/api/auth/accept-invite", { body: { token: "t", password: "x" } });
      },
    ],
    [
      "POST /api/auth/password",
      () => {
        override("post", "/api/auth/password", unauthorized);
        return api.post("/api/auth/password", {
          body: { current_password: "x", new_password: "y" },
        });
      },
    ],
    [
      "GET /api/auth/me",
      () => {
        override("get", "/api/auth/me", unauthorized);
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
        override("get", "/api/teams", unauthorized);
        return api.get("/api/teams");
      },
    ],
    [
      "GET /api/users/{id}",
      () => {
        override("get", "/api/users/{id}", unauthorized);
        return api.get("/api/users/{id}", { params: { id: 1 } });
      },
    ],
    [
      "POST /api/keys",
      () => {
        override("post", "/api/keys", unauthorized);
        return api.post("/api/keys", { body: { name: "k" } });
      },
    ],
    [
      "PATCH /api/providers/{id}",
      () => {
        override("patch", "/api/providers/{id}", unauthorized);
        return api.patch("/api/providers/{id}", { params: { id: 1 }, body: {} });
      },
    ],
    [
      "PUT /api/teams/{id}/members/{user_id}",
      () => {
        override("put", "/api/teams/{id}/members/{user_id}", unauthorized);
        return api.put("/api/teams/{id}/members/{user_id}", {
          params: { id: 1, user_id: 2 },
          body: { role: "member" },
        });
      },
    ],
    [
      "DELETE /api/tokens/{id}",
      () => {
        override("delete", "/api/tokens/{id}", unauthorized);
        return api.delete("/api/tokens/{id}", { params: { id: 1 } });
      },
    ],
    [
      "GET /api/audit",
      () => {
        override("get", "/api/audit", unauthorized);
        return api.get("/api/audit");
      },
    ],
    [
      "POST /api/auth/logout",
      () => {
        override("post", "/api/auth/logout", unauthorized);
        return api.post("/api/auth/logout");
      },
    ],
    [
      "POST /api/users/{id}/invite",
      () => {
        override("post", "/api/users/{id}/invite", unauthorized);
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
    override("get", "/api/teams", () => apiError(403, "forbidden", "Not allowed."));
    await failure(api.get("/api/teams"));
    override("get", "/api/teams", networkFailure);
    await failure(api.get("/api/teams"));
    expect(handler).not.toHaveBeenCalled();
    unsubscribe();
  });
});
