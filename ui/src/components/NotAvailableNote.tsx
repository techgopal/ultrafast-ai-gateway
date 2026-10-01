export const PART_NOT_AVAILABLE = "Not available to your account.";

/**
 * For a part of a page that the account may not see, where the rest of the
 * page stays: a tile, a section. It is not an error, and it has no heading:
 * the part keeps its own. For a whole page there is `NotAvailableContent`.
 */
export function NotAvailableNote() {
  return <p className="text-sm text-muted-foreground">{PART_NOT_AVAILABLE}</p>;
}
