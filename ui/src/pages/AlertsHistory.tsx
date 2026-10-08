import { useMemo, useState } from "react";
import {
  useAlertEventsPages,
  useAlertRules,
  useBudgets,
  useKeys,
  type AlertEventsFilter,
} from "@/api/queries";
import type { components } from "@/api/schema";
import { control, longText } from "@/components/classes";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { FilterSelect, type Choice } from "@/components/FilterSelect";
import { Timestamp } from "@/components/Timestamp";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  deliveriesOf,
  deliveryLine,
  deliverySummary,
  subjectText,
  type Lookups,
} from "@/lib/alerts";

type Event = components["schemas"]["EventView"];

/** The value of a select that leaves nothing out. No id is written so. */
const ANY = "*";

const STATES: readonly Choice[] = [
  { value: ANY, label: "Any state" },
  { value: "firing", label: "Firing" },
  { value: "resolved", label: "Resolved" },
  { value: "test", label: "Test" },
];

const STATE_TEXT: Record<string, string> = {
  firing: "Firing",
  resolved: "Resolved",
  test: "Test",
};

function stateBadge(state: string) {
  const variant = state === "firing" ? "destructive" : state === "resolved" ? "default" : "outline";
  return <Badge variant={variant}>{STATE_TEXT[state] ?? state}</Badge>;
}

export function AlertsHistory() {
  const [rule, setRule] = useState(ANY);
  const [state, setState] = useState(ANY);
  const [run, setRun] = useState(0);
  const rules = useAlertRules();
  const budgets = useBudgets();
  const keys = useKeys();
  const ruleChoices = useMemo(
    (): Choice[] => [
      { value: ANY, label: "Any rule" },
      ...(rules.data?.rules ?? []).map((one) => ({ value: String(one.id), label: one.name })),
    ],
    [rules.data],
  );
  // A rule that is not offered any more (it was deleted) leaves nothing out.
  const effectiveRule = ruleChoices.some((choice) => choice.value === rule) ? rule : ANY;
  const filter: AlertEventsFilter = {
    ...(effectiveRule === ANY ? {} : { rule_id: Number(effectiveRule) }),
    ...(state === ANY ? {} : { state }),
  };
  const events = useAlertEventsPages(filter, run);
  const loaded = useMemo(() => events.data?.pages.flatMap((page) => page.events) ?? [], [events.data]);
  const lookups = useMemo(
    (): Lookups => ({ budgets: budgets.data?.budgets ?? [], keys: keys.data?.keys ?? [] }),
    [budgets.data, keys.data],
  );

  const columns: Column<Event>[] = [
    { id: "time", header: "Time", cell: (event) => <Timestamp value={event.at} /> },
    {
      id: "rule",
      header: "Rule",
      cell: (event) => (
        <span className={`${longText} break-words md:max-w-48`}>{event.rule_name}</span>
      ),
    },
    { id: "state", header: "State", cell: (event) => stateBadge(event.state) },
    {
      id: "subject",
      header: "Subject",
      cell: (event) => (
        <span className={`${longText} break-words md:max-w-48`}>
          {subjectText(event.subject, lookups)}
        </span>
      ),
    },
    {
      id: "summary",
      header: "Summary",
      cell: (event) => (
        <span className={`${longText} break-words md:max-w-80`}>{event.summary}</span>
      ),
    },
    {
      id: "deliveries",
      header: "Deliveries",
      cell: (event) => {
        const deliveries = deliveriesOf(event);
        return (
          <span className="inline-flex flex-col items-start gap-1">
            <span>{deliverySummary(deliveries)}</span>
            {deliveries.length === 0 ? null : (
              <ul className="flex flex-col gap-0.5 text-xs text-muted-foreground">
                {deliveries.map((delivery, index) => (
                  <li key={`${String(delivery.channel_id)}-${String(index)}`} className="break-words">
                    {deliveryLine(delivery)}
                  </li>
                ))}
              </ul>
            )}
          </span>
        );
      },
    },
  ];

  const filtered = effectiveRule !== ANY || state !== ANY;
  const retry = () => {
    void events.refetch();
  };
  const older = () => {
    void events.fetchNextPage();
  };

  return (
    <>
      <div role="group" aria-label="Filters" className="flex flex-wrap items-center gap-2">
        <FilterSelect label="Rule" value={effectiveRule} choices={ruleChoices} onChange={setRule} />
        <FilterSelect label="State" value={state} choices={STATES} onChange={setState} />
        <Button
          type="button"
          variant="outline"
          className={control}
          onClick={() => {
            setRun((before) => before + 1);
          }}
        >
          Refresh
        </Button>
      </div>
      {events.error !== null && events.data === undefined ? (
        <ErrorState error={events.error} onRetry={retry} />
      ) : (
        <DataTable
          caption="Alert history"
          columns={columns}
          rows={loaded}
          loading={events.isPending}
          getRowId={(event) => String(event.id)}
          empty={
            filtered ? (
              <EmptyState
                title="No events match"
                description="Change the filters to see more events."
              />
            ) : (
              <EmptyState
                title="No alerts yet"
                description="Events appear here when a rule fires or resolves, or when a test is sent."
              />
            )
          }
        />
      )}
      {events.isFetchNextPageError && !events.isFetchingNextPage ? (
        <ErrorState error={events.error} onRetry={older} />
      ) : events.hasNextPage ? (
        <div>
          <Button
            type="button"
            variant="outline"
            className={control}
            disabled={events.isFetchingNextPage}
            onClick={older}
          >
            {events.isFetchingNextPage ? "Loading older events" : "Load older"}
          </Button>
        </div>
      ) : null}
    </>
  );
}
