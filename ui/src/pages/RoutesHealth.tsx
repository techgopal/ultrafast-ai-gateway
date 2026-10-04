import { useMemo } from "react";
import { useRoutingHealth } from "@/api/queries";
import type { components } from "@/api/schema";
import { longText } from "@/components/classes";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { QueryProblem } from "@/components/QueryProblem";
import { Timestamp } from "@/components/Timestamp";
import { Badge } from "@/components/ui/badge";
import { healthFor, healthText } from "@/lib/routes";

type Route = components["schemas"]["RouteView"];
type Target = components["schemas"]["TargetHealth"];
type State = components["schemas"]["TargetState"];

export const HEALTH_NOTE = "From real traffic only. The gateway does not send test requests.";

const variants: Record<State, "default" | "secondary" | "destructive"> = {
  closed: "default",
  open: "destructive",
  half_open: "secondary",
};

const columns: Column<Target>[] = [
  {
    id: "target",
    header: "Target",
    cell: (target) => (
      <span className={`${longText} font-mono break-all md:max-w-64`}>
        {`${target.provider}/${target.model}`}
      </span>
    ),
    sortValue: (target) => `${target.provider}/${target.model}`,
  },
  {
    id: "state",
    header: "State",
    cell: (target) => <Badge variant={variants[target.state]}>{healthText(target.state)}</Badge>,
    sortValue: (target) => healthText(target.state),
  },
  {
    id: "successes",
    header: "Successes",
    cell: (target) => target.successes,
    sortValue: (target) => target.successes,
  },
  {
    id: "failures",
    header: "Failures",
    cell: (target) => target.failures,
    sortValue: (target) => target.failures,
  },
  {
    id: "last_failure",
    header: "Last failure",
    cell: (target) => (
      <span className="inline-flex flex-wrap gap-x-2">
        <Timestamp value={target.last_failure_at} />
        {target.last_status === null ? null : <span>{`Status ${String(target.last_status)}`}</span>}
      </span>
    ),
    sortValue: (target) => target.last_failure_at,
  },
];

interface HealthProps {
  /** The heading, and the name of the table. */
  title: string;
  /** Only the targets of this route; without it, every target the gateway has called. */
  route?: Route;
}

/**
 * The health of the targets, for an admin. It is what the gateway saw in
 * real traffic since it started: a target nobody called is not here.
 */
export function RoutesHealth({ title, route }: HealthProps) {
  const health = useRoutingHealth();
  const rows = useMemo(() => {
    const all = health.data?.targets ?? [];
    return route === undefined ? all : healthFor(route, all);
  }, [health.data, route]);
  const headingId = `health-${route === undefined ? "all" : "route"}`;

  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3">
      <h2 id={headingId} className="text-lg font-medium">
        {title}
      </h2>
      <p className="text-sm text-muted-foreground">{HEALTH_NOTE}</p>
      {health.error !== null && health.data === undefined ? (
        <QueryProblem
          part
          error={health.error}
          onRetry={() => {
            void health.refetch();
          }}
        />
      ) : (
        <DataTable
          caption={title}
          columns={columns}
          rows={rows}
          loading={health.isPending}
          getRowId={(target) => `${target.provider}/${target.model}`}
          empty={
            <EmptyState
              title="No calls yet"
              description="No target has been called since the gateway started."
            />
          }
        />
      )}
    </section>
  );
}
