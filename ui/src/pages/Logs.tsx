import { Link } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { useLogsPages, type LogsFilter } from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control } from "@/components/classes";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { PageHeader } from "@/components/PageHeader";
import { ApiError } from "@/api/errors";
import { QueryProblem } from "@/components/QueryProblem";
import { TagChips } from "@/components/TagChips";
import { Timestamp } from "@/components/Timestamp";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { LOGGED_ACTIONS, loggedLabel } from "@/lib/guardrails";
import { formatDuration, formatMoney, formatTokens, sinceFor } from "@/lib/usage";
import {
  ANY,
  chosen,
  LogsFilters,
  NO_FILTERS,
  useOffered,
  type Filters,
} from "@/pages/LogsFilters";

type Log = components["schemas"]["LogView"];

const NONE = "—";

/** The model that answered, or the name that was asked for when none did. */
export function modelOf(log: Pick<Log, "provider" | "model" | "requested">): string {
  return log.provider !== null && log.model !== null
    ? `${log.provider}/${log.model}`
    : log.requested;
}

/** What a call cost, as far as it is known. */
export function costOf(log: Log): string {
  if (log.priced) return formatMoney(log.cost_micros);
  // Tokens with no price: the cost is missing. Nothing reported: nothing to price.
  return log.input_tokens !== null || log.output_tokens !== null ? "Unpriced" : NONE;
}

export function tokensOf(log: Pick<Log, "input_tokens" | "output_tokens">): string {
  if (log.input_tokens === null && log.output_tokens === null) return NONE;
  const part = (count: number | null) => (count === null ? NONE : formatTokens(count));
  return `${part(log.input_tokens)} / ${part(log.output_tokens)}`;
}

/** What the filters ask of the API. A choice not offered any more is not sent. */
function queryOf(filters: Filters, now: number, offered: ReturnType<typeof useOffered>): LogsFilter {
  const query: LogsFilter = {};
  if (filters.range === "custom") {
    if (filters.from !== "") query.from = filters.from;
    if (filters.to !== "") query.to = filters.to;
  } else {
    query.from = sinceFor(filters.range, now);
  }
  const key = chosen(filters.key, offered.keys);
  const user = chosen(filters.user, offered.users);
  const team = chosen(filters.team, offered.teams);
  if (key !== ANY) query.key_id = Number(key);
  if (user !== ANY) query.user_id = Number(user);
  if (team !== ANY) query.team_id = Number(team);
  if (filters.model !== "") query.model = filters.model;
  if (filters.errorsOnly) query.errors = true;
  const guardrail = LOGGED_ACTIONS.find(([value]) => value === filters.guardrail)?.[0];
  if (guardrail !== undefined) query.guardrail = guardrail;
  if (filters.tag !== "") query.tag = [filters.tag];
  return query;
}

/** What the guardrails did to the call at worst: a block, a redaction or a flag. */
export function GuardrailBadge({ log }: { log: Pick<Log, "guardrails"> }) {
  if (log.guardrails === null) return null;
  const { action } = log.guardrails;
  return (
    <Badge variant={action === "blocked" ? "destructive" : action === "redacted" ? "secondary" : "outline"}>
      {loggedLabel(action)}
    </Badge>
  );
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
  {
    id: "key",
    header: "Key",
    cell: (log) => (
      <span className="break-all">
        {log.key_name ?? (log.key_id === null ? NONE : "(deleted)")}
      </span>
    ),
  },
  { id: "user", header: "User", cell: (log) => <span className="break-all">{log.user_email ?? NONE}</span> },
  { id: "model", header: "Model", cell: (log) => <span className="break-all">{modelOf(log)}</span> },
  {
    id: "status",
    header: "Status",
    cell: (log) => (
      <span className="inline-flex flex-wrap items-center gap-1.5">
        <Badge variant={log.status >= 400 ? "destructive" : "secondary"}>{log.status}</Badge>
        {log.cached ? <Badge variant="outline">Cached</Badge> : null}
        {log.estimated ? <Badge variant="outline">Estimated</Badge> : null}
        <GuardrailBadge log={log} />
      </span>
    ),
  },
  { id: "tokens", header: "Tokens", cell: (log) => <span className="tabular-nums">{tokensOf(log)}</span> },
  { id: "cost", header: "Cost", cell: (log) => <span className="tabular-nums">{costOf(log)}</span> },
  {
    id: "duration",
    header: "Duration",
    cell: (log) => <span className="tabular-nums">{formatDuration(log.duration_ms)}</span>,
  },
  { id: "tags", header: "Tags", cell: (log) => <TagChips tags={log.tags} /> },
];

function LogsOf({ others }: { others: boolean }) {
  const [filters, setFilters] = useState<Filters>(NO_FILTERS);
  // The moment a preset range ends at: taken when the range is chosen and at
  // Refresh, so the key of the query does not change by itself.
  const [now, setNow] = useState(() => Date.now());
  const [run, setRun] = useState(0);
  const offered = useOffered(others);
  const query = queryOf(filters, now, offered);
  const log = useLogsPages(query, run);
  const loaded = useMemo(() => log.data?.pages.flatMap((page) => page.logs) ?? [], [log.data]);

  const change = (patch: Partial<Filters>) => {
    setFilters((before) => ({ ...before, ...patch }));
    if (patch.range !== undefined) setNow(Date.now());
  };
  const refresh = () => {
    setNow(Date.now());
    setRun((before) => before + 1);
  };
  const retry = () => {
    void log.refetch();
  };
  const older = () => {
    void log.fetchNextPage();
  };

  const header = (
    <PageHeader
      title="Logs"
      actions={
        <Button type="button" variant="outline" className={control} onClick={refresh}>
          Refresh
        </Button>
      }
    />
  );

  if (log.error !== null && log.data === undefined) {
    // Not available is the whole page; any other failure leaves the filters
    // to change and Refresh to press, with Retry for the same request.
    if (log.error instanceof ApiError && log.error.status === 403) {
      return <QueryProblem title="Logs" error={log.error} onRetry={retry} />;
    }
    return (
      <>
        {header}
        <LogsFilters filters={filters} offered={offered} others={others} onChange={change} />
        <ErrorState error={log.error} onRetry={retry} />
      </>
    );
  }

  return (
    <>
      {header}
      <LogsFilters filters={filters} offered={offered} others={others} onChange={change} />
      <DataTable
        caption="Request logs"
        columns={columns}
        rows={loaded}
        loading={log.isPending}
        getRowId={(one) => String(one.id)}
        empty={
          <EmptyState title="No calls" description="No calls were logged for these filters." />
        }
      />
      {log.isFetchNextPageError && !log.isFetchingNextPage ? (
        <ErrorState error={log.error} onRetry={older} />
      ) : log.hasNextPage ? (
        <div>
          <Button
            type="button"
            variant="outline"
            className={control}
            disabled={log.isFetchingNextPage}
            onClick={older}
          >
            {log.isFetchingNextPage ? "Loading older calls" : "Load older"}
          </Button>
        </div>
      ) : null}
    </>
  );
}

/** The calls through the gateway that the viewer may see: their own, their teams', or all. */
export function Logs() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  return <LogsOf others={can(session.me, { type: "viewOthersUsage" })} />;
}
