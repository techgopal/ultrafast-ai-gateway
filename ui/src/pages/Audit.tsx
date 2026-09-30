import { useId, useMemo, useState } from "react";
import { useAuditFromTheStart, useAuditPages } from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { Timestamp } from "@/components/Timestamp";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { entriesMatching, isFilter } from "@/lib/audit";

type AuditEntry = components["schemas"]["AuditRow"];

export const LOADED_ONLY = "Filtering the loaded entries only. Load older to look further.";

const control = "min-h-11 md:min-h-8";

// No column sorts: the order of the log is the order of the table. The
// summary and the action are texts of the gateway, and are shown as text.
const columns: Column<AuditEntry>[] = [
  {
    id: "at",
    header: "Time",
    cell: (entry) => <Timestamp value={entry.at} />,
  },
  {
    id: "actor",
    header: "Actor",
    cell: (entry) => <span className="break-all">{entry.actor_email}</span>,
  },
  {
    id: "action",
    header: "Action",
    cell: (entry) => <span className="font-mono">{entry.action}</span>,
  },
  {
    id: "summary",
    header: "Summary",
    cell: (entry) => (
      <span className="break-words md:block md:max-w-xl md:whitespace-normal">{entry.summary}</span>
    ),
  },
];

function AuditLog() {
  const log = useAuditPages();
  const startAgain = useAuditFromTheStart();
  const [search, setSearch] = useState("");
  const noticeId = useId();
  const loaded = useMemo(() => log.data?.pages.flatMap((page) => page.entries) ?? [], [log.data]);
  const rows = useMemo(() => entriesMatching(loaded, search), [loaded, search]);

  if (log.error !== null && log.data === undefined) {
    return (
      <>
        <PageHeader title="Audit log" />
        <QueryProblem
          error={log.error}
          onRetry={() => {
            void log.refetch();
          }}
        />
      </>
    );
  }

  const older = () => {
    void log.fetchNextPage();
  };
  const empty =
    loaded.length === 0 ? (
      <EmptyState title="No audit entries" description="Nothing has been recorded yet." />
    ) : (
      <EmptyState title="No entries match" description="Change the filter to see more entries." />
    );
  // The filter looks at what is loaded. While there may be older entries,
  // what it shows is not all there is, whether it shows something or nothing.
  const loadedOnly = isFilter(search) && log.hasNextPage;

  return (
    <>
      <PageHeader
        title="Audit log"
        actions={
          // It is not disabled while the log is read: it keeps the focus, and
          // a second press starts the log again as the first did.
          <Button type="button" variant="outline" className={control} onClick={startAgain}>
            Refresh
          </Button>
        }
      />
      {log.isPending || loaded.length > 0 ? (
        <div className="flex flex-col gap-2">
          <Input
            type="search"
            aria-label="Filter"
            aria-describedby={loadedOnly ? noticeId : undefined}
            placeholder="Filter by actor, action or summary"
            autoComplete="off"
            className={`${control} w-full sm:w-80`}
            value={search}
            onChange={(event) => {
              setSearch(event.target.value);
            }}
          />
          {loadedOnly ? (
            <p id={noticeId} role="status" className="text-sm text-muted-foreground">
              {LOADED_ONLY}
            </p>
          ) : null}
        </div>
      ) : null}
      <DataTable
        caption="Audit log"
        columns={columns}
        rows={rows}
        loading={log.isPending}
        getRowId={(entry) => String(entry.id)}
        empty={empty}
      />
      {log.isFetchNextPageError && !log.isFetchingNextPage ? (
        // What is loaded stays. Retry asks for the same older entries again.
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
            {log.isFetchingNextPage ? "Loading older entries" : "Load older"}
          </Button>
        </div>
      ) : null}
    </>
  );
}

/** What was done in the gateway, and by whom: for admins. */
export function Audit() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  // The gateway refuses it to everybody else: it is not asked.
  if (!can(session.me, { type: "viewAudit" })) return <NotAvailableContent />;
  return <AuditLog />;
}
