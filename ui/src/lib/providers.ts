// What the console knows of the kinds of a provider, and of the API key that
// is sent for one.
import { ConsoleRefusal } from "@/api/errors";

const KIND_NAMES: Record<string, string> = {
  openai: "OpenAI-compatible",
  anthropic: "Anthropic",
  gemini: "Gemini",
  azure: "Azure OpenAI",
};
export const KINDS = ["openai", "anthropic", "gemini", "azure"] as const;

/** The API version the gateway uses for an Azure OpenAI provider that is given none. */
export const DEFAULT_API_VERSION = "2024-10-21";

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

export const HOST_CHANGED = "Enter the API key again: the host changed.";

/**
 * Whether two base URLs name the same host: scheme, host and port, as the
 * gateway reads them. The gateway sends a stored key only to the host it
 * was given for; a URL that cannot be read is another host.
 */
export function sameHost(a: string, b: string): boolean {
  try {
    const [first, second] = [new URL(a), new URL(b)];
    return first.protocol === second.protocol && first.host === second.host;
  } catch {
    return false;
  }
}
