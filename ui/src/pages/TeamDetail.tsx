import { Link, useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import {
  useDeleteTeam,
  usePutTeamMember,
  useRemoveTeamMember,
  useRenameTeam,
  useTeam,
} from "@/api/queries";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { NotFoundContent } from "@/components/NotFoundContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { TeamRoleBadge } from "@/components/RoleBadge";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { YouBadge } from "@/components/YouBadge";
import { idOf } from "@/lib/id";
import { AddDialog } from "@/pages/TeamDetailAddMember";
import { TeamNameDialog } from "@/pages/Teams";

type Team = components["schemas"]["TeamSummary"];
type Member = components["schemas"]["MemberDetail"];

export const CONSEQUENCES = {
  delete: "Keys that belong to this team keep working and lose their team.",
  remove: "They will lose access to this team.",
  leave: "You will lose access to this team.",
  /** For who removes themselves and sees the team all the same. */
  removeSelf:
    "You will no longer be a member of this team. As an admin you can still see and manage this team.",
  lead: "They can rename the team, add and remove its members, and manage its keys.",
  member: "They can no longer manage the team.",
  ownRole: "Your role in this team changes. You stay signed in.",
} as const;

export const DONE = {
  rename: "Team renamed.",
  delete: "Team deleted.",
  add: "Member added.",
  role: "Role changed.",
  remove: "Member removed.",
  leave: "You left the team.",
} as const;

type Asking = "rename" | "add" | "delete" | "role" | "remove";

const memberColumns = (ownId: number): Column<Member>[] => [
  {
    id: "name",
    header: "Name",
    cell: (member) => (
      <span className="inline-flex flex-wrap items-center gap-2">
        {member.name}
        {member.user_id === ownId ? <YouBadge /> : null}
      </span>
    ),
    sortValue: (member) => member.name,
  },
  {
    id: "email",
    header: "Email",
    cell: (member) => member.email,
    sortValue: (member) => member.email,
  },
  {
    id: "role",
    header: "Role",
    cell: (member) => <TeamRoleBadge role={member.role} />,
    sortValue: (member) => member.role,
  },
];

function Team({ me, team, members }: { me: Me; team: Team; members: readonly Member[] }) {
  const navigate = useNavigate();
  const toast = useToast();
  const rename = useRenameTeam();
  const remove = useDeleteTeam();
  const put = usePutTeamMember();
  const removeMember = useRemoveTeamMember();
  const [asking, setAsking] = useState<Asking | null>(null);
  // Who the question is about. Kept while the dialog closes.
  const [member, setMember] = useState<Member | null>(null);

  const { id } = team;
  const ownId = me.user.id;
  const mayRename = can(me, { type: "renameTeam", teamId: id });
  const mayDelete = can(me, { type: "deleteTeam", teamId: id });
  const mayAdd = can(me, { type: "addMember", teamId: id });
  const mayChangeRoles = can(me, { type: "makeLead", teamId: id });
  const mayRemove = can(me, { type: "removeMember", teamId: id });
  // Only who may read the list of all users is given it to choose from.
  const mayChoose = can(me, { type: "inviteUser" });
  const columns = useMemo(() => memberColumns(ownId), [ownId]);

  const own = member !== null && member.user_id === ownId;
  // Without the membership, would the team still be theirs to see?
  const losesAccess =
    own &&
    !can(
      { user: me.user, teams: me.teams.filter((one) => one.team_id !== id) },
      { type: "viewTeam", teamId: id },
    );
  const newRole = member?.role === "lead" ? "member" : "lead";
  const who = member?.name ?? "";

  function closing(reset: () => void) {
    return (open: boolean) => {
      if (open) return;
      setAsking(null);
      reset();
    };
  }

  function askAbout(what: "role" | "remove", about: Member) {
    return () => {
      setMember(about);
      setAsking(what);
    };
  }

  const rowActions =
    mayChangeRoles || mayRemove
      ? (row: Member) => (
          <>
            {mayChangeRoles ? (
              <Button
                type="button"
                variant="outline"
                className={control}
                onClick={askAbout("role", row)}
              >
                {row.role === "lead" ? "Make member" : "Make lead"}
              </Button>
            ) : null}
            {mayRemove ? (
              <Button
                type="button"
                variant="outline"
                className={control}
                onClick={askAbout("remove", row)}
              >
                Remove
              </Button>
            ) : null}
          </>
        )
      : undefined;

  return (
    <>
      {mayRename || mayAdd || mayDelete ? (
        <div role="group" aria-label="Actions" className="flex flex-wrap gap-2">
          {mayRename ? (
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                setAsking("rename");
              }}
            >
              Rename
            </Button>
          ) : null}
          {mayAdd ? (
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                setAsking("add");
              }}
            >
              Add member
            </Button>
          ) : null}
          {mayDelete ? (
            <Button
              type="button"
              variant="destructive"
              className={control}
              onClick={() => {
                setAsking("delete");
              }}
            >
              Delete
            </Button>
          ) : null}
        </div>
      ) : null}

      <h2 className="text-lg font-medium">Members</h2>
      <DataTable
        caption="Members"
        columns={columns}
        rows={members}
        getRowId={(row) => String(row.user_id)}
        empty={<EmptyState title="No members" description="Nobody is in this team." />}
        {...(rowActions === undefined ? {} : { actions: rowActions })}
      />

      {mayRename ? (
        <TeamNameDialog
          open={asking === "rename"}
          title="Rename team"
          description="The name is shown on the keys that belong to the team."
          name={team.name}
          submit="Save"
          submitting="Saving"
          pending={rename.isPending}
          send={(name) => rename.mutateAsync({ id, body: { name } })}
          onCancel={() => {
            closing(rename.reset)(false);
          }}
          onDone={() => {
            closing(rename.reset)(false);
            toast(DONE.rename);
          }}
        />
      ) : null}
      {mayAdd ? (
        <AddDialog
          open={asking === "add"}
          team={team}
          members={members}
          fromList={mayChoose}
          put={put}
          onCancel={() => {
            closing(put.reset)(false);
          }}
          onDone={() => {
            closing(put.reset)(false);
            toast(DONE.add);
          }}
        />
      ) : null}
      {mayChangeRoles ? (
        <ConfirmDialog
          open={asking === "role"}
          onOpenChange={closing(put.reset)}
          title={`Make ${own ? "yourself" : who} a ${newRole}?`}
          body={own ? CONSEQUENCES.ownRole : CONSEQUENCES[newRole]}
          confirmLabel={newRole === "lead" ? "Make lead" : "Make member"}
          onConfirm={async () => {
            if (member === null) return;
            await put.mutateAsync({ id, userId: member.user_id, body: { role: newRole } });
            toast(DONE.role);
          }}
        />
      ) : null}
      {mayRemove ? (
        <ConfirmDialog
          open={asking === "remove"}
          onOpenChange={closing(removeMember.reset)}
          title={`Remove ${who}?`}
          body={
            losesAccess ? CONSEQUENCES.leave : own ? CONSEQUENCES.removeSelf : CONSEQUENCES.remove
          }
          confirmLabel="Remove"
          tone="danger"
          onConfirm={async () => {
            if (member === null) return;
            await removeMember.mutateAsync({ id, userId: member.user_id, leaving: losesAccess });
            if (losesAccess) {
              toast(DONE.leave);
              await navigate({ to: "/teams" });
            } else {
              toast(DONE.remove);
            }
          }}
        />
      ) : null}
      {mayDelete ? (
        <ConfirmDialog
          open={asking === "delete"}
          onOpenChange={closing(remove.reset)}
          title="Delete this team?"
          body={CONSEQUENCES.delete}
          confirmLabel="Delete"
          tone="danger"
          onConfirm={async () => {
            await remove.mutateAsync({ id });
            toast(DONE.delete);
            await navigate({ to: "/teams" });
          }}
        />
      ) : null}
    </>
  );
}

function Loading() {
  return (
    <div role="status" aria-busy="true" aria-label="Loading the team" className="flex flex-col gap-4">
      <Skeleton className="h-8 w-48" />
      <Skeleton className="h-4 w-full max-w-md" />
      <Skeleton className="h-4 w-full max-w-md" />
      <Skeleton className="h-4 w-full max-w-md" />
    </div>
  );
}

function Details({ id }: { id: number }) {
  const session = useSession();
  const team = useTeam(id);

  if (session.status !== "signedIn") return null;
  if (team.data === undefined) {
    if (team.error !== null) {
      return (
        <QueryProblem
          notFound
          title="Team"
          error={team.error}
          onRetry={() => {
            void team.refetch();
          }}
        />
      );
    }
    return <Loading />;
  }

  const shown = team.data;
  return (
    <>
      <Link
        to="/teams"
        className="inline-flex min-h-11 w-fit items-center rounded-sm text-sm text-muted-foreground underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-8"
      >
        Back to teams
      </Link>
      <PageHeader title={shown.team.name} />
      <dl
        aria-label="Details"
        className="grid max-w-md grid-cols-[auto_minmax(0,1fr)] gap-x-6 gap-y-3 text-sm"
      >
        <dt className="text-muted-foreground">Created</dt>
        <dd>
          <Timestamp value={shown.team.created_at} />
        </dd>
      </dl>
      <Team me={session.me} team={shown.team} members={shown.members} />
    </>
  );
}

/** The page of one team. `id` is the id as the address has it. */
export function TeamDetail({ id }: { id: string }) {
  const number = idOf(id);
  // Not an id: the API is not asked.
  if (number === null) return <NotFoundContent />;
  return <Details id={number} />;
}
