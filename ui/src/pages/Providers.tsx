import { Link } from "@tanstack/react-router";
import { useState } from "react";
import {
  useCreateProvider,
  useDeleteProvider,
  useProviders,
  useSyncProvider,
  useUpdateProvider,
} from "@/api/queries";
import { messageOfError } from "@/api/errors";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control, longText } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { useToast } from "@/components/toast";
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { syncText } from "@/lib/models";
import { kindName } from "@/lib/providers";
import { AddDialog } from "@/pages/ProvidersAdd";
import { EditDialog } from "@/pages/ProvidersEdit";

type Provider = components["schemas"]["ProviderView"];

export const DELETE_CONSEQUENCE = "Calls to models of this provider will fail at once.";

export const DONE = {
  update: "Provider updated.",
  delete: "Provider deleted.",
} as const;

const columns: Column<Provider>[] = [
  {
    id: "name",
    header: "Name",
    cell: (provider) => (
      <span className={`${longText} font-mono font-medium break-all md:max-w-64`}>
        {provider.name}
      </span>
    ),
    sortValue: (provider) => provider.name,
  },
  {
    id: "kind",
    header: "Kind",
    // A kind the console does not know is shown as it is.
    cell: (provider) => <Badge variant="outline">{kindName(provider.kind)}</Badge>,
    sortValue: (provider) => provider.kind,
  },
  {
    id: "base_url",
    header: "Base URL",
    cell: (provider) => (
      <span className={`${longText} break-all md:max-w-80`}>{provider.base_url}</span>
    ),
    sortValue: (provider) => provider.base_url,
  },
  {
    id: "has_credential",
    header: "Credential",
    // Whether there is one, in words. The credential itself the API never gives.
    cell: (provider) =>
      provider.has_credential ? (
        <Badge variant="secondary">Set</Badge>
      ) : (
        <Badge variant="outline">None</Badge>
      ),
    sortValue: (provider) => (provider.has_credential ? 1 : 0),
  },
];

type Asking = "add" | "edit" | "delete";

export function Providers() {
  const session = useSession();
  const providers = useProviders();
  const create = useCreateProvider();
  const update = useUpdateProvider();
  const remove = useDeleteProvider();
  const sync = useSyncProvider();
  const toast = useToast();
  const [asking, setAsking] = useState<Asking | null>(null);
  // Which provider the dialog is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Provider | null>(null);
  // The name of the provider that was added, for the notice that says how to call it.
  const [added, setAdded] = useState<string | null>(null);
  // What the last sync of a provider did, or why it did not work. The provider whose sync runs.
  const [synced, setSynced] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [syncing, setSyncing] = useState<number | null>(null);

  if (session.status !== "signedIn") return null;
  const mayManage = can(session.me, { type: "manageProviders" });

  function closing(reset: () => void) {
    return (open: boolean) => {
      if (open) return;
      setAsking(null);
      reset();
    };
  }

  function askAbout(what: "edit" | "delete", provider: Provider) {
    return () => {
      setProblem(null);
      setTarget(provider);
      setAsking(what);
    };
  }

  async function syncModels(provider: Provider) {
    setSynced(null);
    setProblem(null);
    setSyncing(provider.id);
    try {
      setSynced(syncText(await sync.mutateAsync({ id: provider.id })));
    } catch (error) {
      // A session that is over says nothing; the clean-up of the session leaves the page.
      setProblem(messageOfError(error));
    } finally {
      setSyncing(null);
    }
  }

  const addButton = mayManage ? (
    <Button
      type="button"
      className={control}
      onClick={() => {
        // Another action starts: the problem of the last sync is stale.
        setProblem(null);
        setAsking("add");
      }}
    >
      Add provider
    </Button>
  ) : undefined;

  const rowActions = mayManage
    ? (provider: Provider) => (
        <>
          {/* The gateway cannot read the models of an Azure OpenAI provider: they are added by name. */}
          {provider.kind === "azure" ? null : (
            <Button
              type="button"
              variant="outline"
              className={control}
              // One sync at a time: the notices are about one.
              disabled={syncing !== null}
              onClick={() => {
                void syncModels(provider);
              }}
            >
              {syncing === provider.id ? "Syncing" : "Sync models"}
            </Button>
          )}
          <Button
            type="button"
            variant="outline"
            className={control}
            onClick={askAbout("edit", provider)}
          >
            Edit
          </Button>
          <Button
            type="button"
            variant="outline"
            className={control}
            onClick={askAbout("delete", provider)}
          >
            Delete
          </Button>
        </>
      )
    : undefined;

  const failed = providers.error !== null && providers.data === undefined;
  return (
    <>
      {failed ? null : (
        // One child of the page: the place of the notice takes no room while it is empty.
        <div className="flex flex-col">
          <PageHeader title="Providers" actions={addButton} />
          {/*
            Where the page says how a provider that was added is called. The
            place is there before the text: what comes into a live region is
            announced, a live region that comes with its text often is not.
          */}
          <div role="status" className="not-empty:mt-6">
            {added === null ? null : (
              // The announcement is that of the place: the alert is none of its own.
              <Alert role="presentation">
                <AlertTitle>Provider added</AlertTitle>
                <AlertDescription>
                  <p>
                    Call its models as{" "}
                    <code className="font-mono break-words">{`${added}/<model>`}</code>: the name
                    of the provider, a slash, and the name of the model.
                  </p>
                  <p>
                    Enable its models on the{" "}
                    <Link
                      to="/models"
                      className="rounded-sm underline underline-offset-4 outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
                    >
                      Models page
                    </Link>{" "}
                    first.
                  </p>
                </AlertDescription>
                <AlertAction>
                  <Button
                    type="button"
                    variant="outline"
                    className={control}
                    onClick={() => {
                      setAdded(null);
                    }}
                  >
                    Dismiss
                  </Button>
                </AlertAction>
              </Alert>
            )}
            {synced === null ? null : (
              <Alert role="presentation" className={added === null ? "" : "mt-4"}>
                <AlertTitle>Sync done</AlertTitle>
                <AlertDescription>
                  <p>{synced}</p>
                </AlertDescription>
                <AlertAction>
                  <Button
                    type="button"
                    variant="outline"
                    className={control}
                    onClick={() => {
                      setSynced(null);
                    }}
                  >
                    Dismiss
                  </Button>
                </AlertAction>
              </Alert>
            )}
          </div>
          {problem === null ? null : (
            <Alert variant="destructive" className="mt-6">
              <AlertDescription>
                <p>{problem}</p>
              </AlertDescription>
            </Alert>
          )}
        </div>
      )}
      {failed ? (
        <QueryProblem
          title="Providers"
          error={providers.error}
          onRetry={() => {
            void providers.refetch();
          }}
        />
      ) : (
        <DataTable
          caption="Providers"
          columns={columns}
          rows={providers.data?.providers ?? []}
          loading={providers.isPending}
          getRowId={(provider) => String(provider.id)}
          empty={<EmptyState title="No providers" description="No provider has been added yet." />}
          {...(rowActions === undefined ? {} : { actions: rowActions })}
        />
      )}
      {mayManage ? (
        <>
          <AddDialog
            open={asking === "add"}
            create={create}
            onCancel={() => {
              closing(create.reset)(false);
            }}
            onAdded={(name) => {
              closing(create.reset)(false);
              setAdded(name);
            }}
          />
          <EditDialog
            open={asking === "edit"}
            provider={target}
            update={update}
            onCancel={() => {
              closing(update.reset)(false);
            }}
            onDone={() => {
              closing(update.reset)(false);
              toast(DONE.update);
            }}
          />
          <ConfirmDialog
            open={asking === "delete"}
            onOpenChange={closing(remove.reset)}
            title={`Delete ${target?.name ?? "this provider"}?`}
            body={DELETE_CONSEQUENCE}
            confirmLabel="Delete"
            tone="danger"
            onConfirm={async () => {
              if (target === null) return;
              await remove.mutateAsync({ id: target.id });
              // The notice about a provider that is gone would say what is not so.
              if (target.name === added) setAdded(null);
              toast(DONE.delete);
            }}
          />
        </>
      ) : null}
    </>
  );
}
