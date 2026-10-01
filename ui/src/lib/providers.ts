// What the console knows of the kinds of a provider, and of the API key that
// is sent for one.
import { ConsoleRefusal } from "@/api/errors";

const KIND_NAMES: Record<string, string> = { openai: "OpenAI-compatible", anthropic: "Anthropic" };
export const KINDS = ["openai", "anthropic"] as const;

export function kindName(kind: string): string {
  return KIND_NAMES[kind] ?? kind;
}

export const ENTER_AN_API_KEY = "Enter an API key.";

/**
 * The API key to send, of what was typed into its field: without the spaces
 * around it, which nobody means (a copy can bring them). Nothing typed is
 * `""`. A key of spaces only is refused by the console, on the field: sent,
 * it would be refused by the gateway, and left out, the provider would be
 * added without the key the admin believed they typed.
 */
export function apiKeyOf(typed: string): string {
  const key = typed.trim();
  if (key === "" && typed !== "") throw new ConsoleRefusal(ENTER_AN_API_KEY, "api_key");
  return key;
}

/**
 * The new key of "Replace the key": as `apiKeyOf`, and it must be there. A
 * field left empty is refused as one of spaces is: the admin chose to
 * replace the key, and nothing is no key to replace it with.
 */
export function newApiKeyOf(typed: string): string {
  const key = apiKeyOf(typed);
  if (key === "") throw new ConsoleRefusal(ENTER_AN_API_KEY, "api_key");
  return key;
}
