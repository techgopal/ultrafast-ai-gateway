import { useId, type ReactNode } from "react";

interface PartProps {
  title: string;
  description?: string;
  /** What can be done with the part as a whole, beside its title. */
  action?: ReactNode;
  children: ReactNode;
}

/** One of the parts of the page, under its own heading. */
export function Part({ title, description, action, children }: PartProps) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="flex flex-col gap-4 border-t pt-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 id={id} className="text-lg font-semibold">
            {title}
          </h2>
          {description === undefined ? null : (
            <p className="mt-1 text-sm text-muted-foreground">{description}</p>
          )}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}
