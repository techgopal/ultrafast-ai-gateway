import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useDeletePrompt, usePrompts } from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control, longText } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

type Prompt = components["schemas"]["PromptSummary"];

export const DONE = {
  delete: "Template deleted.",
} as const;

export const SUBTITLE =
  "Reusable messages with variables. A call names a template and gives the values of its variables.";

/** What deleting the template does: its versions go, and calls that name it fail. */
export function deleteConsequence(prompt: Pick<Prompt, "version_count">): string {
  const count = prompt.version_count;
  return `Its ${String(count)} ${count === 1 ? "version is" : "versions are"} deleted and calls that name it fail. The logs keep the name and version they recorded. This cannot be undone.`;
}

function PromptList() {
  const session = useSession();
  const list = usePrompts();
  const remove = useDeletePrompt();
  const toast = useToast();
  const [asking, setAsking] = useState<Prompt | null>(null);
  // Which template the dialog is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Prompt | null>(null);
  if (session.status !== "signedIn") return null;
  const { me } = session;
  const mayCreate = can(me, { type: "createPrompt" });

  const columns: Column<Prompt>[] = [
    {
      id: "name",
      header: "Name",
      cell: (prompt) => (
        <span className="flex flex-col items-start gap-1">
          <span className={`${longText} font-medium break-all md:max-w-64`}>{prompt.name}</span>
        </span>
      ),
      sortValue: (prompt) => prompt.name,
    },
    {
      id: "description",
      header: "Description",
      cell: (prompt) =>
        prompt.description === "" ? (
          <span className="text-muted-foreground">None</span>
        ) : (
          <span className={`${longText} break-words md:max-w-64`}>{prompt.description}</span>
        ),
      hideOnMobile: true,
    },
    {
      id: "latest",
      header: "Latest",
      cell: (prompt) =>
        prompt.unreadable ? (
          <Badge variant="destructive">Cannot be read</Badge>
        ) : (
          <span>{`Version ${String(prompt.latest_version)} of ${String(prompt.version_count)}`}</span>
        ),
      sortValue: (prompt) => prompt.latest_version,
    },
    {
      id: "model",
      header: "Model",
      cell: (prompt) =>
        prompt.model === null ? (
          <span className="text-muted-foreground">A call names one</span>
        ) : (
          <span className={`${longText} break-all md:max-w-64`}>{prompt.model}</span>
        ),
      sortValue: (prompt) => prompt.model,
    },
    {
      id: "variables",
      header: "Variables",
      cell: (prompt) =>
        prompt.variables.length === 0 ? (
          <span className="text-muted-foreground">None</span>
        ) : (
          <span className={`${longText} font-mono text-xs break-all md:max-w-64`}>
            {prompt.variables.join(", ")}
          </span>
        ),
    },
    {
      id: "updated",
      header: "Updated",
      cell: (prompt) => <Timestamp value={prompt.updated_at} />,
      sortValue: (prompt) => prompt.updated_at,
    },
  ];

  if (list.error !== null && list.data === undefined) {
    return (
      <QueryProblem
        title="Prompts"
        error={list.error}
        onRetry={() => {
          void list.refetch();
        }}
      />
    );
  }

  return (
    <>
      <PageHeader
        title="Prompts"
        subtitle={SUBTITLE}
        actions={
          mayCreate ? (
            <Button asChild className={control}>
              <Link to="/prompts/new">New template</Link>
            </Button>
          ) : undefined
        }
      />
      <DataTable
        caption="Prompt templates"
        columns={columns}
        rows={list.data?.prompts ?? []}
        loading={list.isPending}
        getRowId={(prompt) => String(prompt.id)}
        empty={
          <EmptyState
            title="No prompt templates"
            description={
              mayCreate
                ? "Make a template to reuse messages and fill in their variables from a call."
                : "An admin or a team lead can make one."
            }
          />
        }
        actions={(prompt) => (
          <>
            <Button asChild variant="outline" className={control}>
              <Link to="/prompts/$id" params={{ id: String(prompt.id) }}>
                Open
              </Link>
            </Button>
            {prompt.unreadable ? null : (
              <Button asChild variant="outline" className={control}>
                <Link to="/playground" search={{ prompt: prompt.name, version: prompt.latest_version }}>
                  Open in Playground
                </Link>
              </Button>
            )}
            {can(me, { type: "managePrompt", createdBy: prompt.created_by }) ? (
              <Button
                type="button"
                variant="outline"
                className={control}
                onClick={() => {
                  setTarget(prompt);
                  setAsking(prompt);
                }}
              >
                Delete
              </Button>
            ) : null}
          </>
        )}
      />
      <ConfirmDialog
        open={asking !== null}
        onOpenChange={(open) => {
          if (open) return;
          setAsking(null);
          remove.reset();
        }}
        title={`Delete ${target?.name ?? "this template"}?`}
        body={target === null ? "" : deleteConsequence(target)}
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

/** The prompt templates: anyone signed in reads them; an admin and the lead of a team make them. */
export function Prompts() {
  return <PromptList />;
}
