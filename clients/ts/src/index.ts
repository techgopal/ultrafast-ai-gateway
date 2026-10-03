export { initWasm, type WasmSource } from "./wasm.js";
export { Client, DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_MS, type ClientOptions } from "./client.js";
export {
  AuthenticationError,
  InvalidRequestError,
  MalformedError,
  NetworkError,
  NotFoundError,
  PermissionDeniedError,
  RateLimitError,
  RequestTimeoutError,
  UltrafastError,
  UpstreamError,
  type ErrorKind,
} from "./errors.js";
export { Target, anthropic, azure, gateway, gemini, openai, openaiCompatible } from "./target.js";
export type {
  ChatRequest,
  ChatResponse,
  EmbeddingsRequest,
  EmbeddingsResponse,
  FinishReason,
  Message,
  StreamEvent,
  Usage,
} from "./types.js";
