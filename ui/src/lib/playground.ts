// What the playground knows: how its parameters are checked, the request it
// sends, how the stream that answers is read, and what the call cost. Pure.

export interface Message {
  role: "user" | "assistant";
  content: string;
}

/** The parameters as they are typed. */
export interface Params {
  maxTokens: string;
  temperature: string;
  topP: string;
  stop: string;
}

/** The parameters that were given, as the request names them. */
export interface ParamValues {
  max_tokens?: number;
  temperature?: number;
  top_p?: number;
  stop?: string[];
}

export interface Checked {
  values: ParamValues;
  /** A text for each field that is not valid, by the name of the field in `Params`. */
  errors: Partial<Record<keyof Params, string>>;
}

export const MAX_STOP_SEQUENCES = 4;
const MAX_TOKENS_LIMIT = 2_000_000_000;

const WHOLE = /^\d+$/;
const NUMBER = /^[+-]?(?:\d+\.?\d*|\.\d+)$/;

function numberIn(text: string, low: number, high: number): number | null {
  if (!NUMBER.test(text)) return null;
  const value = Number(text);
  return value >= low && value <= high ? value : null;
}

/** Reads the typed parameters. What is left empty is not sent. */
export function checkParams(params: Params): Checked {
  const values: ParamValues = {};
  const errors: Checked["errors"] = {};

  const maxTokens = params.maxTokens.trim();
  if (maxTokens !== "") {
    const value = WHOLE.test(maxTokens) ? Number(maxTokens) : 0;
    if (value >= 1 && value <= MAX_TOKENS_LIMIT) values.max_tokens = value;
    else errors.maxTokens = "Enter a whole number of 1 or more.";
  }
  const temperature = params.temperature.trim();
  if (temperature !== "") {
    const value = numberIn(temperature, 0, 2);
    if (value === null) errors.temperature = "Enter a number from 0 to 2.";
    else values.temperature = value;
  }
  const topP = params.topP.trim();
  if (topP !== "") {
    const value = numberIn(topP, 0, 1);
    if (value === null) errors.topP = "Enter a number from 0 to 1.";
    else values.top_p = value;
  }
  const stop = params.stop
    .split(",")
    .map((part) => part.trim())
    .filter((part) => part !== "");
  if (stop.length > MAX_STOP_SEQUENCES) {
    errors.stop = `Give at most ${String(MAX_STOP_SEQUENCES)} stop sequences.`;
  } else if (stop.length > 0) {
    values.stop = stop;
  }
  // A call with a field that is not valid sends nothing of what was valid.
  return Object.keys(errors).length > 0 ? { values: {}, errors } : { values, errors };
}

export interface ChatRequestBody {
  model: string;
  stream: true;
  messages: { role: "system" | "user" | "assistant"; content: string }[];
  max_tokens?: number;
  temperature?: number;
  top_p?: number;
  stop?: string[];
}

/** The request of the playground: always a stream. */
export function requestBody(
  model: string,
  system: string,
  messages: readonly Message[],
  values: ParamValues,
): ChatRequestBody {
  const prompt = system.trim();
  return {
    model,
    stream: true,
    ...values,
    messages: [...(prompt === "" ? [] : [{ role: "system" as const, content: prompt }]), ...messages],
  };
}

/**
 * Cuts a stream of server-sent events into the data of its events, whatever
 * way the network cut it into chunks.
 */
export class SseReader {
  private buffer = "";

  /** The data of every event that the text completed. */
  feed(text: string): string[] {
    this.buffer += text.replace(/\r\n?/g, "\n");
    const out: string[] = [];
    for (;;) {
      const end = this.buffer.indexOf("\n\n");
      if (end === -1) return out;
      const block = this.buffer.slice(0, end);
      this.buffer = this.buffer.slice(end + 2);
      const data = block
        .split("\n")
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice(5).replace(/^ /, ""));
      if (data.length > 0) out.push(data.join("\n"));
    }
  }
}

export interface Usage {
  input: number;
  output: number;
}

/** What one event of the stream says. An event with nothing to use is `{}`. */
export interface Chunk {
  text?: string;
  usage?: Usage;
  /** The model that answered, as the provider names it. */
  model?: string;
  error?: string;
  done?: true;
}

export const BROKE_OFF = "The answer broke off.";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Reads the data of one event of the answer, which is in the OpenAI shape. */
export function chunkOf(data: string): Chunk {
  if (data.trim() === "[DONE]") return { done: true };
  let body: unknown;
  try {
    body = JSON.parse(data);
  } catch {
    return {};
  }
  if (!isRecord(body)) return {};
  if ("error" in body) {
    const { error } = body;
    return {
      error: isRecord(error) && typeof error.message === "string" ? error.message : BROKE_OFF,
    };
  }
  const chunk: Chunk = {};
  const choices = Array.isArray(body.choices) ? (body.choices as unknown[]) : [];
  const first = choices[0];
  if (isRecord(first) && isRecord(first.delta) && typeof first.delta.content === "string") {
    if (first.delta.content !== "") chunk.text = first.delta.content;
  }
  const { usage } = body;
  if (isRecord(usage) && typeof usage.prompt_tokens === "number" && typeof usage.completion_tokens === "number") {
    chunk.usage = { input: usage.prompt_tokens, output: usage.completion_tokens };
    if (typeof body.model === "string") chunk.model = body.model;
  }
  return chunk;
}

export interface Prices {
  input_price_micros: number | null;
  output_price_micros: number | null;
}

/**
 * What the call cost, in millionths of a dollar: the tokens at the price per
 * million tokens of the model, to a whole micro. `null` when the model has no
 * price, or is not known.
 */
export function costMicros(usage: Usage, prices: Prices | null): number | null {
  if (prices === null) return null;
  const { input_price_micros: input, output_price_micros: output } = prices;
  if (input === null || output === null) return null;
  return Math.round((usage.input * input + usage.output * output) / 1_000_000);
}

function shellQuote(text: string): string {
  return `'${text.replace(/'/g, `'"'"'`)}'`;
}

/** The call as a `curl` command for `/v1`, with a placeholder where the key goes. */
export function curlOf(origin: string, body: object): string {
  return [
    `curl ${origin}/v1/chat/completions`,
    "  -H 'Authorization: Bearer <your key>'",
    "  -H 'Content-Type: application/json'",
    `  -d ${shellQuote(JSON.stringify(body))}`,
  ].join(" \\\n");
}

/** How long to wait, as the sentence that follows a refusal; nothing when it is not known. */
export function retryText(seconds: number | null): string {
  if (seconds === null) return "";
  if (seconds < 60) return ` Try again in ${String(seconds)} ${seconds === 1 ? "second" : "seconds"}.`;
  return ` Try again in ${String(Math.ceil(seconds / 60))} minutes.`;
}
