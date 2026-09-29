import type { ReactNode } from "react";

interface EmptyStateProps {
  title: string;
  description?: string;
  /** What the user can do about it, such as the button that creates the first one. */
  action?: ReactNode;
}

/** What a list shows when it has nothing in it. */
export function EmptyState({ title, description, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center gap-2 rounded-lg border border-dashed p-8 text-center">
      <h2 className="text-base font-medium text-foreground">{title}</h2>
      {description === undefined ? null : (
        <p className="text-sm text-muted-foreground">{description}</p>
      )}
      {action === undefined ? null : <div className="mt-2">{action}</div>}
    </div>
  );
}
