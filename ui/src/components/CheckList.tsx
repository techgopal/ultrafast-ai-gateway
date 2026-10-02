import type { ReactNode } from "react";
import { ErrorState } from "@/components/ErrorState";
import type { FieldWiring } from "@/components/Field";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";

/** A long list scrolls inside its place. */
const scrolling = "max-h-48 overflow-y-auto";

export interface ChecksProps<T extends { id: number }> {
  wiring: FieldWiring;
  /** `null` while the list is on its way. */
  items: readonly T[] | null;
  error: unknown;
  retry: () => void;
  loading: string;
  none: string;
  checked: readonly string[];
  onChange: (ids: string[]) => void;
  label: (item: T) => ReactNode;
}

/** A list of checkboxes, one for each thing that can be granted. */
export function Checks<T extends { id: number }>({
  wiring,
  items,
  error,
  retry,
  loading,
  none,
  checked,
  onChange,
  label,
}: ChecksProps<T>) {
  if (items === null) {
    if (error !== null) return <ErrorState error={error} onRetry={retry} />;
    return (
      <div role="status" aria-busy="true" aria-label={loading} className="flex flex-col gap-2">
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-full" />
      </div>
    );
  }
  if (items.length === 0) return <p className="text-sm text-muted-foreground">{none}</p>;
  const { id } = wiring;
  return (
    <div
      role="group"
      id={id}
      aria-labelledby={wiring["aria-labelledby"]}
      aria-describedby={wiring["aria-describedby"]}
      className={`flex flex-col ${scrolling}`}
    >
      {items.map((item) => {
        const value = String(item.id);
        const inputId = `${id}-${value}`;
        return (
          <Label key={item.id} htmlFor={inputId} className="min-h-11 items-start py-2">
            <Checkbox
              id={inputId}
              checked={checked.includes(value)}
              onCheckedChange={(on) => {
                onChange(on === true ? [...checked, value] : checked.filter((one) => one !== value));
              }}
            />
            <span className="flex min-w-0 flex-wrap gap-x-2">{label(item)}</span>
          </Label>
        );
      })}
    </div>
  );
}
