import { useState } from "react";
import {
  useAlertChannels,
  useCreateAlertChannel,
  useDeleteAlertChannel,
  useRotateAlertChannelSecret,
  useTestAlertChannel,
  useUpdateAlertChannel,
} from "@/api/queries";
import { messageOfError } from "@/api/errors";
import type { components } from "@/api/schema";
import { control, longText } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { QueryProblem } from "@/components/QueryProblem";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { useToast } from "@/components/toast";
import { Alert, AlertAction, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { channelKindText, ChannelDialog } from "@/pages/AlertsChannelsEdit";
import { VerifySignatures } from "@/pages/AlertsVerify";

type Channel = components["schemas"]["ChannelView"];

export const DONE = {
  update: "Channel updated.",
  delete: "Channel deleted.",
} as const;

export const SECRET_SHOWN_ONCE = "Copy this signing secret now. It is not shown again.";
export const ROTATE_CONSEQUENCE =
  "The old secret stops working at once. Update the receiver with the new one.";

/** What deleting the channel does to the rules that send to it. */
export function deleteConsequence(channel: Channel): string {
  return channel.rules.length === 0
    ? "No rule sends here."
    : `These rules stop sending here: ${channel.rules.map((rule) => rule.name).join(", ")}.`;
}

interface TestResult {
  name: string;
  ok: boolean;
  status: number | null;
  error: string | null;
}

function testText(result: TestResult): string {
  if (result.ok) {
    return result.status === null
      ? `Test to ${result.name} succeeded.`
      : `Test to ${result.name}: the receiver answered ${String(result.status)}.`;
  }
  return `Test to ${result.name} failed: ${result.error ?? "the receiver did not answer"}.`;
}

function SecretNote() {
  return (
    <p className="text-sm text-muted-foreground">
      The receiver checks the <code className="font-mono">x-uf-signature</code> header with it. See
      How to verify signatures on this page.
    </p>
  );
}

type Asking = "edit" | "delete" | "rotate";

export function AlertsChannels() {
  const channels = useAlertChannels();
  const create = useCreateAlertChannel();
  const update = useUpdateAlertChannel();
  const remove = useDeleteAlertChannel();
  const rotate = useRotateAlertChannelSecret();
  const test = useTestAlertChannel();
  const created = useSecretOnce(create);
  const rotated = useSecretOnce(rotate);
  const toast = useToast();
  const [adding, setAdding] = useState(false);
  const [asking, setAsking] = useState<Asking | null>(null);
  // Which channel the dialog is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Channel | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [changing, setChanging] = useState<ReadonlySet<number>>(new Set());
  const [testing, setTesting] = useState<number | null>(null);
  const [result, setResult] = useState<TestResult | null>(null);

  async function toggle(channel: Channel, enabled: boolean) {
    setProblem(null);
    setChanging((now) => new Set(now).add(channel.id));
    try {
      await update.mutateAsync({ id: channel.id, body: { enabled } });
    } catch (error) {
      // A session that is over says nothing; the clean-up of the session leaves the page.
      setProblem(messageOfError(error));
    } finally {
      setChanging((now) => {
        const next = new Set(now);
        next.delete(channel.id);
        return next;
      });
    }
  }

  async function sendTest(channel: Channel) {
    setProblem(null);
    setResult(null);
    setTesting(channel.id);
    try {
      const answer = await test.mutateAsync({ id: channel.id });
      setResult({ name: channel.name, ok: answer.ok, status: answer.status, error: answer.error });
    } catch (error) {
      setProblem(messageOfError(error));
    } finally {
      setTesting(null);
    }
  }

  function closeForm() {
    setAdding(false);
    setAsking(null);
    create.reset();
    update.reset();
  }

  function closeQuestion(reset: () => void) {
    return (open: boolean) => {
      if (open) return;
      setAsking(null);
      reset();
    };
  }

  function askAbout(what: Asking, channel: Channel) {
    return () => {
      setProblem(null);
      setTarget(channel);
      setAsking(what);
    };
  }

  const columns: Column<Channel>[] = [
    {
      id: "name",
      header: "Name",
      cell: (channel) => (
        <span className={`${longText} font-medium break-all md:max-w-64`}>{channel.name}</span>
      ),
      sortValue: (channel) => channel.name,
    },
    {
      id: "kind",
      header: "Kind",
      cell: (channel) => <Badge variant="outline">{channelKindText(channel.kind)}</Badge>,
      sortValue: (channel) => channel.kind,
    },
    {
      id: "host",
      header: "Host",
      cell: (channel) =>
        channel.url_host === "" ? (
          <Badge variant="secondary">Needs a URL</Badge>
        ) : (
          <span className={`${longText} break-all md:max-w-64`}>{channel.url_host}</span>
        ),
      sortValue: (channel) => channel.url_host,
    },
    {
      id: "rules",
      header: "Rules",
      cell: (channel) =>
        channel.rules.length === 0 ? (
          <span className="text-muted-foreground">None</span>
        ) : (
          <span className={`${longText} break-words md:max-w-64`}>
            {channel.rules.map((rule) => rule.name).join(", ")}
          </span>
        ),
    },
    {
      id: "enabled",
      header: "Enabled",
      cell: (channel) => (
        <Label className={`${control} gap-2`}>
          <Switch
            aria-label={channel.name}
            checked={channel.enabled}
            // A channel with no URL cannot be enabled: the gateway refuses it.
            disabled={changing.has(channel.id) || (!channel.enabled && channel.url_host === "")}
            onCheckedChange={(on) => {
              void toggle(channel, on);
            }}
          />
          <span>{channel.enabled ? "Enabled" : "Disabled"}</span>
        </Label>
      ),
      sortValue: (channel) => (channel.enabled ? 1 : 0),
    },
  ];

  const failed = channels.error !== null && channels.data === undefined;
  if (failed) {
    return (
      <QueryProblem
        part
        error={channels.error}
        onRetry={() => {
          void channels.refetch();
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
          Add channel
        </Button>
      </div>
      <div role="status" className="not-empty:contents">
        {result === null ? null : (
          // The announcement is that of the place: the alert is none of its own.
          <Alert role="presentation" variant={result.ok ? "default" : "destructive"}>
            <AlertDescription>
              <p>{testText(result)}</p>
            </AlertDescription>
            <AlertAction>
              <Button
                type="button"
                variant="outline"
                className={control}
                onClick={() => {
                  setResult(null);
                }}
              >
                Dismiss
              </Button>
            </AlertAction>
          </Alert>
        )}
      </div>
      {problem === null ? null : (
        <Alert variant="destructive">
          <AlertDescription>
            <p>{problem}</p>
          </AlertDescription>
        </Alert>
      )}
      <DataTable
        caption="Alert channels"
        columns={columns}
        rows={channels.data?.channels ?? []}
        loading={channels.isPending}
        getRowId={(channel) => String(channel.id)}
        empty={
          <EmptyState title="No channels" description="Add a channel to receive alerts." />
        }
        actions={(channel) => (
          <>
            <Button
              type="button"
              variant="outline"
              className={control}
              // One test at a time: the result is about one. A channel without a URL has nowhere to send.
              disabled={testing !== null || channel.url_host === ""}
              onClick={() => {
                void sendTest(channel);
              }}
            >
              {testing === channel.id ? "Sending test" : "Send test"}
            </Button>
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={askAbout("edit", channel)}
            >
              Edit
            </Button>
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={askAbout("rotate", channel)}
            >
              Rotate secret
            </Button>
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={askAbout("delete", channel)}
            >
              Delete
            </Button>
          </>
        )}
      />
      <VerifySignatures />
      <ChannelDialog
        open={adding || asking === "edit"}
        channel={target}
        create={create}
        update={update}
        onCancel={closeForm}
        onCreated={(secret) => {
          closeForm();
          created.show(secret);
        }}
        onUpdated={(changed) => {
          closeForm();
          if (changed) toast(DONE.update);
        }}
      />
      <ConfirmDialog
        open={asking === "rotate"}
        onOpenChange={closeQuestion(rotate.reset)}
        title={`Rotate the secret of ${target?.name ?? "this channel"}?`}
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
        title={`Delete ${target?.name ?? "this channel"}?`}
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
        title="Signing secret"
        description={SECRET_SHOWN_ONCE}
        secret={created.secret}
        onClose={created.clear}
      >
        <SecretNote />
      </SecretDialog>
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
