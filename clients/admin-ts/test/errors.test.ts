import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { AdminApiError, createAdminClient } from "../src/index.js";
import { startGateway, type Gateway } from "./gateway.js";
import { memberToken, mintToken } from "./session.js";

let gateway: Gateway;
let adminToken: string;
let member: string;

beforeAll(async () => {
  gateway = await startGateway();
  adminToken = await mintToken(gateway, gateway.admin);
  member = await memberToken(gateway, adminToken, "member@example.com");
});
afterAll(async () => {
  await gateway.stop();
});

async function failure(promise: Promise<unknown>): Promise<AdminApiError> {
  try {
    await promise;
  } catch (error) {
    expect(error).toBeInstanceOf(AdminApiError);
    return error as AdminApiError;
  }
  throw new Error("the call did not throw");
}

describe("error mapping", () => {
  it("401: a token the gateway does not know", async () => {
    const api = createAdminClient({ baseUrl: gateway.origin, token: "uf-at-not-a-real-token" });
    const error = await failure(api.call(api.raw.GET("/api/providers")));
    expect(error.status).toBe(401);
    expect(error.code).toBe("unauthenticated");
    expect(error.fields).toEqual({});
    expect(error.message).not.toContain("uf-at-not-a-real-token");
    expect(String(error)).not.toContain("uf-at-not-a-real-token");
  });

  it("403: a member may not add a provider", async () => {
    const api = createAdminClient({ baseUrl: gateway.origin, token: member });
    const error = await failure(
      api.call(
        api.raw.POST("/api/providers", {
          body: { name: "p", kind: "openai", base_url: "http://127.0.0.1:1" },
        }),
      ),
    );
    expect(error.status).toBe(403);
    expect(error.code).toBe("forbidden");
  });

  it("404: a key that does not exist", async () => {
    const api = createAdminClient({ baseUrl: gateway.origin, token: adminToken });
    const error = await failure(
      api.call(api.raw.GET("/api/keys/{id}", { params: { path: { id: 999999 } } })),
    );
    expect(error.status).toBe(404);
    expect(error.code).toBe("not_found");
  });

  it("409: a provider name taken", async () => {
    const api = createAdminClient({ baseUrl: gateway.origin, token: adminToken });
    const body = { name: "dup", kind: "openai", base_url: "http://127.0.0.1:1" };
    await api.call(api.raw.POST("/api/providers", { body }));
    const error = await failure(api.call(api.raw.POST("/api/providers", { body })));
    expect(error.status).toBe(409);
    expect(error.code).toBe("provider_exists");
  });

  it("422: validation fields", async () => {
    const api = createAdminClient({ baseUrl: gateway.origin, token: adminToken });
    const error = await failure(
      api.call(
        api.raw.POST("/api/providers", {
          body: { name: "ok", kind: "nonsense", base_url: "http://127.0.0.1:1" },
        }),
      ),
    );
    expect(error.status).toBe(422);
    expect(error.code).toBe("validation_failed");
    expect(Object.keys(error.fields)).toContain("kind");
    expect(error.fields.kind).toMatch(/kind must be/);
  });

  it("a gateway that is not there is a typed error with status 0", async () => {
    const api = createAdminClient({ baseUrl: "http://127.0.0.1:1", token: adminToken });
    const error = await failure(api.call(api.raw.GET("/api/providers")));
    expect(error.status).toBe(0);
    expect(error.code).toBe("network_error");
    expect(error.message).not.toContain(adminToken);
  });
});
