import { Badge } from "@/components/ui/badge";

type Variant = "default" | "secondary" | "destructive" | "outline";

const variants: Record<string, Variant> = {
  active: "default",
  ok: "default",
  invited: "secondary",
  suspended: "secondary",
  warning: "secondary",
  error: "destructive",
  disabled: "outline",
  expired: "outline",
  revoked: "outline",
};

/** A status as a pill. The text always states the status; the look only supports it. */
export function StatusBadge({ status }: { status: string }) {
  return <Badge variant={variants[status] ?? "outline"}>{status}</Badge>;
}
