import type { ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import { useLog } from "@/api/queries";
import type { components } from "@/api/schema";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { NotFoundContent } from "@/components/NotFoundContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { TagChips } from "@/components/TagChips";
import { Timestamp } from "@/components/Timestamp";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { idOf } from "@/lib/id";
import { sideLines } from "@/lib/guardrails";
import { formatDuration, outcomeLabel } from "@/lib/usage";
import { costOf, GuardrailBadge, modelOf, tokensOf } from "@/pages/Logs";

/** An attempt, with its place in the order: two attempts can be alike. */
type Attempt = components["schemas"]["LogAttempt"] & { n: number };

const NONE = "—";

function outcomeVariant(outcome: string): "default" | "secondary" | "destructive" | "outline" {
  if (outcome === "ok") return "default";
  if (outcome === "fatal") return "destructive";
  if (outcome === "retryable") return "secondary";
  return "outline";
}

const columns: Column<Attempt>[] = [
  {
    id: "target",
    header: "Provider and model",
    cell: (attempt) => <span className="break-all">{`${attempt.provider}/${attempt.model}`}</span>,
  },
  {
    id: "outcome",
    header: "Outcome",
    cell: (attempt) => (
      <Badge variant={outcomeVariant(attempt.outcome)}>{outcomeLabel(attempt.outcome)}</Badge>
    ),
  },
  {
    id: "status",
    header: "Status",
    cell: (attempt) => <span className="tabular-nums">{attempt.status ?? NONE}</span>,
  },
  {
    id: "duration",
    header: "Duration",
    cell: (attempt) => <span className="tabular-nums">{formatDuration(attempt.duration_ms)}</span>,
  },
];

function Loading() {
  return (
    <>
      <PageHeader title={"Call"} />
      <div role="status" aria-busy="true" aria-label="Loading the call" className="flex flex-col gap-4">
        <Skeleton className="h-4 w-full max-w-md" />
        <Skeleton className="h-4 w-full max-w-md" />
        <Skeleton className="h-4 w-full max-w-md" />
      </div>
    </>
  );
}

function Details({ id }: { id: number }) {
  const call = useLog(id);
  if (call.data === undefined) {
    if (call.error !== null) {
      return (
        <QueryProblem
          notFound
          title="Call"
          error={call.error}
          onRetry={() => {
            void call.refetch();
          }}
        />
      );
    }
    return <Loading />;
  }
  const log = call.data;
  const fields: [string, ReactNode][] = [
    ["Time", <Timestamp key="t" value={log.at} />],
    [
      "Status",
      <span key="s" className="inline-flex flex-wrap items-center gap-1.5">
        <Badge variant={log.status >= 400 ? "destructive" : "secondary"}>{log.status}</Badge>
        {log.cached ? <Badge variant="outline">Cached</Badge> : null}
        {log.estimated ? <Badge variant="outline">Estimated</Badge> : null}
        <GuardrailBadge log={log} />
      </span>,
    ],
    ["Asked for", log.requested],
    ["Answered by", log.provider !== null && log.model !== null ? modelOf(log) : NONE],
    ["Endpoint", log.endpoint],
    ["Streamed", log.stream ? "Yes" : "No"],
    ["Key", log.key_name ?? (log.key_id === null ? NONE : "(deleted)")],
    ["User", log.user_email ?? NONE],
    ["Team", log.team_name ?? NONE],
    ["Tokens in / out", tokensOf(log)],
    ["Cost", costOf(log)],
    ["Duration", formatDuration(log.duration_ms)],
    ["Tags", <TagChips key="g" tags={log.tags} />],
  ];
  const sides = [
    ["Input", log.guardrails?.input],
    ["Output", log.guardrails?.output],
  ] as const;
  return (
    <>
      <Link
        to="/logs"
        className="inline-flex min-h-11 w-fit items-center rounded-sm text-sm text-muted-foreground underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-8"
      >
        Back to logs
      </Link>
      <PageHeader title="Call" />
      <dl
        aria-label="Details"
        className="grid max-w-xl grid-cols-[auto_minmax(0,1fr)] gap-x-6 gap-y-3 text-sm"
      >
        {fields.map(([name, value]) => (
          <div key={name} className="contents">
            <dt className="text-muted-foreground">{name}</dt>
            <dd className="min-w-0 break-words">{value}</dd>
          </div>
        ))}
      </dl>
      {log.guardrails === null ? null : (
        <>
          <h2 className="text-lg font-medium">Guardrails</h2>
          <dl
            aria-label="Guardrails"
            className="grid max-w-xl grid-cols-[auto_minmax(0,1fr)] gap-x-6 gap-y-3 text-sm"
          >
            {sides.map(([name, side]) =>
              side === undefined || side === null ? null : (
                <div key={name} className="contents">
                  <dt className="text-muted-foreground">{name}</dt>
                  <dd className="flex min-w-0 flex-col gap-1 break-words">
                    <span>
                      <GuardrailBadge log={{ guardrails: { action: side.action } }} />
                    </span>
                    {sideLines(side).map((line) => (
                      <span key={line}>{line}</span>
                    ))}
                  </dd>
                </div>
              ),
            )}
          </dl>
        </>
      )}
      <h2 className="text-lg font-medium">Routing attempts</h2>
      <DataTable
        caption="Routing attempts"
        columns={columns}
        rows={log.attempts.map((attempt, n) => ({ ...attempt, n }))}
        getRowId={(attempt) => String(attempt.n)}
        empty={<EmptyState title="No attempts were recorded." />}
      />
    </>
  );
}

/** One call, with the targets it tried in the order it tried them. */
export function LogsDetail({ id }: { id: string }) {
  const number = idOf(id);
  if (number === null) return <NotFoundContent />;
  return <Details id={number} />;
}
