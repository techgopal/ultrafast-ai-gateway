// What the console knows of a prompt template: the variables of a text (as
// the gateway reads them), the draft of a version and what is checked before
// it is sent (the gateway still decides), the request made of it, and the
// difference between two versions. Pure functions: `pages/Prompts*` ask them.
import type { components } from "@/api/schema";

type Schemas = components["schemas"];
type VersionView = Schemas["VersionView"];

export type Role = "system" | "developer" | "user" | "assistant";
export const ROLES: readonly Role[] = ["system", "developer", "user", "assistant"];

export const MAX_NAME_CHARS = 100;
export const MAX_DESCRIPTION_CHARS = 500;
export const MAX_MESSAGES = 64;
export const MAX_CONTENT_BYTES = 64 * 1024;
export const MAX_TOTAL_BYTES = 256 * 1024;
export const MAX_VARIABLES = 64;
export const MAX_MODEL_CHARS = 200;

/** `{{name}}`: a name of letters, digits and `_` that does not start with a digit, up to 64 characters. */
const VARIABLE = /\{\{([A-Za-z_][A-Za-z0-9_]{0,63})\}\}/g;

/** The names the texts use, once each, sorted: the variables of a version. */
export function variablesIn(texts: readonly string[]): string[] {
  const names = new Set<string>();
  for (const text of texts) {
    for (const found of text.matchAll(VARIABLE)) {
      const name = found[1];
      if (name !== undefined) names.add(name);
    }
  }
  return [...names].sort();
}

export interface DiffLine {
  kind: "same" | "add" | "remove";
  text: string;
}

/** The most cells the comparison fills; a larger difference is shown as all removed, then all added. */
const DIFF_CELLS = 4_000_000;

/** A line diff: the common start and end are cut off, the rest is compared by longest common subsequence. */
export function diffLines(before: readonly string[], after: readonly string[]): DiffLine[] {
  let start = 0;
  while (start < before.length && start < after.length && before[start] === after[start]) start += 1;
  let endBefore = before.length;
  let endAfter = after.length;
  while (endBefore > start && endAfter > start && before[endBefore - 1] === after[endAfter - 1]) {
    endBefore -= 1;
    endAfter -= 1;
  }
  const a = before.slice(start, endBefore);
  const b = after.slice(start, endAfter);
  const out: DiffLine[] = before.slice(0, start).map((text) => ({ kind: "same", text }));
  out.push(...middle(a, b));
  out.push(...before.slice(endBefore).map((text): DiffLine => ({ kind: "same", text })));
  return out;
}

function middle(a: readonly string[], b: readonly string[]): DiffLine[] {
  if (a.length === 0 || b.length === 0 || (a.length + 1) * (b.length + 1) > DIFF_CELLS) {
    return [
      ...a.map((text): DiffLine => ({ kind: "remove", text })),
      ...b.map((text): DiffLine => ({ kind: "add", text })),
    ];
  }
  const width = b.length + 1;
  // longest[i * width + j]: the length of the common subsequence of a[i..] and b[j..].
  const longest = new Uint32Array((a.length + 1) * width);
  for (let i = a.length - 1; i >= 0; i -= 1) {
    for (let j = b.length - 1; j >= 0; j -= 1) {
      longest[i * width + j] =
        a[i] === b[j]
          ? (longest[(i + 1) * width + j + 1] ?? 0) + 1
          : Math.max(longest[(i + 1) * width + j] ?? 0, longest[i * width + j + 1] ?? 0);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      out.push({ kind: "same", text: a[i] ?? "" });
      i += 1;
      j += 1;
    } else if ((longest[(i + 1) * width + j] ?? 0) >= (longest[i * width + j + 1] ?? 0)) {
      out.push({ kind: "remove", text: a[i] ?? "" });
      i += 1;
    } else {
      out.push({ kind: "add", text: b[j] ?? "" });
      j += 1;
    }
  }
  for (; i < a.length; i += 1) out.push({ kind: "remove", text: a[i] ?? "" });
  for (; j < b.length; j += 1) out.push({ kind: "add", text: b[j] ?? "" });
  return out;
}

interface PlainMessage {
  role: string;
  content: string;
}

/** The difference between the messages of two versions: each message is its role, then its lines. */
export function diffMessages(before: readonly PlainMessage[], after: readonly PlainMessage[]): DiffLine[] {
  const lines = (messages: readonly PlainMessage[]) =>
    messages.flatMap((message) => [`${message.role}:`, ...message.content.split("\n")]);
  return diffLines(lines(before), lines(after));
}

export interface DraftMessage {
  /** Tells the rows apart while they are edited; not sent. */
  key: number;
  role: Role;
  content: string;
}

/** A version as it is typed. The settings are texts, as in the fields. */
export interface Draft {
  name: string;
  description: string;
  model: string;
  temperature: string;
  maxTokens: string;
  topP: string;
  /** The response format of the version this one starts from: kept as it was, not edited here. */
  responseFormat: unknown;
  messages: DraftMessage[];
}

let nextKey = 1;
export function newKey(): number {
  nextKey += 1;
  return nextKey;
}

export function emptyDraft(): Draft {
  return {
    name: "",
    description: "",
    model: "",
    temperature: "",
    maxTokens: "",
    topP: "",
    responseFormat: null,
    messages: [{ key: newKey(), role: "user", content: "" }],
  };
}

/** The draft of the next version of a template: the text and settings of `version`. */
export function draftOf(template: { name: string; description: string }, version: VersionView): Draft {
  const params = version.params;
  return {
    name: template.name,
    description: template.description,
    model: version.model ?? "",
    temperature: params.temperature == null ? "" : String(params.temperature),
    maxTokens: params.max_tokens == null ? "" : String(params.max_tokens),
    topP: params.top_p == null ? "" : String(params.top_p),
    responseFormat: params.response_format ?? null,
    messages: version.messages.map((message) => ({
      key: newKey(),
      role: ROLES.find((role) => role === message.role) ?? "user",
      content: message.content,
    })),
  };
}

export interface Problems {
  fields: Partial<
    Record<"name" | "description" | "model" | "messages" | "temperature" | "maxTokens" | "topP", string>
  >;
  /** The first problem of each message, by its `key`. */
  rows: Record<number, string>;
}

export function hasProblems(problems: Problems): boolean {
  return Object.keys(problems.fields).length > 0 || Object.keys(problems.rows).length > 0;
}

/** Characters as a person counts them (code points), as the gateway does. */
const charCount = (text: string) => Array.from(text).length;

const byteLength = (text: string) => new TextEncoder().encode(text).length;

/** A number written in plain digits with an optional fraction, or `null`. */
function plainNumber(text: string): number | null {
  return /^(\d+\.?\d*|\.\d+)$/.test(text) ? Number(text) : null;
}

/** What is wrong with the draft, as far as the console can tell. */
export function check(draft: Draft): Problems {
  const fields: Problems["fields"] = {};
  const rows: Record<number, string> = {};
  const name = draft.name.trim();
  if (name === "") fields.name = "Enter a name.";
  else if (charCount(name) > MAX_NAME_CHARS) fields.name = `The name is at most ${String(MAX_NAME_CHARS)} characters.`;
  else if (name.includes("@")) fields.name = "The name cannot contain @: the logs write name@version.";
  if (charCount(draft.description) > MAX_DESCRIPTION_CHARS) {
    fields.description = `The description is at most ${String(MAX_DESCRIPTION_CHARS)} characters.`;
  }
  const model = draft.model.trim();
  if (model !== "" && charCount(model) > MAX_MODEL_CHARS) {
    fields.model = `The model is at most ${String(MAX_MODEL_CHARS)} characters.`;
  }
  if (draft.messages.length === 0) fields.messages = "Add at least one message.";
  else if (draft.messages.length > MAX_MESSAGES) fields.messages = `At most ${String(MAX_MESSAGES)} messages.`;
  else {
    let total = 0;
    for (const message of draft.messages) {
      const bytes = byteLength(message.content);
      total += bytes;
      if (message.content.trim() === "") rows[message.key] = "Write the message.";
      else if (bytes > MAX_CONTENT_BYTES) rows[message.key] = "A message is at most 64 KiB.";
    }
    if (total > MAX_TOTAL_BYTES) fields.messages = "The messages together are at most 256 KiB.";
    else if (variablesIn(draft.messages.map((message) => message.content)).length > MAX_VARIABLES) {
      fields.messages = `At most ${String(MAX_VARIABLES)} variables.`;
    }
  }
  const temperature = draft.temperature.trim();
  if (temperature !== "") {
    const value = plainNumber(temperature);
    if (value === null || value > 2) fields.temperature = "From 0 to 2.";
  }
  const topP = draft.topP.trim();
  if (topP !== "") {
    const value = plainNumber(topP);
    if (value === null || value > 1) fields.topP = "From 0 to 1.";
  }
  const maxTokens = draft.maxTokens.trim();
  if (maxTokens !== "" && !(/^\d+$/.test(maxTokens) && Number(maxTokens) >= 1)) {
    fields.maxTokens = "A whole number from 1.";
  }
  return { fields, rows };
}

/** What a version holds: its messages, and its model and settings when they are given. */
export function versionRequestOf(draft: Draft): Pick<Schemas["CreatePromptRequest"], "messages" | "model" | "params"> {
  const params: NonNullable<Schemas["CreatePromptRequest"]["params"]> = {};
  const temperature = draft.temperature.trim();
  if (temperature !== "") params.temperature = Number(temperature);
  const topP = draft.topP.trim();
  if (topP !== "") params.top_p = Number(topP);
  const maxTokens = draft.maxTokens.trim();
  if (maxTokens !== "") params.max_tokens = Number(maxTokens);
  if (typeof draft.responseFormat === "object" && draft.responseFormat !== null) {
    params.response_format = draft.responseFormat as Record<string, never>;
  }
  const model = draft.model.trim();
  return {
    ...(model === "" ? {} : { model }),
    ...(Object.keys(params).length === 0 ? {} : { params }),
    messages: draft.messages.map(({ role, content }) => ({ role, content })),
  };
}

/** The request that creates a template: the name, the description and version 1. */
export function requestOf(draft: Draft): Schemas["CreatePromptRequest"] {
  const description = draft.description.trim();
  return {
    name: draft.name.trim(),
    ...(description === "" ? {} : { description }),
    ...versionRequestOf(draft),
  };
}
