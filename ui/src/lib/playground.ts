// What the playground knows: how its parameters are checked, the request it
// sends, how the stream that answers is read, and what the call cost. Pure.

/** An image attached to a message of the user, as a `data:` URL. */
export interface Attachment {
  /** The file name, which is also its alt text. */
  name: string;
  url: string;
}

/** A call of a function the model asks for, as the OpenAI shape has it. */
export type ToolCall = {
  id: string;
  type: "function";
  function: { name: string; arguments: string };
};

export type Message =
  | { role: "user"; content: string; images?: readonly Attachment[] }
  | { role: "assistant"; content: string | null; tool_calls?: ToolCall[] }
  | { role: "tool"; content: string; tool_call_id: string };

/** A tool as it is typed: a function with a name; the rest is the provider's. */
export type ToolDef = {
  type: "function";
  function: { name: string } & Record<string, unknown>;
};

/** What the answer must look like, as the request names it. */
export type ResponseFormatBody =
  | { type: "json_object" }
  | { type: "json_schema"; json_schema: { name: string; schema: Record<string, unknown> } };

export type ResponseFormatKind = "text" | "json_object" | "json_schema";

export type ToolChoice = "none" | "required" | { type: "function"; function: { name: string } };

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
  tools?: ToolDef[];
  tool_choice?: ToolChoice;
  response_format?: ResponseFormatBody;
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

type WirePart =
  | { type: "text"; text: string }
  | { type: "image_url"; image_url: { url: string } };

export type WireMessage =
  | { role: "system"; content: string }
  | { role: "user"; content: string | WirePart[] }
  | { role: "assistant"; content: string | null; tool_calls?: ToolCall[] }
  | { role: "tool"; content: string; tool_call_id: string };

export interface ChatRequestBody {
  model: string;
  stream: true;
  messages: WireMessage[];
  max_tokens?: number;
  temperature?: number;
  top_p?: number;
  stop?: string[];
  tools?: ToolDef[];
  tool_choice?: ToolChoice;
  response_format?: ResponseFormatBody;
}

function wireOf(message: Message): WireMessage {
  switch (message.role) {
    case "tool":
      return { role: "tool", content: message.content, tool_call_id: message.tool_call_id };
    case "assistant":
      return message.tool_calls === undefined
        ? { role: "assistant", content: message.content }
        : { role: "assistant", content: message.content, tool_calls: message.tool_calls };
    case "user":
      if (message.images === undefined || message.images.length === 0) {
        return { role: "user", content: message.content };
      }
      return {
        role: "user",
        content: [
          ...(message.content === "" ? [] : [{ type: "text" as const, text: message.content }]),
          ...message.images.map((image) => ({ type: "image_url" as const, image_url: { url: image.url } })),
        ],
      };
  }
}

export const TOOLS_INVALID =
  "Tools must be a JSON array of functions, each with a type of function and a name.";

export interface CheckedTools {
  /** The tools to send; nothing when none were given. */
  tools: ToolDef[] | undefined;
  /** The names of the functions, for the choice of one. */
  names: string[];
  error: string | undefined;
}

/** Reads the typed tools. Empty text and an empty array are no tools. */
export function checkTools(text: string): CheckedTools {
  const none: CheckedTools = { tools: undefined, names: [], error: undefined };
  if (text.trim() === "") return none;
  const refused: CheckedTools = { tools: undefined, names: [], error: TOOLS_INVALID };
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return refused;
  }
  if (!Array.isArray(parsed)) return refused;
  if (parsed.length === 0) return none;
  const names: string[] = [];
  for (const item of parsed as unknown[]) {
    if (!isRecord(item) || item.type !== "function" || !isRecord(item.function)) return refused;
    const { name } = item.function;
    if (typeof name !== "string" || name === "") return refused;
    names.push(name);
  }
  return { tools: parsed as ToolDef[], names, error: undefined };
}

export const SCHEMA_INVALID = 'The schema must be a JSON object, such as {"type":"object"}.';

export interface CheckedFormat {
  /** What to send; nothing for plain text. */
  format: ResponseFormatBody | undefined;
  error: string | undefined;
}

/** Reads the chosen response format. Text is the default and is not sent. */
export function checkResponseFormat(kind: ResponseFormatKind, schemaText: string): CheckedFormat {
  if (kind === "text") return { format: undefined, error: undefined };
  if (kind === "json_object") return { format: { type: "json_object" }, error: undefined };
  const refused: CheckedFormat = { format: undefined, error: SCHEMA_INVALID };
  let parsed: unknown;
  try {
    parsed = JSON.parse(schemaText);
  } catch {
    return refused;
  }
  if (!isRecord(parsed)) return refused;
  return { format: { type: "json_schema", json_schema: { name: "response", schema: parsed } }, error: undefined };
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
    messages: [
      ...(prompt === "" ? [] : [{ role: "system" as const, content: prompt }]),
      ...messages.map(wireOf),
    ],
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
  toolCalls?: ToolCallDelta[];
  usage?: Usage;
  /** The model that answered, as the provider names it. */
  model?: string;
  error?: string;
  done?: true;
  /** A guardrail ended the answer (`finish_reason` is `content_filter`). */
  blocked?: true;
}

/** One piece of a tool call in a stream; the id and the name come in its first piece only. */
export interface ToolCallDelta {
  index: number;
  id?: string;
  name?: string;
  arguments?: string;
}

/** Puts the pieces of the tool calls of a stream together, by their index. */
export class ToolCallAssembler {
  private readonly parts = new Map<number, { id?: string; name: string; arguments: string }>();

  add(deltas: readonly ToolCallDelta[]): void {
    for (const delta of deltas) {
      const part = this.parts.get(delta.index) ?? { name: "", arguments: "" };
      if (delta.id !== undefined) part.id = delta.id;
      part.name += delta.name ?? "";
      part.arguments += delta.arguments ?? "";
      this.parts.set(delta.index, part);
    }
  }

  /** The calls so far, in the order of their index. A piece that never named its call is left out. */
  calls(): ToolCall[] {
    return [...this.parts.entries()]
      .sort(([a], [b]) => a - b)
      .flatMap(([, part]) =>
        part.id === undefined
          ? []
          : [{ id: part.id, type: "function" as const, function: { name: part.name, arguments: part.arguments } }],
      );
  }
}

export const BROKE_OFF = "The answer broke off.";
/** A guardrail ended the answer. The stream does not name it: the log of the call does. */
export const STOPPED_BY_GUARDRAIL = "A guardrail stopped this answer.";

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
  if (isRecord(first) && isRecord(first.delta) && Array.isArray(first.delta.tool_calls)) {
    const deltas: ToolCallDelta[] = [];
    for (const raw of first.delta.tool_calls as unknown[]) {
      if (!isRecord(raw) || typeof raw.index !== "number") continue;
      const delta: ToolCallDelta = { index: raw.index };
      if (typeof raw.id === "string") delta.id = raw.id;
      const fn = isRecord(raw.function) ? raw.function : {};
      if (typeof fn.name === "string") delta.name = fn.name;
      if (typeof fn.arguments === "string") delta.arguments = fn.arguments;
      deltas.push(delta);
    }
    if (deltas.length > 0) chunk.toolCalls = deltas;
  }
  if (isRecord(first) && first.finish_reason === "content_filter") chunk.blocked = true;
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

/** The most the JSON of one request may be: the gateway takes 10 MiB, the rest is margin. */
export const BODY_BUDGET = 9 * 1024 * 1024;

/** The size of the request as it goes over the wire, in bytes. */
export function bodyBytes(body: object): number {
  return new TextEncoder().encode(JSON.stringify(body)).length;
}

export const IMAGE_OMITTED = "# image data omitted";

/** The call as a `curl` command for `/v1`, with a placeholder where the key goes. Image data is cut short. */
export function curlOf(origin: string, body: object): string {
  let shortened = 0;
  const json = JSON.stringify(body, (_key, value: unknown) => {
    if (typeof value === "string" && value.startsWith("data:image/")) {
      const cut = /^data:image\/[a-z+.-]+;base64,/.exec(value);
      if (cut !== null) {
        shortened += 1;
        return `${cut[0]}…`;
      }
    }
    return value;
  });
  const command = [
    `curl ${origin}/v1/chat/completions`,
    "  -H 'Authorization: Bearer <your key>'",
    "  -H 'Content-Type: application/json'",
    `  -d ${shellQuote(json)}`,
  ].join(" \\\n");
  return shortened > 0 ? `${command}\n${IMAGE_OMITTED}` : command;
}

/** How long to wait, as the sentence that follows a refusal; nothing when it is not known. */
export function retryText(seconds: number | null): string {
  if (seconds === null) return "";
  if (seconds < 60) return ` Try again in ${String(seconds)} ${seconds === 1 ? "second" : "seconds"}.`;
  const minutes = Math.ceil(seconds / 60);
  return ` Try again in ${String(minutes)} ${minutes === 1 ? "minute" : "minutes"}.`;
}

// Images mode: one prompt, a size and a number of images.

/** The most images one playground call asks for. */
export const MAX_IMAGES = 4;
export const IMAGE_COUNT_INVALID = `The number of images must be a whole number from 1 to ${String(MAX_IMAGES)}.`;

/** The sizes offered; `default` sends none, and the provider decides. */
export const IMAGE_SIZES = ["default", "1024x1024", "1536x1024", "1024x1536"] as const;
export type ImageSize = (typeof IMAGE_SIZES)[number];

export interface ImageRequestBody {
  model: string;
  prompt: string;
  n?: number;
  size?: string;
}

export function checkImageCount(text: string): { n?: number; error?: string } {
  const typed = text.trim();
  if (!/^\d+$/.test(typed)) return { error: IMAGE_COUNT_INVALID };
  const n = Number(typed);
  return n >= 1 && n <= MAX_IMAGES ? { n } : { error: IMAGE_COUNT_INVALID };
}

export function imageRequestBody(model: string, prompt: string, n: number, size: ImageSize): ImageRequestBody {
  return { model, prompt, n, ...(size === "default" ? {} : { size }) };
}

const IMAGE_FORMATS = ["png", "jpeg", "webp"];

/**
 * The images of an answer as `data:` URLs. Only `b64_json` images are shown:
 * a `url` would make the browser load a page of another origin.
 */
export function imageUrlsOf(answer: { data: readonly object[]; output_format?: string }): string[] {
  const format = IMAGE_FORMATS.includes(answer.output_format ?? "") ? (answer.output_format ?? "png") : "png";
  const urls: string[] = [];
  for (const item of answer.data) {
    const b64 = (item as { b64_json?: unknown }).b64_json;
    if (typeof b64 === "string" && /^[A-Za-z0-9+/=\s]+$/.test(b64)) {
      urls.push(`data:image/${format};base64,${b64.replace(/\s+/g, "")}`);
    }
  }
  return urls;
}

/** The call as a `curl` command for `/v1/images/generations`, with a placeholder where the key goes. */
export function imageCurlOf(origin: string, body: object): string {
  return [
    `curl ${origin}/v1/images/generations`,
    "  -H 'Authorization: Bearer <your key>'",
    "  -H 'Content-Type: application/json'",
    `  -d ${shellQuote(JSON.stringify(body))}`,
  ].join(" \\\n");
}

// Audio mode: a file to transcribe, or a text to speak.

/** The longest text one speech call takes, in characters. */
export const MAX_SPEECH_CHARS = 4096;
export const SPEECH_TOO_LONG = `The text must be at most ${String(MAX_SPEECH_CHARS)} characters.`;

/** The built-in voices of the speech models. */
export const VOICES = ["alloy", "ash", "ballad", "coral", "echo", "fable", "nova", "onyx", "sage", "shimmer"] as const;
export type Voice = (typeof VOICES)[number];

export interface SpeechRequestBody {
  model: string;
  input: string;
  voice: string;
}

export function checkSpeechInput(text: string): string | undefined {
  return Array.from(text).length > MAX_SPEECH_CHARS ? SPEECH_TOO_LONG : undefined;
}

export function speechRequestBody(model: string, input: string, voice: Voice): SpeechRequestBody {
  return { model, input, voice };
}

/** The transcript of a JSON answer, or `null` when it holds none. */
export function transcriptOf(answer: { text?: unknown }): string | null {
  return typeof answer.text === "string" ? answer.text : null;
}

/** The form a transcription is sent as: the file last, as the OpenAI clients send it. */
export function transcriptionForm(model: string, language: string, file: File): FormData {
  const form = new FormData();
  form.append("model", model);
  const code = language.trim();
  if (code !== "") form.append("language", code);
  form.append("file", file, file.name);
  return form;
}

/** The transcription as a `curl` command for `/v1/audio/transcriptions`, with placeholders for the key and the file. */
export function transcriptionCurlOf(origin: string, model: string, language: string): string {
  const code = language.trim();
  return [
    `curl ${origin}/v1/audio/transcriptions`,
    "  -H 'Authorization: Bearer <your key>'",
    "  -F file=@audio.mp3",
    `  -F model=${shellQuote(model)}`,
    ...(code === "" ? [] : [`  -F language=${shellQuote(code)}`]),
  ].join(" \\\n");
}

/** The speech as a `curl` command for `/v1/audio/speech`, with a placeholder where the key goes. */
export function speechCurlOf(origin: string, body: object): string {
  return [
    `curl ${origin}/v1/audio/speech`,
    "  -H 'Authorization: Bearer <your key>'",
    "  -H 'Content-Type: application/json'",
    `  -d ${shellQuote(JSON.stringify(body))}`,
    "  --output speech.mp3",
  ].join(" \\\n");
}
