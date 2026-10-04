// The rules of a tag, as the gateway has them (`crates/gateway/src/tags.rs`):
// at most 20 to a call or a key, a name of `A-Z a-z 0-9 _ . : -`, names and
// values of 1 to 64 characters. The gateway decides; the console refuses
// what it knows the gateway would, so that nothing is sent that cannot be.
import { ConsoleRefusal } from "@/api/errors";

export const MAX_TAGS = 20;
export const MAX_PART = 64;

/** A tag as the editor holds it: text, until it is sent. */
export interface TagRow {
  name: string;
  value: string;
}

export const TAG_PROBLEMS = {
  count: "At most 20 tags.",
  empty: "Every tag needs a name and a value.",
  long: "A name or value is at most 64 characters.",
  charset: "A tag name may use only letters, digits and _ . : -",
  twice: "A tag name can be used once.",
} as const;

/** What the filter of the logs says when its text is not `name:value`. */
export const BAD_TAG_FILTER = "Write the tag as name:value.";

const NAME = /^[A-Za-z0-9_.:-]+$/;

function length(text: string): number {
  return Array.from(text).length;
}

/** A row with nothing in it is not a tag: it was added and left. */
function filled(rows: readonly TagRow[]): TagRow[] {
  return rows.filter((one) => one.name.trim() !== "" || one.value.trim() !== "");
}

/** Why the tags cannot be sent, or `null`. */
export function tagProblem(rows: readonly TagRow[]): string | null {
  const tags = filled(rows);
  if (tags.length > MAX_TAGS) return TAG_PROBLEMS.count;
  const seen = new Set<string>();
  for (const { name, value } of tags) {
    if (name === "" || value === "") return TAG_PROBLEMS.empty;
    if (length(name) > MAX_PART || length(value) > MAX_PART) return TAG_PROBLEMS.long;
    if (!NAME.test(name)) return TAG_PROBLEMS.charset;
    if (seen.has(name)) return TAG_PROBLEMS.twice;
    seen.add(name);
  }
  return null;
}

/** The tags to send, or a refusal on the field `tags`. */
export function tagsOf(rows: readonly TagRow[]): Record<string, string> {
  const problem = tagProblem(rows);
  if (problem !== null) throw new ConsoleRefusal(problem, "tags");
  const tags: Record<string, string> = {};
  for (const { name, value } of filled(rows).sort((a, b) => (a.name < b.name ? -1 : 1))) {
    tags[name] = value;
  }
  return tags;
}

/** The rows of the editor for tags the gateway has, by name. */
export function rowsOf(tags: Readonly<Record<string, string>>): TagRow[] {
  return Object.entries(tags)
    .sort(([a], [b]) => (a < b ? -1 : 1))
    .map(([name, value]) => ({ name, value }));
}

/** What a list shows of the tags: `name:value`, by name. */
export function chipsOf(tags: Readonly<Record<string, string>>): string[] {
  return rowsOf(tags).map(({ name, value }) => `${name}:${value}`);
}

/**
 * The tag of the filter of the logs: `name:value`, the name ending at the
 * first colon, as in the gateway. The tag is as the gateway takes it.
 */
export function parseTagFilter(text: string): { tag: string } | { problem: string } {
  const trimmed = text.trim();
  const at = trimmed.indexOf(":");
  const bad = { problem: BAD_TAG_FILTER };
  if (at === -1) return bad;
  const name = trimmed.slice(0, at);
  const value = trimmed.slice(at + 1);
  if (name === "" || value === "") return bad;
  if (length(name) > MAX_PART || length(value) > MAX_PART || !NAME.test(name)) return bad;
  return { tag: trimmed };
}
