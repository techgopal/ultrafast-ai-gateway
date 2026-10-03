// The real global fetch against a real local HTTP server (Node only here; src/ is not Node-specific).
import http from "node:http";
import type { AddressInfo } from "node:net";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Client, gateway, type StreamEvent } from "../src/index.js";
import { KEY, MSGS, OPENAI_CHAT, OPENAI_STREAM } from "./helpers.js";

let server: http.Server;
let base: string;
const seen: Array<{ url: string; headers: http.IncomingHttpHeaders; body: { model: string; stream?: boolean } }> = [];

beforeAll(async () => {
  server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      const b = JSON.parse(body) as { model: string; stream?: boolean };
      seen.push({ url: req.url ?? "", headers: req.headers, body: b });
      if (b.model === "limited") {
        res.writeHead(429, { "retry-after": "7" }).end(JSON.stringify({ error: { message: "slow down" } }));
      } else if (b.model === "moved") {
        res.writeHead(302, { location: "http://127.0.0.1:1/" }).end();
      } else if (b.stream) {
        res.writeHead(200, { "content-type": "text/event-stream" });
        res.write(OPENAI_STREAM.slice(0, 40));
        setTimeout(() => res.end(OPENAI_STREAM.slice(40)), 5);
      } else {
        res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(OPENAI_CHAT));
      }
    });
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});
afterAll(() => new Promise<void>((r) => server.close(() => r())));

describe("node smoke", () => {
  const client = () => new Client(gateway({ baseUrl: base, key: KEY }), { timeoutMs: 5000 });
  it("chat, stream, refusal, redirect, connection refused", async () => {
    const c = client();
    const r = await c.chat({ model: "gpt-4o", messages: MSGS, tags: { team: "x" } });
    expect(r.content).toBe("hello");
    expect(seen[0]!.url).toBe("/v1/chat/completions");
    expect(seen[0]!.headers.authorization).toBe(`Bearer ${KEY}`);
    expect(seen[0]!.headers["x-uf-tags"]).toBe('{"team":"x"}');

    const events: StreamEvent[] = [];
    for await (const e of c.chatStream({ model: "gpt-4o", messages: MSGS })) events.push(e);
    expect(events.filter((e) => e.type === "delta").map((e) => (e as { text: string }).text).join("")).toBe("hello");
    expect(events.at(-1)?.type).toBe("done");

    await expect(c.chat({ model: "limited", messages: MSGS })).rejects.toMatchObject({ kind: "rate_limited", retryable: true, retryAfter: 7, status: 429 });
    await expect(c.chat({ model: "moved", messages: MSGS })).rejects.toMatchObject({ kind: "invalid_request", message: "the server answered with a redirect" });

    const dead = new Client(gateway({ baseUrl: "http://127.0.0.1:1", key: KEY }), { timeoutMs: 2000 });
    const e = await dead.chat({ model: "m", messages: MSGS }).catch((x) => x);
    expect(e).toMatchObject({ kind: "network", retryable: true });
    expect(JSON.stringify(e) + e.message).not.toContain(KEY);
  });
});
