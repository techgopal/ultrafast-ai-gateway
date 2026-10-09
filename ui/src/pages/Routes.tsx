import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useDeleteRoute, useRoutes } from "@/api/queries";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control, longText } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { useToast } from "@/components/toast";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { targetText, teamsText } from "@/lib/routes";
import { RoutesHealth } from "@/pages/RoutesHealth";

type Route = components["schemas"]["RouteView"];

export const DELETE_CONSEQUENCE = "Calls that use this route fail at once.";
export const EMPTY_FOR_MEMBER = "No routes are available to you yet. Ask an admin.";

export const DONE = {
  delete: "Route deleted.",
  copied: "Copied.",
  notCopied: "Could not copy. Select the name and copy it by hand.",
} as const;

const columns: Column<Route>[] = [
  {
    id: "name",
    header: "Name",
    cell: (route) => (
      <span className={`${longText} font-mono font-medium break-all md:max-w-64`}>{route.name}</span>
    ),
    sortValue: (route) => route.name,
  },
  {
    id: "primaries",
    header: "Primary targets",
    cell: (route) => (
      <ul className="flex flex-col gap-1">
        {route.primaries.map((target) => (
          <li key={target.model} className="font-mono break-all">
            {targetText(target)}
          </li>
        ))}
      </ul>
    ),
  },
  {
    id: "fallbacks",
    header: "Fallbacks",
    cell: (route) => route.fallbacks.length,
    sortValue: (route) => route.fallbacks.length,
  },
  {
    id: "teams",
    header: "Teams",
    cell: (route) => teamsText(route),
  },
  {
    id: "guardrails",
    header: "Guardrails",
    // In the order they run. The gateway tells an admin only; this list is theirs.
    cell: (route) =>
      route.guardrails.length === 0 ? (
        <span className="text-muted-foreground">None</span>
      ) : (
        <span role="group" aria-label="Guardrails" className="flex flex-wrap gap-1">
          {route.guardrails.map((one) => (
            <Badge key={one.id} variant="outline" className="h-auto break-all whitespace-normal">
              {one.name}
            </Badge>
          ))}
        </span>
      ),
  },
  {
    id: "status",
    header: "Status",
    cell: (route) =>
      route.broken ? (
        <Badge variant="destructive">Broken</Badge>
      ) : (
        <Badge variant="default">Ready</Badge>
      ),
    sortValue: (route) => (route.broken ? 1 : 0),
  },
];

function AdminRoutes({ me }: { me: Me }) {
  const routes = useRoutes();
  const remove = useDeleteRoute();
  const toast = useToast();
  const [target, setTarget] = useState<Route | null>(null);
  const [asking, setAsking] = useState(false);
  const failed = routes.error !== null && routes.data === undefined;

  if (failed) {
    return (
      <QueryProblem
        title="Routing"
        error={routes.error}
        onRetry={() => {
          void routes.refetch();
        }}
      />
    );
  }
  return (
    <>
      <PageHeader
        title="Routing"
        subtitle="A route is a name that clients call. It spreads the calls over models, with fallbacks."
        actions={
          <Button asChild className={control}>
            <Link to="/routes/new">Add route</Link>
          </Button>
        }
      />
      <DataTable
        caption="Routes"
        columns={columns}
        rows={routes.data?.routes ?? []}
        loading={routes.isPending}
        getRowId={(route) => String(route.id)}
        empty={
          <EmptyState
            title="No routes"
            description="Add a route to call models by a name of your own."
          />
        }
        actions={(route) => (
          <>
            <Button asChild variant="outline" className={control}>
              <Link to="/routes/$id" params={{ id: String(route.id) }}>
                Edit
              </Link>
            </Button>
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                setTarget(route);
                setAsking(true);
              }}
            >
              Delete
            </Button>
          </>
        )}
      />
      {can(me, { type: "viewRoutingHealth" }) ? <RoutesHealth title="Target health" /> : null}
      <ConfirmDialog
        open={asking}
        onOpenChange={(open) => {
          if (open) return;
          setAsking(false);
          remove.reset();
        }}
        title={`Delete ${target?.name ?? "this route"}?`}
        body={DELETE_CONSEQUENCE}
        confirmLabel="Delete"
        tone="danger"
        onConfirm={async () => {
          if (target === null) return;
          await remove.mutateAsync({ id: target.id });
          toast(DONE.delete);
        }}
      />
    </>
  );
}

function MemberRoutes() {
  const routes = useRoutes();
  const toast = useToast();
  const failed = routes.error !== null && routes.data === undefined;

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast(DONE.copied);
    } catch {
      toast(DONE.notCopied, "error");
    }
  }

  if (failed) {
    return (
      <QueryProblem
        title="Routing"
        error={routes.error}
        onRetry={() => {
          void routes.refetch();
        }}
      />
    );
  }
  const list = [...(routes.data?.routes ?? [])].sort((a, b) => a.name.localeCompare(b.name));
  return (
    <>
      <PageHeader title="Routing" />
      {routes.isPending ? null : list.length === 0 ? (
        <EmptyState title="No routes" description={EMPTY_FOR_MEMBER} />
      ) : (
        <section className="flex flex-col gap-3">
          <h2 id="usable-routes" className="text-base font-medium">
            Routes you can use
          </h2>
          <ul aria-labelledby="usable-routes" className="flex flex-col gap-2">
            {list.map((route) => (
              <li
                key={route.id}
                className="flex items-start justify-between gap-3 rounded-lg border bg-card p-3"
              >
                <div className="flex min-w-0 flex-col gap-1">
                  <span className="font-mono font-medium break-all">{route.name}</span>
                  <ul aria-label={`Models of ${route.name}`} className="flex flex-col text-sm">
                    {route.primaries.map((target) => (
                      <li key={target.model} className="font-mono break-all">
                        {target.model}
                        {target.enabled ? "" : " (disabled)"}
                      </li>
                    ))}
                    {route.fallbacks.map((target) => (
                      <li
                        key={target.model}
                        className="font-mono break-all text-muted-foreground"
                      >
                        {`${target.model} (fallback${target.enabled ? "" : ", disabled"})`}
                      </li>
                    ))}
                  </ul>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  className={control}
                  aria-label={`Copy ${route.name}`}
                  onClick={() => {
                    void copy(route.name);
                  }}
                >
                  Copy
                </Button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </>
  );
}

function RoutesFor({ me }: { me: Me }) {
  return can(me, { type: "manageRoutes" }) ? <AdminRoutes me={me} /> : <MemberRoutes />;
}

/** The routes: managed by an admin, a read-only list of the names for everybody else. */
export function Routes() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  return <RoutesFor me={session.me} />;
}
