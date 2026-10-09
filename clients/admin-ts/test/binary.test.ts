import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createAdminClient, type AdminClient } from "../src/index.js";
import { startGateway, type Gateway } from "./gateway.js";
import { mintToken } from "./session.js";

let gateway: Gateway;
let api: AdminClient;

beforeAll(async () => {
  gateway = await startGateway();
  api = createAdminClient({ baseUrl: gateway.origin, token: await mintToken(gateway, gateway.admin) });
  await api.call(
    api.raw.POST("/api/providers", {
      body: { name: "exported", kind: "openai", base_url: "http://127.0.0.1:1" },
    }),
  );
});
afterAll(async () => {
  await gateway.stop();
});

describe("downloads are not parsed as JSON", () => {
  it("exportConfig returns the parsed configuration file", async () => {
    const file = await api.exportConfig();
    expect(file.format).toBe("ultrafast-config");
    expect(file.version).toBe(1);
    expect(file.providers?.map((p) => p.name)).toContain("exported");
  });

  it("downloadBackup returns the bytes of a SQLite file", async () => {
    const bytes = await api.downloadBackup();
    expect(bytes).toBeInstanceOf(Uint8Array);
    expect(bytes.length).toBeGreaterThan(4096);
    expect(new TextDecoder().decode(bytes.subarray(0, 15))).toBe("SQLite format 3");
  });

  it("raw exposes the same download with parseAs", async () => {
    const { data, response } = await api.raw.GET("/api/backup", { parseAs: "arrayBuffer" });
    expect(response.headers.get("content-type")).toContain("sqlite");
    expect(data).toBeInstanceOf(ArrayBuffer);
  });
});
