import { UltrafastError, errorFromJson } from "./errors.js";
import { HostFailure, clampStatus, readAnswer, send, type Request } from "./http.js";
import { decode, usageOf } from "./stream.js";
import { specOf, type Target } from "./target.js";
import type {
  ChatRequest,
  ChatResponse,
  EmbeddingsRequest,
  EmbeddingsResponse,
  Message,
  StreamEvent,
} from "./types.js";
import { wasm, type Wasm } from "./wasm.js";

export const DEFAULT_TIMEOUT_MS = 120_000;
export const DEFAULT_MAX_RESPONSE_BYTES = 32 * 1024 * 1024;

export interface ClientOptions {
  /** Your own `fetch` (tests, proxies, edge runtimes). It must not follow redirects with credentials. */
  fetch?: typeof fetch;
  /**
   * Milliseconds for the whole of a `chat` or `embed`, for the answer to
   * start a stream, and for each silent stretch inside a stream (default 120000).
   */
  timeoutMs?: number;
  /** The most a `chat` or `embed` answer, or any error body, may hold (default 32 MiB). Streams are not capped in total. */
  maxResponseBytes?: number;
}

const ROLES = ["system", "user", "assistant"];

function checkMessages(messages: unknown): Message[] {
  if (!Array.isArray(messages)) throw new HostFailure("invalid_request", "messages must be a list");
  return messages.map((m: unknown) => {
    const { role, content } = (m ?? {}) as Partial<Message>;
    if (typeof role !== "string" || !ROLES.includes(role)) {
      throw new HostFailure("invalid_request", `a message role is one of ${ROLES.join(", ")}`);
    }
    if (typeof content !== "string") {
      throw new HostFailure("invalid_request", "a message content is a string (text only)");
    }
    return { role, content } as Message;
  });
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
    this.#timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    this.#max = options.maxResponseBytes ?? DEFAULT_MAX_RESPONSE_BYTES;
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
        finish_reason: string | null;
        usage: { input_tokens: number; output_tokens: number } | null;
      };
      return {
        id: out.id,
        model: out.model,
        content: out.content,
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

