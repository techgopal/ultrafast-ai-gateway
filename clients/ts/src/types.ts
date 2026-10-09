export type ContentPart = { type: "text"; text: string } | { type: "image"; url: string };

/** A tool call the model made: `arguments` is the JSON text it produced. */
export interface ToolCall {
  id: string;
  name: string;
  arguments: string;
}

/** A function the model may call; `parameters` is a JSON Schema (default `{"type":"object"}`). */
export interface Tool {
  name: string;
  description?: string;
  parameters?: Record<string, unknown>;
  /** OpenAI's schema-enforced arguments. Sent to OpenAI and Azure; Anthropic and Gemini ignore it. */
  strict?: boolean;
}

export type ToolChoice = "auto" | "none" | "required" | { name: string };

/**
 * `content` is text, or text and image parts (an `image` url is an http(s)
 * URL or a `data:image/...;base64,` URL; only `user` messages carry images),
 * or null for an assistant message that only calls tools. `toolCalls` is for
 * `assistant`; a `tool` message needs `toolCallId`.
 */
export interface Message {
  role: "system" | "user" | "assistant" | "tool";
  content: string | ContentPart[] | null;
  toolCalls?: ToolCall[];
  toolCallId?: string;
}

/** Structured output: plain text, any JSON object, or JSON that matches a JSON Schema. */
export type ResponseFormat =
  | { type: "text" }
  | { type: "json_object" }
  | { type: "json_schema"; jsonSchema: { name: string; schema: Record<string, unknown>; strict?: boolean; description?: string } };

export interface ChatRequest {
  model: string;
  messages: Message[];
  maxTokens?: number;
  temperature?: number;
  topP?: number;
  stop?: string | string[];
  tools?: Tool[];
  toolChoice?: ToolChoice;
  parallelToolCalls?: boolean;
  responseFormat?: ResponseFormat;
  /** Sent to a gateway only, as `x-uf-tags` (at most 1 KiB of JSON). */
  tags?: Record<string, string>;
}

export interface Usage {
  inputTokens: number;
  outputTokens: number;
}

export type FinishReason = "stop" | "length" | "content_filter" | "tool_calls" | (string & {});

export interface ChatResponse {
  id: string | null;
  model: string | null;
  content: string;
  toolCalls: ToolCall[];
  finishReason: FinishReason | null;
  usage: Usage | null;
}

export type StreamEvent =
  | { type: "delta"; text: string }
  | { type: "tool_call_start"; index: number; id: string; name: string }
  | { type: "tool_call_delta"; index: number; arguments: string }
  | { type: "done"; finishReason: FinishReason | null; usage: Usage | null };

export interface EmbeddingsRequest {
  model: string;
  input: string | string[];
  dimensions?: number;
  tags?: Record<string, string>;
}

export interface EmbeddingsResponse {
  model: string;
  vectors: number[][];
  promptTokens: number | null;
}
