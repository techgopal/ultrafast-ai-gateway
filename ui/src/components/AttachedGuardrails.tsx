import { useGuardrails } from "@/api/queries";
import { Badge } from "@/components/ui/badge";

/**
 * The guardrails of a team or a user by name, in the order they run. Reads the
 * list of guardrails, which only an admin may: it is for an admin's page.
 */
export function AttachedGuardrails({ ids }: { ids: readonly number[] }) {
  const list = useGuardrails();
  if (ids.length === 0) return <span className="text-muted-foreground">None</span>;
  const names = ids.flatMap((id) => {
    const found = list.data?.guardrails.find((one) => one.id === id);
    return found === undefined ? [] : [found];
  });
  return (
    <span role="group" aria-label="Guardrails" className="flex flex-wrap gap-1">
      {names.map((one) => (
        <Badge key={one.id} variant="outline" className="h-auto break-all whitespace-normal">
          {one.name}
        </Badge>
      ))}
    </span>
  );
}
