import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createAdminClient, type AdminClient } from "../src/index.js";
import { startGateway, type Gateway } from "./gateway.js";
import { mintToken } from "./session.js";

let gateway: Gateway;
let api: AdminClient;

beforeAll(async () => {
  gateway = await startGateway();
  api = createAdminClient({ baseUrl: gateway.origin, token: await mintToken(gateway, gateway.admin) });
});
afterAll(async () => {
  await gateway.stop();
});

describe("round trips through the token, one per tag", () => {
  let providerId = 0;
  let modelId = 0;
  let keyId = 0;
  let teamId = 0;

  it("providers: create, list, update, view in the list", async () => {
    const created = await api.call(
      api.raw.POST("/api/providers", {
        body: { name: "local", kind: "openai", base_url: "http://127.0.0.1:1", api_key: "sk-test-not-real" },
      }),
    );
    providerId = created.id;
    expect(created.has_credential).toBe(true);
    expect(JSON.stringify(created)).not.toContain("sk-test-not-real");
    const updated = await api.call(
      api.raw.PATCH("/api/providers/{id}", {
        params: { path: { id: providerId } },
        body: { base_url: "http://127.0.0.1:1/v1" },
      }),
    );
    expect(updated.base_url).toBe("http://127.0.0.1:1/v1");
    const list = await api.call(api.raw.GET("/api/providers"));
    expect(list.providers.map((p) => p.name)).toEqual(["local"]);
  });

  it("models: create, enable, list", async () => {
    const created = await api.call(
      api.raw.POST("/api/models", { body: { provider_id: providerId, name: "gpt-test" } }),
    );
    modelId = created.id;
    expect(created.enabled).toBe(false);
    const updated = await api.call(
      api.raw.PATCH("/api/models/{id}", {
        params: { path: { id: modelId } },
        body: { enabled: true, input_price_micros: 1000 },
      }),
    );
    expect(updated.enabled).toBe(true);
    const list = await api.call(api.raw.GET("/api/models"));
    expect(list.models.map((m) => m.name)).toContain("gpt-test");
  });

  it("keys: create shows the secret once, list never does, revoke", async () => {
    const created = await api.call(api.raw.POST("/api/keys", { body: { name: "ci" } }));
    keyId = created.key.id;
    expect(created.secret).toMatch(/^uf-/);
    const list = await api.call(api.raw.GET("/api/keys"));
    expect(JSON.stringify(list)).not.toContain(created.secret);
    expect(list.keys.map((k) => k.name)).toContain("ci");
    await api.call(api.raw.DELETE("/api/keys/{id}", { params: { path: { id: keyId } } }));
    const viewed = await api.call(api.raw.GET("/api/keys/{id}", { params: { path: { id: keyId } } }));
    expect(viewed.revoked_at).not.toBeNull();
  });

  it("budgets: set, list, delete", async () => {
    const set = await api.call(
      api.raw.PUT("/api/budgets", {
        body: { scope: "gateway", amount_micros: 5_000_000, period: "monthly", action: "alert" },
      }),
    );
    expect(set.amount_micros).toBe(5_000_000);
    const list = await api.call(api.raw.GET("/api/budgets"));
    expect(list.budgets.map((b) => b.id)).toContain(set.id);
    const gone = await api.call(api.raw.DELETE("/api/budgets/{id}", { params: { path: { id: set.id } } }));
    expect(gone).toBeUndefined();
  });

  it("limits: set and delete", async () => {
    const set = await api.call(
      api.raw.PUT("/api/limits", { body: { scope: "gateway", requests_per_minute: 600 } }),
    );
    expect(set.requests_per_minute).toBe(600);
    await api.call(api.raw.DELETE("/api/limits/{id}", { params: { path: { id: set.id } } }));
    const list = await api.call(api.raw.GET("/api/limits"));
    expect(list.limits).toEqual([]);
  });

  it("teams: create, rename, view, delete", async () => {
    const created = await api.call(api.raw.POST("/api/teams", { body: { name: "Platform" } }));
    teamId = created.id;
    await api.call(
      api.raw.PATCH("/api/teams/{id}", { params: { path: { id: teamId } }, body: { name: "Infra" } }),
    );
    const viewed = await api.call(api.raw.GET("/api/teams/{id}", { params: { path: { id: teamId } } }));
    expect(viewed.team.name).toBe("Infra");
    await api.call(api.raw.DELETE("/api/teams/{id}", { params: { path: { id: teamId } } }));
  });

  it("alerts: channel, then a rule that names it, then both removed", async () => {
    const channel = await api.call(
      api.raw.POST("/api/alerts/channels", {
        body: { name: "ops", kind: "webhook", url: "http://127.0.0.1:9/hook" },
      }),
    );
    const rule = await api.call(
      api.raw.POST("/api/alerts/rules", {
        body: {
          name: "errors",
          kind: "error_rate",
          // The document types `params` as an object with no properties, which the
          // generator reads as Record<string, never>; the gateway reads it by `kind`.
          params: { scope: "gateway", percent: 50 } as unknown as Record<string, never>,
          channel_ids: [channel.channel.id],
        },
      }),
    );
    expect(rule.kind).toBe("error_rate");
    const rules = await api.call(api.raw.GET("/api/alerts/rules"));
    expect(rules.rules.map((r) => r.name)).toContain("errors");
    await api.call(api.raw.DELETE("/api/alerts/rules/{id}", { params: { path: { id: rule.id } } }));
    await api.call(
      api.raw.DELETE("/api/alerts/channels/{id}", { params: { path: { id: channel.channel.id } } }),
    );
  });

  it("tokens: list, revoke, and the revoked token is refused", async () => {
    const second = await mintToken(gateway, gateway.admin, "second");
    const list = await api.call(api.raw.GET("/api/tokens"));
    const names = list.tokens.map((t) => t.name);
    expect(names).toContain("second");
    expect(JSON.stringify(list)).not.toContain(second);
    const id = list.tokens.find((t) => t.name === "second")?.id ?? 0;
    await api.call(api.raw.DELETE("/api/tokens/{id}", { params: { path: { id } } }));
    const other = createAdminClient({ baseUrl: gateway.origin, token: second });
    await expect(other.call(other.raw.GET("/api/tokens"))).rejects.toMatchObject({ status: 401 });
  });

  it("users: invite and list", async () => {
    const invited = await api.call(
      api.raw.POST("/api/users", { body: { email: "new@example.com", name: "New", role: "member" } }),
    );
    expect(invited.invite_link).toBeTruthy();
    const list = await api.call(api.raw.GET("/api/users"));
    expect(list.users.map((u) => u.email)).toContain("new@example.com");
  });

  it("models and providers: delete in order", async () => {
    await api.call(api.raw.DELETE("/api/models/{id}", { params: { path: { id: modelId } } }));
    await api.call(api.raw.DELETE("/api/providers/{id}", { params: { path: { id: providerId } } }));
    expect((await api.call(api.raw.GET("/api/providers"))).providers).toEqual([]);
  });
});
