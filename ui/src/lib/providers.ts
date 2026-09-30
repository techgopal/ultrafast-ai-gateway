// What the console knows of the kinds of a provider.

const KIND_NAMES: Record<string, string> = { openai: "OpenAI-compatible", anthropic: "Anthropic" };
export const KINDS = ["openai", "anthropic"] as const;

export function kindName(kind: string): string {
  return KIND_NAMES[kind] ?? kind;
}
