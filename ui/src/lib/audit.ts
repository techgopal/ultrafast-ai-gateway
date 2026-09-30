import type { components } from "@/api/schema";

type AuditEntry = components["schemas"]["AuditRow"];

/** Whether the text is a filter: blanks alone leave nothing out, and are none. */
export function isFilter(text: string): boolean {
  return text.trim() !== "";
}

/**
 * The entries that have the text in their actor, their action or their
 * summary, without regard to case, in the order they were given. No text
 * leaves nothing out.
 */
export function entriesMatching(entries: readonly AuditEntry[], text: string): AuditEntry[] {
  if (!isFilter(text)) return [...entries];
  const wanted = text.trim().toLowerCase();
  return entries.filter((entry) =>
    [entry.actor_email, entry.action, entry.summary].some((value) =>
      value.toLowerCase().includes(wanted),
    ),
  );
}
