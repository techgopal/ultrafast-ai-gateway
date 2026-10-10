import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
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

describe("timeouts cover the body, aborts stay the caller's", () => {
  let server: Server;
  let origin = "";

  beforeAll(async () => {
    server = createServer((request, response) => {
      if (request.url === "/api/providers") {
        // Headers and the start of a body, then nothing.
        response.writeHead(200, { "content-type": "application/json" });
        response.write('{"providers":[');
      } else if (request.url === "/api/backup") {
        response.writeHead(200, { "content-type": "application/vnd.sqlite3" });
        response.write("SQLite format 3\0");
        if (request.headers["x-slow"] === undefined) {
          setTimeout(() => response.end("rest"), 600);
        }
      } else {
        response.writeHead(200, { "content-type": "application/json" });
        response.write('{"format":');
      }
    });
    await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
    origin = `http://127.0.0.1:${String((server.address() as AddressInfo).port)}`;
  });
  afterAll(async () => {
    server.closeAllConnections();
    await new Promise((done) => server.close(done));
  });

  it("call: a body that stalls after the headers is an AdminApiError timeout", async () => {
    const api = createAdminClient({ baseUrl: origin, token: TOKEN, timeoutMs: 300 });
    const error = await api.call(api.raw.GET("/api/providers")).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(AdminApiError);
    expect((error as AdminApiError).code).toBe("timeout");
    expect((error as AdminApiError).status).toBe(0);
  });

  it("exportConfig: a stalled body is a timeout, and its own timeoutMs applies", async () => {
    const api = createAdminClient({ baseUrl: origin, token: TOKEN });
    const started = Date.now();
    const error = await api.exportConfig({ timeoutMs: 300 }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(AdminApiError);
    expect((error as AdminApiError).code).toBe("timeout");
    expect(Date.now() - started).toBeLessThan(5_000);
  });

  it("downloadBackup: a longer timeoutMs than the client's lets a slow body finish", async () => {
    const api = createAdminClient({ baseUrl: origin, token: TOKEN, timeoutMs: 200 });
    const early = await api.downloadBackup().catch((e: unknown) => e);
    expect(early).toBeInstanceOf(AdminApiError);
    const bytes = await api.downloadBackup({ timeoutMs: 5_000 });
    expect(new TextDecoder().decode(bytes)).toContain("SQLite format 3");
  });

  it("a caller's abort is rethrown as it was given", async () => {
    const api = createAdminClient({ baseUrl: origin, token: TOKEN });
    const reason = new Error("mine");
    const controller = new AbortController();
    const pending = api.call(api.raw.GET("/api/providers", { signal: controller.signal }));
    setTimeout(() => {
      controller.abort(reason);
    }, 100);
    await expect(pending).rejects.toBe(reason);
    const early = new AbortController();
    early.abort(reason);
    await expect(api.call(api.raw.GET("/api/providers", { signal: early.signal }))).rejects.toBe(reason);
  });

  it("a caller's abort with a primitive reason is rethrown as given", async () => {
    const api = createAdminClient({ baseUrl: origin, token: TOKEN });
    const controller = new AbortController();
    const pending = api.call(api.raw.GET("/api/providers", { signal: controller.signal }));
    setTimeout(() => {
      controller.abort("stop");
    }, 100);
    await expect(pending).rejects.toBe("stop");
    const early = new AbortController();
    early.abort(42);
    await expect(api.call(api.raw.GET("/api/providers", { signal: early.signal }))).rejects.toBe(42);
  });
});

describe("an answer that is not what was asked for", () => {
  it("a 200 export that is not JSON is an AdminApiError that does not echo the body", async () => {
    const api = createAdminClient({
      baseUrl: "http://gateway.test",
      token: TOKEN,
      fetch: answering(() => new Response("<html>login uf-at-secret-value-1234567890</html>", { status: 200 })),
    });
    const error = await api.exportConfig().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(AdminApiError);
    expect((error as AdminApiError).status).toBe(200);
    expect((error as AdminApiError).code).toBe("invalid_response");
    expect((error as AdminApiError).message).not.toContain("html");
  });
});
