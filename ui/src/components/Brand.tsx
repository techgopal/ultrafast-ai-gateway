import { cn } from "@/lib/utils";

/** The mark of docs/brand/mark.svg: a gate (two pillars) with a bolt through it. */
export function BrandMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 64 64" aria-hidden="true" focusable="false" className={cn("size-8 shrink-0", className)}>
      <rect width="64" height="64" rx="14" className="fill-brand-ink" />
      <rect x="12" y="14" width="8" height="36" rx="2" className="fill-brand-paper" />
      <rect x="44" y="14" width="8" height="36" rx="2" className="fill-brand-paper" />
      <polygon points="36,10 24,35 31,35 27,54 41,27 34,27 38,10" className="fill-brand-signal" />
    </svg>
  );
}

/** The mark and the name, as the console's header. */
export function Brand({ className }: { className?: string }) {
  return (
    <span className={cn("flex items-center gap-2", className)}>
      <BrandMark />
      <span className="text-base font-semibold tracking-tight">Ultrafast</span>
    </span>
  );
}
