import { Link } from "@tanstack/react-router";
import { useEffect, useMemo } from "react";
import { useLogsPages } from "@/api/queries";
import type { components } from "@/api/schema";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { Timestamp } from "@/components/Timestamp";
import { formatMoney, formatTokens } from "@/lib/usage";
import { costOf, modelOf } from "@/pages/Logs";

type Log = components["schemas"]["LogView"];

const NONE = "—";

function pair(a: number | null, b: number | null): string {
  if (a === null && b === null) return NONE;
  return `${formatTokens(a ?? 0)} / ${formatTokens(b ?? 0)}`;
}

const columns: Column<Log>[] = [
  {
    id: "time",
    header: "Time",
    cell: (log) => (
      <Link
        to="/logs/$id"
        params={{ id: String(log.id) }}
        className="inline-flex min-h-11 items-center rounded-sm underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-0"
      >
        <Timestamp value={log.at} />
      </Link>
    ),
  },
  { id: "model", header: "Model", cell: (log) => <span className="break-all">{modelOf(log)}</span> },
  {
    id: "tokens",
    header: "Tokens in / out",
    cell: (log) => <span className="tabular-nums">{pair(log.input_tokens, log.output_tokens)}</span>,
  },
  {
    id: "cache",
    header: "Cache reads / writes",
    cell: (log) => (
      <span className="tabular-nums">{pair(log.cache_read_tokens, log.cache_write_tokens)}</span>
    ),
  },
  { id: "cost", header: "Cost", cell: (log) => <span className="tabular-nums">{costOf(log)}</span> },
];

function sum(logs: readonly Log[], of: (log: Log) => number | null): number {
  return logs.reduce((total, log) => total + (of(log) ?? 0), 0);
}

/**
 * The calls of one coding-agent session, oldest first, with what they came to
 * together. Every page of the session is read: a session is a bounded thing.
 */
export function LogsSession({ id }: { id: string }) {
  const log = useLogsPages({ agent_session: id }, 0);
  const { hasNextPage, isFetchingNextPage, fetchNextPage, isFetchNextPageError } = log;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage && !isFetchNextPageError) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, isFetchNextPageError, fetchNextPage]);
  const calls = useMemo(
    () => [...(log.data?.pages.flatMap((page) => page.logs) ?? [])].sort((a, b) => a.id - b.id),
    [log.data],
  );
  if (log.error !== null && log.data === undefined) {
    return (
      <QueryProblem
        title={`Session ${id}`}
        error={log.error}
        onRetry={() => {
          void log.refetch();
        }}
      />
    );
  }
  const totals: [string, string][] = [
    ["Calls", String(calls.length)],
    ["Tokens in / out", pair(sum(calls, (c) => c.input_tokens), sum(calls, (c) => c.output_tokens))],
    [
      "Cache reads / writes",
      pair(sum(calls, (c) => c.cache_read_tokens), sum(calls, (c) => c.cache_write_tokens)),
    ],
    ["Cost", formatMoney(sum(calls, (c) => c.cost_micros))],
  ];
  return (
    <>
      <Link
        to="/logs"
        className="inline-flex min-h-11 w-fit items-center rounded-sm text-sm text-muted-foreground underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-8"
      >
        Back to logs
      </Link>
      <PageHeader title={`Session ${id}`} />
      <dl
        aria-label="Totals"
        className="grid max-w-xl grid-cols-[auto_minmax(0,1fr)] gap-x-6 gap-y-3 text-sm"
      >
        {totals.map(([name, value]) => (
          <div key={name} className="contents">
            <dt className="text-muted-foreground">{name}</dt>
            <dd className="min-w-0 break-words tabular-nums">{value}</dd>
          </div>
        ))}
      </dl>
      <DataTable
        caption="Session calls"
        columns={columns}
        rows={calls}
        loading={log.isPending || hasNextPage}
        getRowId={(one) => String(one.id)}
        empty={<EmptyState title="No calls were logged for this session." />}
      />
    </>
  );
}
