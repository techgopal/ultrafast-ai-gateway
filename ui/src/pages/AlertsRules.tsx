import { useMemo, useState } from "react";
import {
  useAlertRules,
  useBudgets,
  useCreateAlertRule,
  useDeleteAlertRule,
  useKeys,
  useUpdateAlertRule,
} from "@/api/queries";
import { messageOfError } from "@/api/errors";
import type { components } from "@/api/schema";
import { control, longText } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { QueryProblem } from "@/components/QueryProblem";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { conditionText, kindText, subjectText, type Lookups } from "@/lib/alerts";
import { RuleDialog } from "@/pages/AlertsRulesEdit";

type Rule = components["schemas"]["RuleView"];

export const DONE = {
  add: "Rule added.",
  update: "Rule updated.",
  delete: "Rule deleted.",
} as const;

export const DELETE_CONSEQUENCE = "It stops watching at once. Its past events stay in History.";

type Asking = "edit" | "delete";

export function AlertsRules() {
  const rules = useAlertRules();
  const budgets = useBudgets();
  const keys = useKeys();
  const create = useCreateAlertRule();
  const update = useUpdateAlertRule();
  const remove = useDeleteAlertRule();
  const toast = useToast();
  const [adding, setAdding] = useState(false);
  const [asking, setAsking] = useState<Asking | null>(null);
  // Which rule the dialog is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Rule | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [changing, setChanging] = useState<ReadonlySet<number>>(new Set());

  const lookups = useMemo(
    (): Lookups => ({
      budgets: budgets.data?.budgets ?? [],
      keys: keys.data?.keys ?? [],
    }),
    [budgets.data, keys.data],
  );

  async function toggle(rule: Rule, enabled: boolean) {
    setProblem(null);
    setChanging((now) => new Set(now).add(rule.id));
    try {
      await update.mutateAsync({ id: rule.id, body: { enabled } });
    } catch (error) {
      // A session that is over says nothing; the clean-up of the session leaves the page.
      setProblem(messageOfError(error));
    } finally {
      setChanging((now) => {
        const next = new Set(now);
        next.delete(rule.id);
        return next;
      });
    }
  }

  function closeForm() {
    setAdding(false);
    setAsking(null);
    create.reset();
    update.reset();
  }

  function closeQuestion(open: boolean) {
    if (open) return;
    setAsking(null);
    remove.reset();
  }

  const columns: Column<Rule>[] = [
    {
      id: "name",
      header: "Name",
      cell: (rule) => <span className={`${longText} font-medium break-all md:max-w-64`}>{rule.name}</span>,
      sortValue: (rule) => rule.name,
    },
    {
      id: "kind",
      header: "Kind",
      cell: (rule) => <Badge variant="outline">{kindText(rule.kind)}</Badge>,
      sortValue: (rule) => kindText(rule.kind),
    },
    {
      id: "condition",
      header: "Condition",
      cell: (rule) => (
        <span className={`${longText} break-words md:max-w-80`}>{conditionText(rule, lookups)}</span>
      ),
    },
    {
      id: "channels",
      header: "Channels",
      cell: (rule) =>
        rule.channels.length === 0 ? (
          <span className="text-muted-foreground">None</span>
        ) : (
          <span className={`${longText} break-words md:max-w-56`}>
            {rule.channels.map((channel) => channel.name).join(", ")}
          </span>
        ),
    },
    {
      id: "status",
      header: "Status",
      cell: (rule) =>
        rule.firing.length === 0 ? (
          <Badge variant="outline">OK</Badge>
        ) : (
          <span className="inline-flex flex-col items-start gap-1">
            <Badge variant="destructive">Firing</Badge>
            <ul className="flex flex-col gap-0.5 text-xs text-muted-foreground">
              {rule.firing.map((one) => (
                <li key={one.subject} className="break-words">
                  {subjectText(one.subject, lookups)}, since <Timestamp value={one.since} />
                </li>
              ))}
            </ul>
          </span>
        ),
      sortValue: (rule) => (rule.firing.length === 0 ? 0 : 1),
    },
    {
      id: "enabled",
      header: "Enabled",
      cell: (rule) => (
        <Label className={`${control} gap-2`}>
          <Switch
            aria-label={rule.name}
            checked={rule.enabled}
            disabled={changing.has(rule.id)}
            onCheckedChange={(on) => {
              void toggle(rule, on);
            }}
          />
          <span>{rule.enabled ? "Enabled" : "Disabled"}</span>
        </Label>
      ),
      sortValue: (rule) => (rule.enabled ? 1 : 0),
    },
  ];

  const failed = rules.error !== null && rules.data === undefined;
  if (failed) {
    return (
      <QueryProblem
        part
        error={rules.error}
        onRetry={() => {
          void rules.refetch();
        }}
      />
    );
  }

  return (
    <>
      <div className="flex justify-end">
        <Button
          type="button"
          className={control}
          onClick={() => {
            setProblem(null);
            setTarget(null);
            setAdding(true);
          }}
        >
          Add rule
        </Button>
      </div>
      {problem === null ? null : (
        <Alert variant="destructive">
          <AlertDescription>
            <p>{problem}</p>
          </AlertDescription>
        </Alert>
      )}
      <DataTable
        caption="Alert rules"
        columns={columns}
        rows={rules.data?.rules ?? []}
        loading={rules.isPending || budgets.isPending || keys.isPending}
        getRowId={(rule) => String(rule.id)}
        empty={
          <EmptyState
            title="No alert rules"
            description="Add a rule to be told when a budget, an error rate or a circuit needs attention."
          />
        }
        actions={(rule) => (
          <>
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                setProblem(null);
                setTarget(rule);
                setAsking("edit");
              }}
            >
              Edit
            </Button>
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                setProblem(null);
                setTarget(rule);
                setAsking("delete");
              }}
            >
              Delete
            </Button>
          </>
        )}
      />
      <RuleDialog
        open={adding || asking === "edit"}
        rule={target}
        create={create}
        update={update}
        onCancel={closeForm}
        onDone={(changed) => {
          const was = adding;
          closeForm();
          if (changed) toast(was ? DONE.add : DONE.update);
        }}
      />
      <ConfirmDialog
        open={asking === "delete"}
        onOpenChange={closeQuestion}
        title={`Delete ${target?.name ?? "this rule"}?`}
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
