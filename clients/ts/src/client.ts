import { UltrafastError, errorFromJson } from "./errors.js";
import { HostFailure, clampStatus, readAnswer, send, type Request } from "./http.js";
import { decode, usageOf } from "./stream.js";
import { specOf, type Target } from "./target.js";
import type {
  ChatRequest,
  ChatResponse,
  EmbeddingsRequest,
  EmbeddingsResponse,
  ContentPart,
  StreamEvent,
  Tool,
  ToolCall,
  ToolChoice,
} from "./types.js";
import { wasm, type Wasm } from "./wasm.js";

export const DEFAULT_TIMEOUT_MS = 120_000;
/** The most a timer can hold (a larger delay fires at once). */
const MAX_TIMEOUT_MS = 2 ** 31 - 1;
export const DEFAULT_MAX_RESPONSE_BYTES = 32 * 1024 * 1024;

export interface ClientOptions {
  /** Your own `fetch` (tests, proxies, edge runtimes). It must not follow redirects with credentials. */
  fetch?: typeof fetch;
  /**
   * Milliseconds for the whole of a `chat` or `embed`, for the answer to
   * start a stream, and for each silent stretch inside a stream (default
   * 120000). A finite number from 1 to 2^31-1; anything else is a TypeError.
   */
  timeoutMs?: number;
  /** The most a `chat` or `embed` answer, or any error body, may hold (default 32 MiB). Streams are not capped in total. */
  maxResponseBytes?: number;
}

const ROLES = ["system", "user", "assistant", "tool"];

const bad = (message: string): HostFailure => new HostFailure("invalid_request", message);
const isString = (v: unknown): v is string => typeof v === "string";
const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

function checkContent(content: unknown): string | ContentPart[] | null {
  if (content === null || isString(content)) return content;
  if (!Array.isArray(content)) throw bad("a message content is a string, a list of parts, or null");
  return content.map((p: unknown): ContentPart => {
    if (isObject(p) && p["type"] === "text" && isString(p["text"])) return { type: "text", text: p["text"] };
    if (isObject(p) && p["type"] === "image" && isString(p["url"])) return { type: "image", url: p["url"] };
    throw bad('a content part is {type:"text",text} or {type:"image",url}');
  });
}

function checkMessages(messages: unknown): unknown[] {
  if (!Array.isArray(messages)) throw bad("messages must be a list");
  return messages.map((m: unknown) => {
    const o = isObject(m) ? m : {};
    const role = o["role"];
    if (!isString(role) || !ROLES.includes(role)) throw bad(`a message role is one of ${ROLES.join(", ")}`);
    const content = checkContent(o["content"]);
    const toolCalls = o["toolCalls"];
    const toolCallId = o["toolCallId"];
    if (toolCalls !== undefined) {
      if (role !== "assistant") throw bad("toolCalls are only for assistant messages");
      if (
        !Array.isArray(toolCalls) ||
        !toolCalls.every((c: unknown) => isObject(c) && isString(c["id"]) && isString(c["name"]) && isString(c["arguments"]))
      ) {
        throw bad("toolCalls is a list of {id, name, arguments} strings");
      }
    }
    if (role === "tool") {
      if (!isString(toolCallId) || toolCallId === "") throw bad("a tool message needs a toolCallId");
    } else if (toolCallId !== undefined) {
      throw bad("toolCallId is only for tool messages");
    }
    if (content === null && !(Array.isArray(toolCalls) && toolCalls.length > 0)) {
      throw bad("a message needs content unless it is an assistant message with toolCalls");
    }
    return {
      role,
      content,
      tool_calls: toolCalls === undefined ? undefined : (toolCalls as ToolCall[]).map((c) => ({ id: c.id, name: c.name, arguments: c.arguments })),
      tool_call_id: toolCallId,
    };
  });
}

function checkTools(tools: unknown): Tool[] | undefined {
  if (tools === undefined) return undefined;
  if (!Array.isArray(tools) || !tools.every((t: unknown) => isObject(t) && isString(t["name"]))) {
    throw bad("tools is a list of {name, description?, parameters?, strict?}");
  }
  return (tools as Tool[]).map((t) => ({ name: t.name, description: t.description, parameters: t.parameters, strict: t.strict }));
}

function checkToolChoice(choice: unknown): ToolChoice | undefined {
  if (choice === undefined) return undefined;
  if (choice === "auto" || choice === "none" || choice === "required") return choice;
  if (isObject(choice) && isString(choice["name"])) return { name: choice["name"] };
  throw bad('toolChoice is "auto", "none", "required" or {name}');
}

interface Built {
  http: Request;
  kind: string;
}

/**
 * Talks to a gateway or a provider. Building requests and reading answers is
 * the Rust code compiled to WebAssembly; the network is the runtime's `fetch`.
 * No retries, routing, caching or circuit breaking: every error says whether
 * trying again could help.
 */
export class Client {
  readonly #target: Target;
  readonly #fetch: typeof fetch;
  readonly #timeoutMs: number;
  readonly #max: number;

  constructor(target: Target, options: ClientOptions = {}) {
    specOf(target);
    this.#target = target;
    this.#fetch = options.fetch ?? ((...a) => globalThis.fetch(...a));
    const t = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    if (typeof t !== "number" || !Number.isFinite(t) || t <= 0 || t > MAX_TIMEOUT_MS) {
      throw new TypeError(`timeoutMs must be a number from 1 to ${MAX_TIMEOUT_MS} (no timeout is not offered)`);
    }
    const max = options.maxResponseBytes ?? DEFAULT_MAX_RESPONSE_BYTES;
    if (typeof max !== "number" || !Number.isFinite(max) || max <= 0) {
      throw new TypeError("maxResponseBytes must be a positive number");
    }
    this.#timeoutMs = t;
    this.#max = max;
  }

  toJSON(): Record<string, unknown> {
    return { target: this.#target.toJSON(), timeoutMs: this.#timeoutMs };
  }

  toString(): string {
    return `Client(${this.#target.toString()})`;
  }

  [Symbol.for("nodejs.util.inspect.custom")](): string {
    return this.toString();
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    return this.#run(async (w, ctl) => {
      const built = this.#buildChat(w, request, false);
      const deadline = Date.now() + this.#timeoutMs;
      const resp = await send(this.#fetch, built.http, ctl, this.#timeoutMs);
      const a = await readAnswer(resp, this.#max, deadline, ctl);
      const out = JSON.parse(w.parseResponse(built.kind, a.status, a.body, a.retryAfter)) as {
        id: string | null;
        model: string | null;
        content: string;
        tool_calls: ToolCall[];
        finish_reason: string | null;
        usage: { input_tokens: number; output_tokens: number } | null;
      };
      return {
        id: out.id,
        model: out.model,
        content: out.content,
        toolCalls: out.tool_calls.map((c) => ({ id: c.id, name: c.name, arguments: c.arguments })),
        finishReason: out.finish_reason,
        usage: usageOf(out.usage),
      };
    });
  }

  /**
   * Events in order, then at most one error: a stream that fails or is cut
   * short after some text throws after the text, never ends silently. The
   * request starts on the first `next()`; stopping early (`break`) cancels it.
   */
  async *chatStream(request: ChatRequest): AsyncGenerator<StreamEvent> {
    const w = await this.#loaded();
    const ctl = new AbortController();
    try {
      const built = this.#buildChat(w, request, true);
      const deadline = Date.now() + this.#timeoutMs;
      const resp = await send(this.#fetch, built.http, ctl, this.#timeoutMs);
      if (clampStatus(resp.status) >= 300 || resp.type === "opaqueredirect") {
        const a = await readAnswer(resp, this.#max, deadline, ctl);
        throw w.classifyError(a.status, a.body, a.retryAfter);
      }
      yield* decode(w, built.kind, resp, this.#timeoutMs, ctl);
    } catch (e) {
      ctl.abort();
      throw this.#fail(w, e);
    }
  }

  async embed(request: EmbeddingsRequest): Promise<EmbeddingsResponse> {
    return this.#run(async (w, ctl) => {
      const spec = specOf(this.#target);
      const input = typeof request.input === "string" ? [request.input] : request.input;
      const body = JSON.stringify({
        model: request.model,
        input,
        dimensions: request.dimensions,
        tags: request.tags,
      });
      const http = JSON.parse(w.buildEmbeddingsRequest(this.#targetJson(), body)) as Request;
      const deadline = Date.now() + this.#timeoutMs;
      const resp = await send(this.#fetch, http, ctl, this.#timeoutMs);
      const a = await readAnswer(resp, this.#max, deadline, ctl);
      const out = JSON.parse(w.parseEmbeddings(spec.kind, a.status, a.body, request.model, a.retryAfter)) as {
        model: string;
        vectors: number[][];
        prompt_tokens: number | null;
      };
      return { model: out.model, vectors: out.vectors, promptTokens: out.prompt_tokens };
    });
  }

  #targetJson(): string {
    const s = specOf(this.#target);
    return JSON.stringify({ kind: s.kind, base_url: s.baseUrl, api_key: s.key, api_version: s.apiVersion });
  }

  #buildChat(w: Wasm, r: ChatRequest, stream: boolean): Built {
    const stop = r.stop === undefined ? undefined : typeof r.stop === "string" ? [r.stop] : r.stop;
    const body = JSON.stringify({
      model: r.model,
      messages: checkMessages(r.messages),
      tools: checkTools(r.tools),
      tool_choice: checkToolChoice(r.toolChoice),
      parallel_tool_calls: r.parallelToolCalls,
      max_tokens: r.maxTokens,
      temperature: r.temperature,
      top_p: r.topP,
      stop,
      stream,
      tags: r.tags,
    });
    const http = JSON.parse(w.buildRequest(this.#targetJson(), body)) as Request;
    return { http, kind: specOf(this.#target).kind };
  }

  async #loaded(): Promise<Wasm> {
    return wasm();
  }

  async #run<T>(op: (w: Wasm, ctl: AbortController) => Promise<T>): Promise<T> {
    const w = await this.#loaded();
    const ctl = new AbortController();
    try {
      return await op(w, ctl);
    } catch (e) {
      ctl.abort();
      throw this.#fail(w, e);
    }
  }

  /** Every thrown error is an `UltrafastError` whose message has the key removed. */
  #fail(w: Wasm, e: unknown): UltrafastError {
    let json: string;
    if (e instanceof HostFailure) {
      json = w.hostError(e.hostKind, e.message);
    } else if (typeof e === "string") {
      json = e;
    } else {
      json = w.hostError("malformed", "an unexpected error occurred");
    }
    const key = specOf(this.#target).key;
    try {
      return errorFromJson(w.scrubError(json, key));
    } catch {
      return errorFromJson(w.scrubError(w.hostError("malformed", "an unexpected error occurred"), key));
    }
  }
}

