import { useMemo, useState } from "react";
import {
  useCreateModel,
  useDeleteModel,
  useModels,
  usePutModelGrants,
  useSyncProvider,
  useUpdateModel,
} from "@/api/queries";
import { messageOfError } from "@/api/errors";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control, longText } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { FilterSelect } from "@/components/FilterSelect";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { useToast } from "@/components/toast";
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  accessSummary,
  ANY,
  chosen,
  hasNoAccess,
  matches,
  priceText,
  providerChoices,
  refOf,
  sortModels,
  statusChoices,
  syncText,
} from "@/lib/models";
import { AccessDialog } from "@/pages/ModelsAccess";
import { AddDialog, SyncDialog } from "@/pages/ModelsAdd";
import { PriceDialog } from "@/pages/ModelsPrice";

type Model = components["schemas"]["ModelView"];

export const DELETE_CONSEQUENCE =
  "Routes that use this model lose this target. Calls to it fail at once.";
export const NO_ACCESS_HINT = "Enabled, but nobody has access yet.";
export const EMPTY_FOR_MEMBER = "No models are available to you yet. Ask an admin.";

export const DONE = {
  add: "Model added.",
  access: "Access updated.",
  price: "Price saved.",
  delete: "Model deleted.",
  copied: "Copied.",
  notCopied: "Could not copy. Select the name and copy it by hand.",
} as const;

type Asking = "sync" | "add" | "access" | "price" | "delete";

function AdminModels() {
  const models = useModels();
  const create = useCreateModel();
  const update = useUpdateModel();
  const grants = usePutModelGrants();
  const remove = useDeleteModel();
  const sync = useSyncProvider();
  const toast = useToast();
  const [asking, setAsking] = useState<Asking | null>(null);
  // Which model the dialog is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Model | null>(null);
  const [synced, setSynced] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [providerChosen, setProvider] = useState(ANY);
  const [statusChosen, setStatus] = useState(ANY);

  const all = useMemo(() => models.data?.models ?? [], [models.data]);
  const providers = useMemo(() => providerChoices(all), [all]);
  const statuses = useMemo(() => statusChoices(), []);
  // A choice that is gone filters nothing.
  const provider = chosen(providerChosen, providers);
  const status = chosen(statusChosen, statuses);
  const rows = useMemo(
    () => sortModels(all).filter((model) => matches(model, { search, provider, status })),
    [all, search, provider, status],
  );
  // Every model whose change is on its way: each row's switch waits for its own.
  const [changing, setChanging] = useState<ReadonlySet<number>>(new Set());

  function settled(id: number) {
    setChanging((now) => {
      const next = new Set(now);
      next.delete(id);
      return next;
    });
  }

  async function toggle(model: Model, enabled: boolean) {
    setProblem(null);
    setChanging((now) => new Set(now).add(model.id));
    try {
      await update.mutateAsync({ id: model.id, body: { enabled } });
    } catch (error) {
      // A session that is over says nothing; the clean-up of the session leaves the page.
      setProblem(messageOfError(error));
    } finally {
      settled(model.id);
    }
  }

  function closing(reset: () => void) {
    setAsking(null);
    reset();
  }

  function askAbout(what: "access" | "price" | "delete", model: Model) {
    return () => {
      setTarget(model);
      setAsking(what);
    };
  }

  const columns: Column<Model>[] = [
    {
      id: "name",
      header: "Name",
      cell: (model) => (
        <span className={`${longText} font-mono font-medium break-all md:max-w-64`}>
          {model.name}
        </span>
      ),
      sortValue: (model) => model.name,
    },
    {
      id: "provider",
      header: "Provider",
      cell: (model) => (
        <span className={`${longText} font-mono break-all md:max-w-48`}>{model.provider_name}</span>
      ),
      sortValue: (model) => model.provider_name,
    },
    {
      id: "status",
      header: "Status",
      cell: (model) => {
        const text = model.enabled ? "Enabled" : "Disabled";
        return (
          <Label className={`${control} gap-2`}>
            <Switch
              aria-label={model.name}
              checked={model.enabled}
              disabled={changing.has(model.id)}
              onCheckedChange={(on) => {
                void toggle(model, on);
              }}
            />
            <span>{text}</span>
          </Label>
        );
      },
      sortValue: (model) => (model.enabled ? 1 : 0),
    },
    {
      id: "access",
      header: "Access",
      cell: (model) => (
        <span className="inline-flex flex-col items-start gap-1">
          <span>{accessSummary(model.grants)}</span>
          {model.enabled && hasNoAccess(model.grants) ? (
            <span className="text-xs text-muted-foreground">{NO_ACCESS_HINT}</span>
          ) : null}
        </span>
      ),
    },
    {
      id: "input_price",
      header: "Input $/1M",
      cell: (model) => <span className="tabular-nums">{priceText(model.input_price_micros)}</span>,
      sortValue: (model) => model.input_price_micros,
    },
    {
      id: "output_price",
      header: "Output $/1M",
      cell: (model) => <span className="tabular-nums">{priceText(model.output_price_micros)}</span>,
      sortValue: (model) => model.output_price_micros,
    },
  ];

  const failed = models.error !== null && models.data === undefined;
  const empty =
    all.length === 0 ? (
      <EmptyState
        title="No models"
        description="Sync the models of a provider, or add one by name."
      />
    ) : (
      <EmptyState title="No models match" description="Change the filters to see more models." />
    );

  const headerActions = (
    <>
      <Button
        type="button"
        variant="outline"
        className={control}
        onClick={() => {
          setAsking("sync");
        }}
      >
        Sync models
      </Button>
      <Button
        type="button"
        className={control}
        onClick={() => {
          setAsking("add");
        }}
      >
        Add model
      </Button>
    </>
  );

  return (
    <>
      {failed ? null : (
        // One child of the page: the place of the notice takes no room while it is empty.
        <div className="flex flex-col">
          <PageHeader title="Models" actions={headerActions} />
          <div role="status" className="not-empty:mt-6">
            {synced === null ? null : (
              // The announcement is that of the place: the alert is none of its own.
              <Alert role="presentation">
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
          title="Models"
          error={models.error}
          onRetry={() => {
            void models.refetch();
          }}
        />
      ) : (
        <>
          {models.isPending || all.length > 0 ? (
            <div role="group" aria-label="Filters" className="flex flex-wrap items-center gap-2">
              <Input
                type="search"
                aria-label="Search"
                placeholder="Search name or provider"
                autoComplete="off"
                className={`${control} w-full sm:w-64`}
                value={search}
                onChange={(event) => {
                  setSearch(event.target.value);
                }}
              />
              <FilterSelect
                label="Provider"
                value={provider}
                choices={providers}
                onChange={setProvider}
              />
              <FilterSelect label="Status" value={status} choices={statuses} onChange={setStatus} />
            </div>
          ) : null}
          <DataTable
            caption="Models"
            columns={columns}
            rows={rows}
            loading={models.isPending}
            getRowId={(model) => String(model.id)}
            empty={empty}
            actions={(model) => (
              <>
                <Button
                  type="button"
                  variant="outline"
                  className={control}
                  onClick={askAbout("access", model)}
                >
                  Edit access
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  className={control}
                  onClick={askAbout("price", model)}
                >
                  Edit price
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  className={control}
                  onClick={askAbout("delete", model)}
                >
                  Delete
                </Button>
              </>
            )}
          />
        </>
      )}
      <SyncDialog
        open={asking === "sync"}
        sync={sync}
        onCancel={() => {
          closing(sync.reset);
        }}
        onSynced={(result) => {
          closing(sync.reset);
          setSynced(syncText(result));
        }}
      />
      <AddDialog
        open={asking === "add"}
        create={create}
        onCancel={() => {
          closing(create.reset);
        }}
        onAdded={() => {
          closing(create.reset);
          toast(DONE.add);
        }}
      />
      <AccessDialog
        open={asking === "access"}
        model={target}
        put={grants}
        onCancel={() => {
          closing(grants.reset);
        }}
        onDone={() => {
          closing(grants.reset);
          toast(DONE.access);
        }}
      />
      <PriceDialog
        open={asking === "price"}
        model={target}
        update={update}
        onCancel={() => {
          closing(update.reset);
        }}
        onDone={() => {
          closing(update.reset);
          toast(DONE.price);
        }}
      />
      <ConfirmDialog
        open={asking === "delete"}
        onOpenChange={(open) => {
          if (!open) closing(remove.reset);
        }}
        title={`Delete ${target?.name ?? "this model"}?`}
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

function MemberModels() {
  const models = useModels();
  const toast = useToast();
  const failed = models.error !== null && models.data === undefined;

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast(DONE.copied);
    } catch {
      toast(DONE.notCopied, "error");
    }
  }

  if (failed) {
    return (
      <QueryProblem
        title="Models"
        error={models.error}
        onRetry={() => {
          void models.refetch();
        }}
      />
    );
  }
  const list = sortModels(models.data?.models ?? []);
  return (
    <>
      <PageHeader title="Models" />
      {models.isPending ? null : list.length === 0 ? (
        <EmptyState title="No models" description={EMPTY_FOR_MEMBER} />
      ) : (
        <section className="flex flex-col gap-3">
          <h2 id="usable-models" className="text-base font-medium">
            Models you can use
          </h2>
          <ul aria-labelledby="usable-models" className="flex flex-col gap-2">
            {list.map((model) => (
              <li
                key={model.id}
                className="flex items-center justify-between gap-3 rounded-lg border bg-card p-3"
              >
                <span className="flex min-w-0 flex-col gap-1">
                  <span className="font-mono break-all">{refOf(model)}</span>
                  <span className="text-xs text-muted-foreground">
                    Input {priceText(model.input_price_micros)} · Output{" "}
                    {priceText(model.output_price_micros)} per 1M tokens
                  </span>
                </span>
                <Button
                  type="button"
                  variant="outline"
                  className={control}
                  aria-label={`Copy ${refOf(model)}`}
                  onClick={() => {
                    void copy(refOf(model));
                  }}
                >
                  Copy
                </Button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </>
  );
}

function ModelsFor({ me }: { me: Me }) {
  return can(me, { type: "manageModels" }) ? <AdminModels /> : <MemberModels />;
}

/** The catalog of models: managed by an admin, a read-only list for everybody else. */
export function Models() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  return <ModelsFor me={session.me} />;
}
