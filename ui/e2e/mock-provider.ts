// A model API in the test process, for the gateway to call in place of a
// real provider. It speaks the OpenAI chat completions form, asks for its own
// API key, and remembers what it was sent. Only the gateway calls it: the
// browser tests fail on any request of the browser to another origin.
import { randomBytes } from "node:crypto";
import { createServer, type IncomingMessage } from "node:http";

export interface MockCall {
  /** Whether the call carried the provider's API key. */
  authorized: boolean;
  model: unknown;
}

export interface MockProvider {
  /** The base URL to give the gateway, ending in `/v1`. */
  baseUrl: string;
  /** The API key the provider asks for. */
  apiKey: string;
  /** What every completion answers. */
  answer: string;
  calls: MockCall[];
  close: () => Promise<void>;
}

async function bodyOf(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  try {
    return JSON.parse(Buffer.concat(chunks).toString("utf8")) as unknown;
  } catch {
    return null;
  }
}

export async function startMockProvider(): Promise<MockProvider> {
  const apiKey = `mock-${randomBytes(16).toString("hex")}`;
  const answer = `Hello from the mock provider ${randomBytes(4).toString("hex")}.`;
  const calls: MockCall[] = [];

  const server = createServer((request, response) => {
    void (async () => {
      const body = await bodyOf(request);
      const send = (status: number, value: unknown) => {
        response.writeHead(status, { "content-type": "application/json" });
        response.end(JSON.stringify(value));
      };
      if (request.method !== "POST" || request.url !== "/v1/chat/completions") {
        send(404, { error: { message: "Unknown path.", type: "invalid_request_error" } });
        return;
      }
      const authorized = request.headers.authorization === `Bearer ${apiKey}`;
      const model = typeof body === "object" && body !== null && "model" in body ? body.model : null;
      calls.push({ authorized, model });
      if (!authorized) {
        send(401, {
          error: { message: "Incorrect API key.", type: "invalid_request_error", code: "invalid_api_key" },
        });
        return;
      }
      send(200, {
        id: "chatcmpl-e2e",
        object: "chat.completion",
        created: Math.floor(Date.now() / 1000),
        model,
        choices: [{ index: 0, message: { role: "assistant", content: answer }, finish_reason: "stop" }],
        usage: { prompt_tokens: 3, completion_tokens: 7, total_tokens: 10 },
      });
    })();
  });

  await new Promise<void>((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => {
      done();
    });
  });
  const address = server.address();
  if (typeof address !== "object" || address === null) throw new Error("The mock provider has no port.");

  return {
    baseUrl: `http://127.0.0.1:${address.port}/v1`,
    apiKey,
    answer,
    calls,
    close: () =>
      new Promise<void>((done) => {
        server.closeAllConnections();
        server.close(() => {
          done();
        });
      }),
  };
}
