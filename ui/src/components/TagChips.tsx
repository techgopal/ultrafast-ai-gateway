import { Badge } from "@/components/ui/badge";
import { chipsOf } from "@/lib/tags";

export const NO_TAGS = "No tags";

/**
 * The tags of a key or a call as chips, `name:value` by name, or "No tags".
 * A group, not a list: a card of a narrow screen is a list item, and its own
 * items are what the screen counts.
 */
export function TagChips({ tags }: { tags: Readonly<Record<string, string>> }) {
  const chips = chipsOf(tags);
  if (chips.length === 0) return <span className="text-muted-foreground">{NO_TAGS}</span>;
  return (
    <span role="group" aria-label="Tags" className="flex flex-wrap gap-1">
      {chips.map((chip) => (
        <Badge key={chip} variant="outline" className="h-auto font-mono break-all whitespace-normal">
          {chip}
        </Badge>
      ))}
    </span>
  );
}
