import { describe, expect, it } from "vitest";
import { inspect } from "node:util";
import { AdminApiError, createAdminClient } from "../src/index.js";

const TOKEN = "uf-at-secret-value-1234567890";

function answering(response: () => Response | Promise<Response>, seen: Request[] = []) {
  return (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    seen.push(new Request(input, init));
    return Promise.resolve(response());
  };
}

describe("createAdminClient (no gateway)", () => {
  it("sends the token as a bearer and strips a trailing slash from the base URL", async () => {
    const seen: Request[] = [];
    const api = createAdminClient({
      baseUrl: "http://gw.test/",
      token: TOKEN,
      fetch: answering(
        () => new Response(JSON.stringify({ providers: [] }), { headers: { "content-type": "application/json" } }),
        seen,
      ),
    });
    await api.raw.GET("/api/providers");
    expect(seen[0]?.url).toBe("http://gw.test/api/providers");
    expect(seen[0]?.headers.get("authorization")).toBe(`Bearer ${TOKEN}`);
  });

  it("maps a body that is not the gateway's error shape", async () => {
    const api = createAdminClient({
      baseUrl: "http://gw.test",
      token: TOKEN,
      fetch: answering(() => new Response("upstream down", { status: 502 })),
    });
    const error = await api.call(api.raw.GET("/api/providers")).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(AdminApiError);
    expect((error as AdminApiError).status).toBe(502);
    expect((error as AdminApiError).code).toBe("http_502");
    expect((error as AdminApiError).message).toBe("upstream down");
  });

  it("times out with a typed error", async () => {
    const hanging = (_input: RequestInfo | URL, init?: RequestInit): Promise<Response> =>
      new Promise((_resolve, reject) => {
        init?.signal?.addEventListener("abort", () => {
          reject(init.signal?.reason);
        });
      });
    const api = createAdminClient({ baseUrl: "http://gw.test", token: TOKEN, timeoutMs: 50, fetch: hanging });
    const error = await api.call(api.raw.GET("/api/providers")).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(AdminApiError);
    expect((error as AdminApiError).status).toBe(0);
    expect((error as AdminApiError).code).toBe("timeout");
  });

  it("never shows the token: not in the client, an error, or its inspection", async () => {
    const api = createAdminClient({
      baseUrl: "http://gw.test",
      token: TOKEN,
      fetch: answering(
        () =>
          new Response(JSON.stringify({ error: { code: "forbidden", message: `bad ${TOKEN}` } }), {
            status: 403,
            headers: { "content-type": "application/json" },
          }),
      ),
    });
    const error = (await api.call(api.raw.GET("/api/providers")).catch((e: unknown) => e)) as AdminApiError;
    for (const text of [String(error), error.message, JSON.stringify(error), inspect(error), inspect(api, { depth: 6 }), JSON.stringify(api)]) {
      expect(text).not.toContain(TOKEN);
    }
  });
});
