/** Where calls go. The key is held out of reach of `JSON.stringify`, `String` and `console.log`. */

export type TargetKind = "gateway" | "openai" | "anthropic" | "gemini" | "azure";

export interface TargetSpec {
  kind: TargetKind;
  baseUrl: string;
  key: string;
  apiVersion?: string;
}

const specs = new WeakMap<Target, TargetSpec>();

export class Target {
  readonly kind: TargetKind;
  readonly baseUrl: string;

  /** @internal use `gateway()`, `openai()`, ... */
  constructor(spec: TargetSpec) {
    this.kind = spec.kind;
    this.baseUrl = spec.baseUrl;
    specs.set(this, spec);
  }

  toJSON(): Record<string, string> {
    return { kind: this.kind, baseUrl: this.baseUrl, key: "[redacted]" };
  }

  toString(): string {
    return `Target(${this.kind}, ${this.baseUrl}, key=[redacted])`;
  }

  [Symbol.for("nodejs.util.inspect.custom")](): string {
    return this.toString();
  }
}

/** @internal */
export function specOf(target: Target): TargetSpec {
  const spec = specs.get(target);
  if (!spec) throw new TypeError("not a target: build one with gateway(), openai(), ...");
  return spec;
}

const need = (name: string, v: unknown): string => {
  if (typeof v !== "string" || v === "") throw new TypeError(`${name} is required`);
  return v;
};

/** An Ultrafast gateway; a trailing `/v1` on `baseUrl` is accepted and ignored. */
export function gateway(o: { baseUrl: string; key: string }): Target {
  return new Target({ kind: "gateway", baseUrl: need("baseUrl", o.baseUrl), key: need("key", o.key) });
}

/** OpenAI directly; `baseUrl` includes the version segment (default `https://api.openai.com/v1`). */
export function openai(o: { key: string; baseUrl?: string }): Target {
  return new Target({ kind: "openai", baseUrl: o.baseUrl ?? "https://api.openai.com/v1", key: need("key", o.key) });
}

export function anthropic(o: { key: string; baseUrl?: string }): Target {
  return new Target({ kind: "anthropic", baseUrl: o.baseUrl ?? "https://api.anthropic.com", key: need("key", o.key) });
}

export function gemini(o: { key: string; baseUrl?: string }): Target {
  return new Target({
    kind: "gemini",
    baseUrl: o.baseUrl ?? "https://generativelanguage.googleapis.com",
    key: need("key", o.key),
  });
}

/** Azure OpenAI; the call's `model` is the deployment name. */
export function azure(o: { endpoint: string; key: string; apiVersion?: string }): Target {
  const spec: TargetSpec = { kind: "azure", baseUrl: need("endpoint", o.endpoint), key: need("key", o.key) };
  if (o.apiVersion !== undefined) spec.apiVersion = o.apiVersion;
  return new Target(spec);
}

/**
 * Any API speaking OpenAI's format (Groq, Mistral, OpenRouter, Ollama); `baseUrl` includes `/v1`.
 * The key may be empty or left out for a server that needs none (Ollama): no Authorization header is sent.
 */
export function openaiCompatible(o: { baseUrl: string; key?: string }): Target {
  const key = o.key ?? "";
  if (typeof key !== "string") throw new TypeError("key is a string");
  return new Target({ kind: "openai", baseUrl: need("baseUrl", o.baseUrl), key });
}
