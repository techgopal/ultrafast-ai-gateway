import { useId, useMemo, useState } from "react";
import { useCreateKey, useKeys, useRevokeKey } from "@/api/queries";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control, longText } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { NO_TEAM } from "@/lib/keys";
import { CreateDialog } from "@/pages/KeysCreate";
import {
  ANY,
  chosen,
  FilterSelect,
  matches,
  statusChoices,
  teamChoices,
} from "@/pages/KeysFilters";

type Key = components["schemas"]["KeyView"];

export const NEW_KEY_TITLE = "Your new key";
export const NEW_KEY_DESCRIPTION = "Copy this key now. It is not shown again.";
export const SUSPENDED_HINT = "The owner is not active";
export const REVOKE_CONSEQUENCE = "Apps using this key stop working at once. This cannot be undone.";
export const KEY_REVOKED = "Key revoked.";

const NO_OWNER = "No owner";

/** How a new key is used. It shows a placeholder, never the key. */
function KeyExample() {
  return (
    <div role="group" aria-label="How to use the key" className="flex flex-col gap-2 text-sm">
      <p className="text-muted-foreground">
        Apps call this gateway at the base URL, and send the key in the header.
      </p>
      <dl className="flex flex-col gap-1">
        <dt className="text-muted-foreground">Base URL</dt>
        <dd className="mb-1">
          <code className="font-mono break-words">{`${window.location.origin}/v1`}</code>
        </dd>
        <dt className="text-muted-foreground">Header</dt>
        <dd>
          <code className="font-mono break-words">{"Authorization: Bearer <key>"}</code>
        </dd>
      </dl>
    </div>
  );
}

// ---------------------------------------------------------------- the page

const columns: Column<Key>[] = [
  {
    id: "name",
    header: "Name",
    cell: (key) => <span className={`${longText} font-medium break-words md:max-w-64`}>{key.name}</span>,
    sortValue: (key) => key.name,
  },
  {
    id: "display",
    header: "Key",
    cell: (key) => <span className="font-mono">{key.display}</span>,
  },
  {
    id: "owner",
    header: "Owner",
    cell: (key) =>
      key.owner_email === null ? (
        <span className="text-muted-foreground">{NO_OWNER}</span>
      ) : (
        <span className={`${longText} break-all md:max-w-64`}>{key.owner_email}</span>
      ),
    sortValue: (key) => key.owner_email,
  },
  {
    id: "team",
    header: "Team",
    cell: (key) =>
      key.team_name === null ? (
        <span className="text-muted-foreground">{NO_TEAM}</span>
      ) : (
        <span className={`${longText} break-words md:max-w-64`}>{key.team_name}</span>
      ),
    sortValue: (key) => key.team_name,
  },
  {
    id: "expires_at",
    header: "Expires",
    cell: (key) => <Timestamp value={key.expires_at} />,
    sortValue: (key) => key.expires_at,
  },
  {
    id: "status",
    header: "Status",
    cell: (key) => (
      <span className="inline-flex flex-col items-start gap-1">
        <StatusBadge status={key.status} />
        {key.status === "suspended" ? (
          <span className="text-xs text-muted-foreground">{SUSPENDED_HINT}</span>
        ) : null}
      </span>
    ),
    sortValue: (key) => key.status,
  },
];

function KeyList({ me }: { me: Me }) {
  const keys = useKeys();
  const create = useCreateKey();
  const once = useSecretOnce(create);
  const revoke = useRevokeKey();
  const toast = useToast();
  const showRevokedId = useId();
  const [creating, setCreating] = useState(false);
  // Which key the question is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Key | null>(null);
  const [asking, setAsking] = useState(false);
  const [search, setSearch] = useState("");
  const [teamChosen, setTeam] = useState(ANY);
  const [statusChosen, setStatus] = useState(ANY);
  const [showRevoked, setShowRevoked] = useState(false);

  const all = useMemo(() => keys.data?.keys ?? [], [keys.data]);
  const teams = useMemo(() => teamChoices(all), [all]);
  const statuses = useMemo(() => statusChoices(all, showRevoked), [all, showRevoked]);
  // A choice that is gone, as "revoked" when the revoked keys are hidden, filters nothing.
  const team = chosen(teamChosen, teams);
  const status = chosen(statusChosen, statuses);
  const rows = useMemo(
    () => all.filter((key) => matches(key, { search, team, status, showRevoked })),
    [all, search, team, status, showRevoked],
  );

  const mayCreate = can(me, { type: "createKeyForSelf", teamId: null });
  function mayRevoke(key: Key): boolean {
    // What is revoked stays revoked: the gateway would change nothing.
    return (
      key.status !== "revoked" &&
      can(me, { type: "revokeKey", ownerId: key.owner_id, teamId: key.team_id })
    );
  }
  const rowActions = all.some(mayRevoke)
    ? (key: Key) =>
        mayRevoke(key) ? (
          <Button
            type="button"
            variant="outline"
            className={control}
            onClick={() => {
              setTarget(key);
              setAsking(true);
            }}
          >
            Revoke
          </Button>
        ) : null
    : undefined;

  function closeCreate() {
    setCreating(false);
    create.reset();
  }

  const createButton = mayCreate ? (
    <Button
      type="button"
      className={control}
      onClick={() => {
        setCreating(true);
      }}
    >
      Create key
    </Button>
  ) : undefined;

  const failed = keys.error !== null && keys.data === undefined;
  const empty =
    all.length === 0 ? (
      <EmptyState title="No virtual keys" description="There is no key for you to see." />
    ) : (
      <EmptyState
        title="No keys match"
        description={
          showRevoked
            ? "Change the filters to see more keys."
            : "Change the filters, or turn on Show revoked."
        }
      />
    );

  return (
    <>
      {failed ? null : <PageHeader title="Virtual keys" actions={createButton} />}
      {failed ? (
        <QueryProblem
          title="Virtual keys"
          error={keys.error}
          onRetry={() => {
            void keys.refetch();
          }}
        />
      ) : (
        <>
          {keys.isPending || all.length > 0 ? (
            <div role="group" aria-label="Filters" className="flex flex-wrap items-center gap-2">
              <Input
                type="search"
                aria-label="Search"
                placeholder="Search name, owner or key"
                autoComplete="off"
                className={`${control} w-full sm:w-64`}
                value={search}
                onChange={(event) => {
                  setSearch(event.target.value);
                }}
              />
              <FilterSelect label="Team" value={team} choices={teams} onChange={setTeam} />
              <FilterSelect label="Status" value={status} choices={statuses} onChange={setStatus} />
              <div className="flex min-h-11 items-center gap-2 md:min-h-8">
                <Checkbox
                  id={showRevokedId}
                  checked={showRevoked}
                  onCheckedChange={(checked) => {
                    setShowRevoked(checked === true);
                  }}
                />
                <Label htmlFor={showRevokedId}>Show revoked</Label>
              </div>
            </div>
          ) : null}
          <DataTable
            caption="Virtual keys"
            columns={columns}
            rows={rows}
            loading={keys.isPending}
            getRowId={(key) => String(key.id)}
            empty={empty}
            {...(rowActions === undefined ? {} : { actions: rowActions })}
          />
        </>
      )}
      {mayCreate ? (
        <>
          <CreateDialog
            open={creating}
            me={me}
            create={create}
            onCancel={closeCreate}
            onCreated={(secret) => {
              setCreating(false);
              // Shows the key, and makes the mutation forget its answer.
              once.show(secret);
            }}
          />
          <SecretDialog
            title={NEW_KEY_TITLE}
            description={NEW_KEY_DESCRIPTION}
            secret={once.secret}
            onClose={once.clear}
          >
            <KeyExample />
          </SecretDialog>
        </>
      ) : null}
      <ConfirmDialog
        open={asking}
        onOpenChange={(open) => {
          if (open) return;
          setAsking(false);
          revoke.reset();
        }}
        title={`Revoke ${target?.name ?? "this key"}?`}
        body={REVOKE_CONSEQUENCE}
        confirmLabel="Revoke"
        tone="danger"
        onConfirm={async () => {
          if (target === null) return;
          await revoke.mutateAsync({ id: target.id });
          toast(KEY_REVOKED);
        }}
      />
    </>
  );
}

/** The virtual keys the signed-in user may see. */
export function Keys() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  return <KeyList me={session.me} />;
}
