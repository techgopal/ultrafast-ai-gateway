import { Link } from "@tanstack/react-router";
import { useState } from "react";
import {
  useDeleteGuardrail,
  useGuardrails,
  useRotateGuardrailSecret,
  useUpdateGuardrail,
} from "@/api/queries";
import { messageOfError } from "@/api/errors";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control, longText } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { useToast } from "@/components/toast";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { directionsLabel } from "@/lib/guardrails";
import { Protocol } from "@/pages/GuardrailsProtocol";
import { ROTATE_CONSEQUENCE, SECRET_SHOWN_ONCE, SecretNote } from "@/pages/GuardrailsSecret";

type Guardrail = components["schemas"]["GuardrailView"];

export const DONE = {
  delete: "Guardrail deleted.",
} as const;

/** What deleting the guardrail does to the routes and keys it is on. */
export function deleteConsequence(guardrail: Guardrail): string {
  const routes = guardrail.routes.map((route) => route.name);
  const parts: string[] = [];
  if (guardrail.is_default) parts.push("every call");
  if (routes.length > 0) parts.push(`the routes ${routes.join(", ")}`);
  if (guardrail.key_count > 0) {
    parts.push(`${String(guardrail.key_count)} ${guardrail.key_count === 1 ? "key" : "keys"}`);
  }
  return parts.length === 0
    ? "It is attached to nothing."
    : `Calls stop being checked by it on ${parts.join(", ")}. This cannot be undone.`;
}

function appliesTo(guardrail: Guardrail) {
  const routes = guardrail.routes.map((route) => route.name);
  const none = !guardrail.is_default && routes.length === 0 && guardrail.key_count === 0;
  return (
    <span className="flex flex-col items-start gap-1">
      {guardrail.is_default ? <Badge variant="secondary">Every call</Badge> : null}
      {routes.length === 0 ? null : (
        <span className={`${longText} break-words md:max-w-64`}>{`Routes: ${routes.join(", ")}`}</span>
      )}
      {guardrail.key_count === 0 ? null : (
        <span>{`${String(guardrail.key_count)} ${guardrail.key_count === 1 ? "key" : "keys"}`}</span>
      )}
      {none ? <span className="text-muted-foreground">Nothing yet</span> : null}
    </span>
  );
}

type Asking = "delete" | "rotate";

function GuardrailList() {
  const list = useGuardrails();
  const update = useUpdateGuardrail();
  const remove = useDeleteGuardrail();
  const rotate = useRotateGuardrailSecret();
  const rotated = useSecretOnce(rotate);
  const toast = useToast();
  const [asking, setAsking] = useState<Asking | null>(null);
  // Which guardrail the dialog is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Guardrail | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [changing, setChanging] = useState<ReadonlySet<number>>(new Set());

  async function toggle(guardrail: Guardrail, enabled: boolean) {
    setProblem(null);
    setChanging((now) => new Set(now).add(guardrail.id));
    try {
      await update.mutateAsync({ id: guardrail.id, body: { enabled } });
    } catch (error) {
      // A session that is over says nothing; the clean-up of the session leaves the page.
      setProblem(messageOfError(error));
    } finally {
      setChanging((now) => {
        const next = new Set(now);
        next.delete(guardrail.id);
        return next;
      });
    }
  }

  function askAbout(what: Asking, guardrail: Guardrail) {
    return () => {
      setProblem(null);
      setTarget(guardrail);
      setAsking(what);
    };
  }

  function closeQuestion(reset: () => void) {
    return (open: boolean) => {
      if (open) return;
      setAsking(null);
      reset();
    };
  }

  const columns: Column<Guardrail>[] = [
    {
      id: "name",
      header: "Name",
      cell: (guardrail) => (
        <span className={`${longText} font-medium break-all md:max-w-64`}>{guardrail.name}</span>
      ),
      sortValue: (guardrail) => guardrail.name,
    },
    {
      id: "kind",
      header: "Kind",
      cell: (guardrail) => (
        <Badge variant="outline">{guardrail.kind === "external" ? "External" : "Rules"}</Badge>
      ),
      sortValue: (guardrail) => guardrail.kind,
    },
    {
      id: "checks",
      header: "Checks",
      cell: (guardrail) =>
        guardrail.kind === "external" ? (
          guardrail.url_host === "" ? (
            <Badge variant="secondary">Needs a URL</Badge>
          ) : (
            <span className="flex flex-col items-start gap-1">
              <span className={`${longText} break-all md:max-w-64`}>{guardrail.url_host}</span>
              <span className="text-xs text-muted-foreground">
                {`${directionsLabel(guardrail.directions ?? "both")}, fails ${guardrail.fail_mode ?? "open"}`}
              </span>
            </span>
          )
        ) : (
          <span>{`${String(guardrail.rules.length)} ${guardrail.rules.length === 1 ? "rule" : "rules"}`}</span>
        ),
    },
    { id: "applies", header: "Applies to", cell: appliesTo },
    {
      id: "enabled",
      header: "Enabled",
      cell: (guardrail) => (
        <Label className={`${control} gap-2`}>
          <Switch
            aria-label={guardrail.name}
            checked={guardrail.enabled}
            // A guardrail with no URL cannot be enabled: the gateway refuses it.
            disabled={
              changing.has(guardrail.id) ||
              (!guardrail.enabled && guardrail.kind === "external" && guardrail.url_host === "")
            }
            onCheckedChange={(on) => {
              void toggle(guardrail, on);
            }}
          />
          <span>{guardrail.enabled ? "Enabled" : "Disabled"}</span>
        </Label>
      ),
      sortValue: (guardrail) => (guardrail.enabled ? 1 : 0),
    },
  ];

  const failed = list.error !== null && list.data === undefined;
  if (failed) {
    return (
      <QueryProblem
        title="Guardrails"
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
        title="Guardrails"
        subtitle="Block, redact or flag what goes to models and what comes back."
        actions={
          <Button asChild className={control}>
            <Link to="/guardrails/new">Add guardrail</Link>
          </Button>
        }
      />
      {problem === null ? null : (
        <Alert variant="destructive">
          <AlertDescription>
            <p>{problem}</p>
          </AlertDescription>
        </Alert>
      )}
      <DataTable
        caption="Guardrails"
        columns={columns}
        rows={list.data?.guardrails ?? []}
        loading={list.isPending}
        getRowId={(guardrail) => String(guardrail.id)}
        empty={
          <EmptyState
            title="No guardrails"
            description="Add one to redact personal data or block words, then attach it to a route or a key."
          />
        }
        actions={(guardrail) => (
          <>
            <Button asChild variant="outline" className={control}>
              <Link to="/guardrails/$id" params={{ id: String(guardrail.id) }}>
                Edit
              </Link>
            </Button>
            {guardrail.kind === "external" ? (
              <Button
                type="button"
                variant="outline"
                className={control}
                onClick={askAbout("rotate", guardrail)}
              >
                Rotate secret
              </Button>
            ) : null}
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={askAbout("delete", guardrail)}
            >
              Delete
            </Button>
          </>
        )}
      />
      <Protocol />
      <ConfirmDialog
        open={asking === "rotate"}
        onOpenChange={closeQuestion(rotate.reset)}
        title={`Rotate the secret of ${target?.name ?? "this guardrail"}?`}
        body={ROTATE_CONSEQUENCE}
        confirmLabel="Rotate secret"
        tone="danger"
        onConfirm={async () => {
          if (target === null) return;
          const made = await rotate.mutateAsync({ id: target.id });
          rotated.show(made.secret);
        }}
      />
      <ConfirmDialog
        open={asking === "delete"}
        onOpenChange={closeQuestion(remove.reset)}
        title={`Delete ${target?.name ?? "this guardrail"}?`}
        body={target === null ? "" : deleteConsequence(target)}
        confirmLabel="Delete"
        tone="danger"
        onConfirm={async () => {
          if (target === null) return;
          await remove.mutateAsync({ id: target.id });
          toast(DONE.delete);
        }}
      />
      <SecretDialog
        title="New signing secret"
        description={SECRET_SHOWN_ONCE}
        secret={rotated.secret}
        onClose={rotated.clear}
      >
        <SecretNote />
      </SecretDialog>
    </>
  );
}

/** The guardrails, their rules and where they are attached: only an admin sees and changes them. */
export function Guardrails() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  if (!can(session.me, { type: "manageGuardrails" })) return <NotAvailableContent />;
  return <GuardrailList />;
}
