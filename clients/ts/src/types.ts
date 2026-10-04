export interface Message {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface ChatRequest {
  model: string;
  messages: Message[];
  maxTokens?: number;
  temperature?: number;
  topP?: number;
  stop?: string | string[];
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
  finishReason: FinishReason | null;
  usage: Usage | null;
}

export type StreamEvent =
  | { type: "delta"; text: string }
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
