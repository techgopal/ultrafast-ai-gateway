import { useState } from "react";
import {
  useBudgets,
  useDeleteBudget,
  useDeleteLimit,
  useLimits,
  useSetBudget,
  useSetLimit,
} from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { PageHeader } from "@/components/PageHeader";
import { useToast } from "@/components/toast";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { actionText, percentOf, periodText } from "@/lib/limits";
import { formatDollars } from "@/lib/money";
import { formatTokens } from "@/lib/usage";
import { BudgetDialog } from "@/pages/LimitsBudgetsEdit";
import { LimitDialog } from "@/pages/LimitsEdit";

type Limit = components["schemas"]["LimitView"];
type Budget = components["schemas"]["BudgetView"];

export const DONE = {
  limitSaved: "Limit saved.",
  limitDeleted: "Limit deleted.",
  budgetSaved: "Budget saved.",
  budgetDeleted: "Budget deleted.",
} as const;

export const LIMIT_DELETE_CONSEQUENCE = "Calls are no longer limited by this.";
export const BUDGET_DELETE_CONSEQUENCE = "Spend is no longer capped by this budget.";

const NOT_SET = "Not set";
const NOT_SHOWN = "Not shown";

const count = (value: number | null) => (value === null ? NOT_SET : formatTokens(value));

const scopeColumn = {
  id: "scope",
  header: "Scope",
  cell: (row: { label: string }) => <span className="break-all">{row.label}</span>,
};

const limitColumns: Column<Limit>[] = [
  { ...scopeColumn, sortValue: (row) => row.label },
  {
    id: "requests",
    header: "Requests/min",
    cell: (row) => <span className="tabular-nums">{count(row.requests_per_minute)}</span>,
    sortValue: (row) => row.requests_per_minute,
  },
  {
    id: "tokens",
    header: "Tokens/min",
    cell: (row) => <span className="tabular-nums">{count(row.tokens_per_minute)}</span>,
    sortValue: (row) => row.tokens_per_minute,
  },
  {
    id: "concurrent",
    header: "Concurrent",
    cell: (row) => <span className="tabular-nums">{count(row.concurrent)}</span>,
    sortValue: (row) => row.concurrent,
  },
];

const budgetColumnsBeforeSpent: Column<Budget>[] = [
  { ...scopeColumn, sortValue: (row) => row.label },
  {
    id: "period",
    header: "Period",
    cell: (row) => (
      <span className="inline-flex flex-col">
        <span>{periodText(row.period)}</span>
        <span className="text-xs text-muted-foreground">Since {row.period_start}</span>
      </span>
    ),
    sortValue: (row) => row.period,
  },
  {
    id: "amount",
    header: "Amount",
    cell: (row) => <span className="tabular-nums">{formatDollars(row.amount_micros)}</span>,
    sortValue: (row) => row.amount_micros,
  },
];

const spentColumn: Column<Budget> = {
    id: "spent",
    header: "Spent this period",
    cell: (row) => {
      // The gateway does not give this viewer that spend.
      if (row.spent_micros === null) {
        return <span className="text-muted-foreground">{NOT_SHOWN}</span>;
      }
      const percent = percentOf(row.spent_micros, row.amount_micros);
      const over = row.spent_micros >= row.amount_micros;
      return (
        <span className="inline-flex min-w-32 flex-col gap-1">
          <span className={`tabular-nums ${over ? "text-destructive" : ""}`}>
            {formatDollars(row.spent_micros)} ({percent}%)
          </span>
          <progress
            aria-label="Spent this period"
            value={row.spent_micros}
            max={row.amount_micros}
            className={`h-2 w-full appearance-none overflow-hidden rounded-full [&::-webkit-progress-bar]:bg-muted ${
              over
                ? "[&::-moz-progress-bar]:bg-destructive [&::-webkit-progress-value]:bg-destructive"
                : "[&::-moz-progress-bar]:bg-primary [&::-webkit-progress-value]:bg-primary"
            }`}
          />
        </span>
      );
    },
    sortValue: (row) =>
      row.spent_micros === null ? null : percentOf(row.spent_micros, row.amount_micros),
  };

const budgetColumnsAfterSpent: Column<Budget>[] = [
  {
    id: "action",
    header: "Action",
    cell: (row) => (
      <Badge variant={row.action === "block" ? "secondary" : "outline"}>
        {actionText(row.action)}
      </Badge>
    ),
    sortValue: (row) => row.action,
  },
];

type Asking = "set" | "delete";

function LimitsSection({ admin }: { admin: boolean }) {
  const limits = useLimits();
  const put = useSetLimit();
  const remove = useDeleteLimit();
  const toast = useToast();
  const [asking, setAsking] = useState<Asking | null>(null);
  // Which limit the dialog is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Limit | null>(null);
  const rows = limits.data?.limits ?? [];
  const failed = limits.error !== null && limits.data === undefined;

  function ask(what: Asking, row: Limit | null) {
    return () => {
      setTarget(row);
      setAsking(what);
    };
  }
  function closing(reset: () => void) {
    setAsking(null);
    reset();
  }

  return (
    <section aria-labelledby="limits-title" className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 id="limits-title" className="text-lg font-medium">
          Limits
        </h2>
        {admin ? (
          <Button type="button" className={control} onClick={ask("set", null)}>
            Set limit
          </Button>
        ) : null}
      </div>
      {failed ? (
        <ErrorState
          error={limits.error}
          onRetry={() => {
            void limits.refetch();
          }}
        />
      ) : (
        <DataTable
          caption="Limits"
          columns={limitColumns}
          rows={rows}
          loading={limits.isPending}
          getRowId={(row) => String(row.id)}
          empty={
            admin ? (
              <EmptyState
                title="No limits"
                description="Set a limit to cap requests, tokens or concurrent calls."
              />
            ) : (
              <EmptyState title="No limits apply to you." />
            )
          }
          {...(admin
            ? {
                actions: (row: Limit) => (
                  <>
                    <Button type="button" variant="outline" className={control} onClick={ask("set", row)}>
                      Edit
                    </Button>
                    <Button type="button" variant="outline" className={control} onClick={ask("delete", row)}>
                      Delete
                    </Button>
                  </>
                ),
              }
            : {})}
        />
      )}
      {admin ? (
        <>
          <LimitDialog
            open={asking === "set"}
            row={target}
            put={put}
            onCancel={() => {
              closing(put.reset);
            }}
            onDone={() => {
              closing(put.reset);
              toast(DONE.limitSaved);
            }}
          />
          <ConfirmDialog
            open={asking === "delete"}
            onOpenChange={(open) => {
              if (!open) closing(remove.reset);
            }}
            title={`Delete the limits of ${target?.label ?? "this target"}?`}
            body={LIMIT_DELETE_CONSEQUENCE}
            confirmLabel="Delete"
            tone="danger"
            onConfirm={async () => {
              if (target === null) return;
              await remove.mutateAsync({ id: target.id });
              toast(DONE.limitDeleted);
            }}
          />
        </>
      ) : null}
    </section>
  );
}

function BudgetsSection({ admin }: { admin: boolean }) {
  const budgets = useBudgets();
  const put = useSetBudget();
  const remove = useDeleteBudget();
  const toast = useToast();
  const [asking, setAsking] = useState<Asking | null>(null);
  const [target, setTarget] = useState<Budget | null>(null);
  const rows = budgets.data?.budgets ?? [];
  const failed = budgets.error !== null && budgets.data === undefined;
  // A member's list has no spent figure for the gateway or a team: the
  // column is left out when no row has one.
  const columns =
    rows.length > 0 && rows.every((row) => row.spent_micros === null)
      ? [...budgetColumnsBeforeSpent, ...budgetColumnsAfterSpent]
      : [...budgetColumnsBeforeSpent, spentColumn, ...budgetColumnsAfterSpent];


  function ask(what: Asking, row: Budget | null) {
    return () => {
      setTarget(row);
      setAsking(what);
    };
  }
  function closing(reset: () => void) {
    setAsking(null);
    reset();
  }

  return (
    <section aria-labelledby="budgets-title" className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 id="budgets-title" className="text-lg font-medium">
          Budgets
        </h2>
        {admin ? (
          <Button type="button" className={control} onClick={ask("set", null)}>
            Set budget
          </Button>
        ) : null}
      </div>
      {failed ? (
        <ErrorState
          error={budgets.error}
          onRetry={() => {
            void budgets.refetch();
          }}
        />
      ) : (
        <DataTable
          caption="Budgets"
          columns={columns}
          rows={rows}
          loading={budgets.isPending}
          getRowId={(row) => String(row.id)}
          empty={
            admin ? (
              <EmptyState title="No budgets" description="Set a budget to cap spend." />
            ) : (
              <EmptyState title="No budget applies to you." />
            )
          }
          {...(admin
            ? {
                actions: (row: Budget) => (
                  <>
                    <Button type="button" variant="outline" className={control} onClick={ask("set", row)}>
                      Edit
                    </Button>
                    <Button type="button" variant="outline" className={control} onClick={ask("delete", row)}>
                      Delete
                    </Button>
                  </>
                ),
              }
            : {})}
        />
      )}
      {admin ? (
        <>
          <BudgetDialog
            open={asking === "set"}
            row={target}
            put={put}
            onCancel={() => {
              closing(put.reset);
            }}
            onDone={() => {
              closing(put.reset);
              toast(DONE.budgetSaved);
            }}
          />
          <ConfirmDialog
            open={asking === "delete"}
            onOpenChange={(open) => {
              if (!open) closing(remove.reset);
            }}
            title={`Delete the ${target === null ? "" : `${periodText(target.period).toLowerCase()} `}budget of ${target?.label ?? "this target"}?`}
            body={BUDGET_DELETE_CONSEQUENCE}
            confirmLabel="Delete"
            tone="danger"
            onConfirm={async () => {
              if (target === null) return;
              await remove.mutateAsync({ id: target.id });
              toast(DONE.budgetDeleted);
            }}
          />
        </>
      ) : null}
    </section>
  );
}

/**
 * Limits on calls and budgets on spend. An admin sets and deletes them;
 * everybody else reads those that apply to them.
 */
export function Limits() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  const manageLimits = can(session.me, { type: "manageLimits" });
  const manageBudgets = can(session.me, { type: "manageBudgets" });
  return (
    <>
      <PageHeader
        title="Budgets and limits"
        {...(manageLimits || manageBudgets ? {} : { subtitle: "What applies to you." })}
      />
      <LimitsSection admin={manageLimits} />
      <BudgetsSection admin={manageBudgets} />
    </>
  );
}
