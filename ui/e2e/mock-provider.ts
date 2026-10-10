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
  /** What every completion answers; a test may change it at any time (at least three words, for a stream). */
  answer: string;
  /** The arguments of the tool call that is answered when the request has tools and no tool result. */
  toolArguments: string;
  /** The models `GET /v1/models` lists. */
  models: string[];
  /** Completions asked for, in order. */
  calls: MockCall[];
  /** The roles of the messages each completion carried, in order. */
  roles: string[][];
  /** The text of each message of each completion, in order (a message with parts is its text parts joined). */
  texts: string[][];
  /** Image generations asked for, in order. */
  imageCalls: { authorized: boolean; body: unknown }[];
  /** Audio calls asked for, in order: the form of a transcription (its text fields and file) or the JSON of a speech. */
  audioCalls: {
    path: string;
    authorized: boolean;
    contentType: string;
    /** The bytes of the file part of a transcription. */
    fileBytes: number;
    /** The names of the form's text fields, or the JSON keys of a speech. */
    fields: string[];
    body: unknown;
  }[];
  /** How many times the model list was asked for. */
  listCalls: number;
  /** The token usage every completion reports; a test may change it at any time. */
  usage: { prompt: number; completion: number };
  mode: MockMode;
  close: () => Promise<void>;
}

async function rawOf(request: IncomingMessage): Promise<Buffer> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks);
}

function jsonOf(raw: Buffer): unknown {
  try {
    return JSON.parse(raw.toString("utf8")) as unknown;
  } catch {
    return null;
  }
}

/** The field names of a multipart form, and the size of its `file` part. */
function formOf(raw: Buffer, contentType: string): { fields: string[]; fileBytes: number } {
  const boundary = /boundary=(?:"([^"]+)"|([^;]+))/.exec(contentType);
  const marker = boundary?.[1] ?? boundary?.[2];
  const out = { fields: [] as string[], fileBytes: 0 };
  if (marker === undefined) return out;
  const text = raw.toString("latin1");
  for (const part of text.split(`--${marker}`)) {
    const name = /name="([^"]+)"/.exec(part)?.[1];
    if (name === undefined) continue;
    const bodyAt = part.indexOf("\r\n\r\n");
    if (name === "file") out.fileBytes = Math.max(0, part.length - bodyAt - 4 - 2);
    else out.fields.push(name);
  }
  return out;
}

/** A tenth of a second of silence as a WAV file: what the mock answers to a speech call. */
export function silentWav(): Buffer {
  const samples = 800;
  const header = Buffer.alloc(44);
  header.write("RIFF", 0);
  header.writeUInt32LE(36 + samples, 4);
  header.write("WAVEfmt ", 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(8000, 24);
  header.writeUInt32LE(8000, 28);
  header.writeUInt16LE(1, 32);
  header.writeUInt16LE(8, 34);
  header.write("data", 36);
  header.writeUInt32LE(samples, 40);
  return Buffer.concat([header, Buffer.alloc(samples, 128)]);
}

/** A 1 x 1 PNG, base64: what the mock answers to an image generation. */
export const TINY_PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

export async function startMockProvider(
  models: string[] = ["e2e-model", "e2e-other"],
): Promise<MockProvider> {
  const apiKey = `mock-${randomBytes(16).toString("hex")}`;
  const first = `Hello from the mock provider ${randomBytes(4).toString("hex")}.`;
  const toolArguments = JSON.stringify({ city: "Oslo" });
  const calls: MockCall[] = [];
  const roles: string[][] = [];
  const texts: string[][] = [];
  const imageCalls: { authorized: boolean; body: unknown }[] = [];
  const audioCalls: MockProvider["audioCalls"] = [];
  const state = {
    answer: first,
    listCalls: 0,
    mode: {} as MockMode,
    usage: { prompt: 3, completion: 7 },
  };

  const server = createServer((request, response) => {
    void (async () => {
      const raw = await rawOf(request);
      const body = jsonOf(raw);
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
      if (request.method === "POST" && request.url === "/v1/images/generations") {
        imageCalls.push({ authorized, body });
        if (!authorized) {
          send(401, { error: { message: "Incorrect API key.", type: "invalid_request_error" } });
          return;
        }
        const wanted =
          typeof body === "object" && body !== null && "n" in body && typeof body.n === "number" ? body.n : 1;
        send(200, {
          created: Math.floor(Date.now() / 1000),
          data: Array.from({ length: wanted }, () => ({ b64_json: TINY_PNG })),
          usage: { input_tokens: state.usage.prompt, output_tokens: state.usage.completion, total_tokens: state.usage.prompt + state.usage.completion },
        });
        return;
      }
      if (
        request.method === "POST" &&
        (request.url === "/v1/audio/transcriptions" || request.url === "/v1/audio/speech")
      ) {
        const contentType = request.headers["content-type"] ?? "";
        const form = request.url === "/v1/audio/speech" ? null : formOf(raw, contentType);
        audioCalls.push({
          path: request.url,
          authorized,
          contentType,
          fileBytes: form?.fileBytes ?? 0,
          fields: form?.fields ?? (typeof body === "object" && body !== null ? Object.keys(body) : []),
          body,
        });
        if (!authorized) {
          send(401, { error: { message: "Incorrect API key.", type: "invalid_request_error" } });
          return;
        }
        if (form === null) {
          response.writeHead(200, { "content-type": "audio/wav" });
          response.end(silentWav());
        } else {
          send(200, { text: "Hello from the mock recording." });
        }
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
      const messages =
        typeof body === "object" && body !== null && "messages" in body && Array.isArray(body.messages)
          ? (body.messages as { role?: unknown; content?: unknown }[])
          : [];
      const hasTools =
        typeof body === "object" && body !== null && "tools" in body && Array.isArray(body.tools) && body.tools.length > 0;
      const hasToolResult = messages.some((message) => message.role === "tool");
      const callsTool = hasTools && !hasToolResult;
      calls.push({ authorized, model });
      roles.push(messages.map((message) => String(message.role)));
      texts.push(
        messages.map((message) =>
          typeof message.content === "string"
            ? message.content
            : Array.isArray(message.content)
              ? message.content
                  .map((part: { text?: unknown }) => (typeof part.text === "string" ? part.text : ""))
                  .join("")
              : "",
        ),
      );
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
      const toolCalls = [
        { id: "call_e2e_1", type: "function", function: { name: "weather", arguments: toolArguments } },
      ];
      if (streaming && callsTool) {
        // The call in pieces: id and name in the first, the arguments in two more.
        const cut = Math.floor(toolArguments.length / 2);
        const piece = (call: object, finish: string | null, usage?: object) =>
          `data: ${JSON.stringify({ id: "chatcmpl-e2e", object: "chat.completion.chunk", model, choices: [{ index: 0, delta: { tool_calls: [call] }, finish_reason: finish }], ...(usage === undefined ? {} : { usage }) })}\n\n`;
        response.writeHead(200, { "content-type": "text/event-stream" });
        response.write(piece({ index: 0, id: "call_e2e_1", type: "function", function: { name: "weather", arguments: "" } }, null));
        response.write(piece({ index: 0, function: { arguments: toolArguments.slice(0, cut) } }, null));
        response.write(
          piece({ index: 0, function: { arguments: toolArguments.slice(cut) } }, "tool_calls", {
            prompt_tokens: state.usage.prompt,
            completion_tokens: state.usage.completion,
            total_tokens: state.usage.prompt + state.usage.completion,
          }),
        );
        response.end("data: [DONE]\n\n");
        return;
      }
      if (callsTool) {
        send(200, {
          id: "chatcmpl-e2e",
          object: "chat.completion",
          created: Math.floor(Date.now() / 1000),
          model,
          choices: [
            { index: 0, message: { role: "assistant", content: null, tool_calls: toolCalls }, finish_reason: "tool_calls" },
          ],
          usage: {
            prompt_tokens: state.usage.prompt,
            completion_tokens: state.usage.completion,
            total_tokens: state.usage.prompt + state.usage.completion,
          },
        });
        return;
      }
      if (streaming) {
        // The answer in three pieces, then the finish and the usage.
        const words = state.answer.split(" ");
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
            message: { role: "assistant", content: state.answer },
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
    get answer() {
      return state.answer;
    },
    set answer(value: string) {
      state.answer = value;
    },
    toolArguments,
    models,
    calls,
    roles,
    texts,
    imageCalls,
    audioCalls,
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
