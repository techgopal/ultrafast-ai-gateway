// Node smoke test of the built module against a local fake OpenAI server:
// one chat, one stream, one refusal. Usage: node smoke.mjs <pkg-dir (nodejs target)>
import assert from "node:assert/strict";
import http from "node:http";
import { createRequire } from "node:module";
import path from "node:path";

const pkg = path.resolve(process.argv[2] ?? new URL("../pkg", import.meta.url).pathname);
const w = createRequire(import.meta.url)(path.join(pkg, "ultrafast_client_wasm.js"));

const CHAT = {
  id: "c1", model: "gpt-4o",
  choices: [{ message: { role: "assistant", content: "hello" }, finish_reason: "stop" }],
  usage: { prompt_tokens: 3, completion_tokens: 2 },
};
const sse = (o) => `data: ${JSON.stringify(o)}\n\n`;
const STREAM = [
  sse({ choices: [{ delta: { content: "he" } }] }),
  sse({ choices: [{ delta: { content: "llo" } }] }),
  sse({ choices: [{ delta: {}, finish_reason: "stop" }], usage: { prompt_tokens: 3, completion_tokens: 2 } }),
  "data: [DONE]\n\n",
].join("");

const seen = [];
const server = http.createServer((req, res) => {
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => {
    seen.push({ url: req.url, headers: req.headers, body: JSON.parse(body) });
    const b = JSON.parse(body);
    if (b.model === "limited") {
      res.writeHead(429, { "retry-after": "7" }).end(JSON.stringify({ error: { message: "slow down" } }));
    } else if (b.stream) {
      res.writeHead(200, { "content-type": "text/event-stream" });
      // Split mid-event to prove the decoder buffers.
      res.write(STREAM.slice(0, 40));
      setTimeout(() => res.end(STREAM.slice(40)), 5);
    } else {
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(CHAT));
    }
  });
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;
const target = JSON.stringify({ kind: "gateway", base_url: base, api_key: "uf-key-12345" });

async function call(model, stream) {
  const built = JSON.parse(
    w.buildRequest(target, JSON.stringify({
      model, stream, tags: { team: "x" },
      messages: [{ role: "user", content: "hi" }],
    })),
  );
  const resp = await fetch(built.url, { method: built.method, headers: built.headers, body: built.body });
  return resp;
}

try {
  // Chat.
  let resp = await call("gpt-4o", false);
  const out = JSON.parse(w.parseResponse("openai", resp.status, new Uint8Array(await resp.arrayBuffer()), null));
  assert.equal(out.content, "hello");
  assert.equal(out.finish_reason, "stop");
  assert.deepEqual(out.usage, { input_tokens: 3, output_tokens: 2 });
  assert.equal(seen[0].url, "/v1/chat/completions");
  assert.equal(seen[0].headers.authorization, "Bearer uf-key-12345");
  assert.equal(seen[0].headers["x-uf-tags"], '{"team":"x"}');

  // Stream.
  resp = await call("gpt-4o", true);
  const dec = new w.StreamDecoder("openai");
  const events = [];
  for await (const chunk of resp.body) events.push(...JSON.parse(dec.feed(chunk)));
  events.push(...JSON.parse(dec.finish()));
  assert.equal(dec.takeError(), undefined);
  assert.equal(events.filter((e) => e.type === "delta").map((e) => e.text).join(""), "hello");
  assert.equal(events.at(-1).type, "done");
  dec.free();

  // Refusal: thrown as error JSON.
  resp = await call("limited", false);
  let err;
  try {
    w.parseResponse("openai", resp.status, new Uint8Array(await resp.arrayBuffer()), resp.headers.get("retry-after"));
  } catch (e) {
    err = JSON.parse(e);
  }
  assert.deepEqual(
    { kind: err.kind, retryable: err.retryable, status: err.status, retry_after_secs: err.retry_after_secs, message: err.message },
    { kind: "rate_limited", retryable: true, status: 429, retry_after_secs: 7, message: "slow down" },
  );
  console.log("smoke ok: chat, stream, refusal");
} finally {
  server.close();
}
