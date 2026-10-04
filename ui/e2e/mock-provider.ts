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

/** How the provider misbehaves, set by a test at any time. */
export interface MockMode {
  /** Answers every call with this status and an error body. */
  status?: number;
  /** Waits this long before the first byte of the answer. */
  delayMs?: number;
  /** For a streaming call: sends one chunk, then drops the connection. */
  breakAfterFirstChunk?: boolean;
}

export interface MockProvider {
  /** The base URL to give the gateway, ending in `/v1`. */
  baseUrl: string;
  /** The API key the provider asks for. */
  apiKey: string;
  /** What every completion answers. */
  answer: string;
  /** The models `GET /v1/models` lists. */
  models: string[];
  /** Completions asked for, in order. */
  calls: MockCall[];
  /** How many times the model list was asked for. */
  listCalls: number;
  /** The token usage every completion reports; a test may change it at any time. */
  usage: { prompt: number; completion: number };
  mode: MockMode;
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

export async function startMockProvider(
  models: string[] = ["e2e-model", "e2e-other"],
): Promise<MockProvider> {
  const apiKey = `mock-${randomBytes(16).toString("hex")}`;
  const answer = `Hello from the mock provider ${randomBytes(4).toString("hex")}.`;
  const calls: MockCall[] = [];
  const state = {
    listCalls: 0,
    mode: {} as MockMode,
    usage: { prompt: 3, completion: 7 },
  };

  const server = createServer((request, response) => {
    void (async () => {
      const body = await bodyOf(request);
      const send = (status: number, value: unknown) => {
        response.writeHead(status, { "content-type": "application/json" });
        response.end(JSON.stringify(value));
      };
      const authorized = request.headers.authorization === `Bearer ${apiKey}`;
      if (request.method === "GET" && request.url === "/v1/models") {
        state.listCalls += 1;
        if (!authorized) {
          send(401, {
            error: {
              message: "Incorrect API key.",
              type: "invalid_request_error",
            },
          });
          return;
        }
        send(200, {
          object: "list",
          data: models.map((id) => ({ id, object: "model" })),
        });
        return;
      }
      if (request.method !== "POST" || request.url !== "/v1/chat/completions") {
        send(404, {
          error: { message: "Unknown path.", type: "invalid_request_error" },
        });
        return;
      }
      const model =
        typeof body === "object" && body !== null && "model" in body
          ? body.model
          : null;
      const streaming =
        typeof body === "object" &&
        body !== null &&
        "stream" in body &&
        body.stream === true;
      calls.push({ authorized, model });
      if (!authorized) {
        send(401, {
          error: {
            message: "Incorrect API key.",
            type: "invalid_request_error",
            code: "invalid_api_key",
          },
        });
        return;
      }
      const { mode } = state;
      if (mode.delayMs !== undefined)
        await new Promise((done) => setTimeout(done, mode.delayMs));
      if (mode.status !== undefined) {
        send(mode.status, {
          error: { message: "The mock fails.", type: "server_error" },
        });
        return;
      }
      if (mode.breakAfterFirstChunk === true && streaming) {
        response.writeHead(200, { "content-type": "text/event-stream" });
        response.write(
          `data: ${JSON.stringify({ id: "chatcmpl-e2e", object: "chat.completion.chunk", model, choices: [{ index: 0, delta: { role: "assistant", content: "Hel" } }] })}\n\n`,
          () => {
            request.socket.destroy();
          },
        );
        return;
      }
      if (streaming) {
        // The answer in three pieces, then the finish and the usage.
        const words = answer.split(" ");
        const pieces = [
          words.slice(0, 2).join(" "),
          ` ${words.slice(2, -1).join(" ")}`,
          ` ${words.at(-1) ?? ""}`,
        ];
        const chunk = (delta: object, finish: string | null, usage?: object) =>
          `data: ${JSON.stringify({ id: "chatcmpl-e2e", object: "chat.completion.chunk", model, choices: [{ index: 0, delta, finish_reason: finish }], ...(usage === undefined ? {} : { usage }) })}\n\n`;
        response.writeHead(200, { "content-type": "text/event-stream" });
        for (const piece of pieces) response.write(chunk({ content: piece }, null));
        response.write(
          chunk({}, "stop", {
            prompt_tokens: state.usage.prompt,
            completion_tokens: state.usage.completion,
            total_tokens: state.usage.prompt + state.usage.completion,
          }),
        );
        response.end("data: [DONE]\n\n");
        return;
      }
      send(200, {
        id: "chatcmpl-e2e",
        object: "chat.completion",
        created: Math.floor(Date.now() / 1000),
        model,
        choices: [
          {
            index: 0,
            message: { role: "assistant", content: answer },
            finish_reason: "stop",
          },
        ],
        usage: {
          prompt_tokens: state.usage.prompt,
          completion_tokens: state.usage.completion,
          total_tokens: state.usage.prompt + state.usage.completion,
        },
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
  if (typeof address !== "object" || address === null)
    throw new Error("The mock provider has no port.");

  return {
    baseUrl: `http://127.0.0.1:${address.port}/v1`,
    apiKey,
    answer,
    models,
    calls,
    get listCalls() {
      return state.listCalls;
    },
    get usage() {
      return state.usage;
    },
    set usage(value: { prompt: number; completion: number }) {
      state.usage = value;
    },
    get mode() {
      return state.mode;
    },
    set mode(value: MockMode) {
      state.mode = value;
    },
    close: () =>
      new Promise<void>((done) => {
        server.closeAllConnections();
        server.close(() => {
          done();
        });
      }),
  };
}
