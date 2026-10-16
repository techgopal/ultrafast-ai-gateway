// What the playground's MCP tab knows: the arguments of a call as a text, the
// hints the input schema gives, and the blocks of a result.
import type { components } from "@/api/schema";

/** What of a tool is read here: its input schema, as the gateway sent it. */
interface Schemed {
  input_schema: unknown;
}

type Call = components["schemas"]["PlaygroundMcpRequest"];

/**
 * The request of a `tools/call`. The API description types free parameters
 * as `Record<string, never>`, which no object satisfies; the gateway checks them.
 */
export function callOf(tool: string, args: Record<string, unknown>): Call {
  return {
    jsonrpc: "2.0",
    id: 1,
    method: "tools/call",
    params: { name: tool, arguments: args } as unknown as NonNullable<Call["params"]>,
  };
}

export const ARGUMENTS_NOT_AN_OBJECT = "The arguments must be a JSON object.";
export const ARGUMENTS_NOT_JSON = "The arguments are not valid JSON.";

/** The arguments as typed: empty is no arguments. */
export function argumentsOf(text: string): { value: Record<string, unknown> } | { error: string } {
  const typed = text.trim();
  if (typed === "") return { value: {} };
  let parsed: unknown;
  try {
    parsed = JSON.parse(typed);
  } catch {
    return { error: ARGUMENTS_NOT_JSON };
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return { error: ARGUMENTS_NOT_AN_OBJECT };
  }
  return { value: parsed as Record<string, unknown> };
}

/**
 * The arguments a tool takes, one line each, from its input schema:
 * `q (string, required): what to look for`. Anything the schema does not
 * say is left out.
 */
export function hintsOf(tool: Schemed): string[] {
  const schema: unknown = tool.input_schema;
  if (typeof schema !== "object" || schema === null) return [];
  const properties: unknown = Reflect.get(schema, "properties");
  if (typeof properties !== "object" || properties === null) return [];
  const required: unknown = Reflect.get(schema, "required");
  const needed = Array.isArray(required) ? required.filter((one) => typeof one === "string") : [];
  return Object.entries(properties).map(([name, about]) => {
    const kind: unknown = typeof about === "object" && about !== null ? Reflect.get(about, "type") : undefined;
    const text: unknown =
      typeof about === "object" && about !== null ? Reflect.get(about, "description") : undefined;
    const parts = [
      Array.isArray(kind) ? kind.join(" or ") : typeof kind === "string" ? kind : "",
      needed.includes(name) ? "required" : "",
    ].filter((part) => part !== "");
    const head = parts.length === 0 ? name : `${name} (${parts.join(", ")})`;
    return typeof text === "string" && text !== "" ? `${head}: ${text}` : head;
  });
}

/** A starting point for the arguments: each required argument, empty. */
export function skeletonOf(tool: Schemed): string {
  const schema: unknown = tool.input_schema;
  const required: unknown =
    typeof schema === "object" && schema !== null ? Reflect.get(schema, "required") : undefined;
  const properties: unknown =
    typeof schema === "object" && schema !== null ? Reflect.get(schema, "properties") : undefined;
  const names = Array.isArray(required) ? required.filter((one) => typeof one === "string") : [];
  if (names.length === 0) return "{}";
  const empty = (name: string): unknown => {
    const about: unknown =
      typeof properties === "object" && properties !== null ? Reflect.get(properties, name) : undefined;
    const kind: unknown =
      typeof about === "object" && about !== null ? Reflect.get(about, "type") : undefined;
    switch (kind) {
      case "number":
      case "integer":
        return 0;
      case "boolean":
        return false;
      case "array":
        return [];
      case "object":
        return {};
      default:
        return "";
    }
  };
  return JSON.stringify(Object.fromEntries(names.map((name) => [name, empty(name)])), null, 2);
}

/** One block of the content of a result, as the page shows it. */
export type Block =
  | { kind: "text"; text: string }
  | { kind: "other"; label: string };

/** The content of a result: text as it is, anything else named by its kind. */
export function blocksOf(result: unknown): Block[] {
  const content: unknown =
    typeof result === "object" && result !== null ? Reflect.get(result, "content") : undefined;
  if (!Array.isArray(content)) return [];
  return content.map((block: unknown): Block => {
    const kind: unknown =
      typeof block === "object" && block !== null ? Reflect.get(block, "type") : undefined;
    const text: unknown =
      typeof block === "object" && block !== null ? Reflect.get(block, "text") : undefined;
    if (kind === "text" && typeof text === "string") return { kind: "text", text };
    if (kind === "image") return { kind: "other", label: "An image, not shown here." };
    if (kind === "resource") return { kind: "other", label: "An embedded resource, not shown here." };
    return { kind: "other", label: "A block of an unknown kind." };
  });
}

/** The error of a JSON-RPC reply, as its message. */
export function rpcErrorOf(reply: unknown): string | null {
  const error: unknown = typeof reply === "object" && reply !== null ? Reflect.get(reply, "error") : undefined;
  if (typeof error !== "object" || error === null) return null;
  const message: unknown = Reflect.get(error, "message");
  return typeof message === "string" ? message : "The call failed.";
}
