import type { components } from "@/api/schema";

type AuditEntry = components["schemas"]["AuditRow"];

/**
 * The entries that have the text in their actor, their action or their
 * summary, without regard to case, in the order they were given. No text
 * leaves nothing out.
 */
export function entriesMatching(entries: readonly AuditEntry[], text: string): AuditEntry[] {
  const wanted = text.trim().toLowerCase();
  if (wanted === "") return [...entries];
  return entries.filter((entry) =>
    [entry.actor_email, entry.action, entry.summary].some((value) =>
      value.toLowerCase().includes(wanted),
    ),
  );
}
