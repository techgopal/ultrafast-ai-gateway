import { useForm } from "@tanstack/react-form";
import { Link, useNavigate } from "@tanstack/react-router";
import { useMemo, useRef, useState } from "react";
import { ApiError, ConsoleRefusal } from "@/api/errors";
import {
  useDeleteTeam,
  usePutTeamMember,
  useRemoveTeamMember,
  useRenameTeam,
  useTeam,
  useUsers,
} from "@/api/queries";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { dialogButton, dialogFit, useReturnFocus } from "@/components/dialog-fit";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { Field, type FieldWiring } from "@/components/Field";
import { applyApiError, onField, useFormFailure } from "@/components/form";
import { FormError } from "@/components/FormError";
import { NotFoundContent } from "@/components/NotFoundContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Skeleton } from "@/components/ui/skeleton";
import { TeamNameDialog } from "@/pages/Teams";
import { idOf } from "@/pages/UserDetail";
import { YouBadge } from "@/pages/Users";

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

export const NO_USER_WITH_ID = "No user with that ID.";
export const USER_ID_HINT = "Ask an admin for the user's ID.";
export const CHOOSE_A_USER = "Choose a user.";

const TEAM_ROLE_NAMES: Record<string, string> = { lead: "Lead", member: "Member" };

/** A role in a team, in a neutral badge. One the console does not know is shown as it is. */
function TeamRoleBadge({ role }: { role: string }) {
  return <Badge variant="outline">{TEAM_ROLE_NAMES[role] ?? role}</Badge>;
}

const control = "min-h-11 md:min-h-8";

/** What the console says about the one field of the form. It is no answer of the gateway. */
function onUserId(message: string): ConsoleRefusal {
  return new ConsoleRefusal(message, "user_id");
}

/** The refusals of the gateway that are about the user who is added. */
function aboutTheUser(error: unknown): unknown {
  // The gateway says "not found"; the form says what was not found. It may be
  // the team as well: `usePutTeamMember` asks for the team again, and when it
  // is gone the page shows that in place of this form.
  if (error instanceof ApiError && error.status === 404) return onUserId(NO_USER_WITH_ID);
  return onField(error, "user_disabled", "user_id");
}

type User = components["schemas"]["UserView"];

/** The list the user is chosen from, for who may read it. */
interface Choice {
  /** Who can be added; `null` while they are not known. */
  candidates: readonly User[] | null;
  /** Why they are not known. */
  error: unknown;
  retry: () => void;
}

interface AddFormProps {
  team: Team;
  members: readonly Member[];
  put: ReturnType<typeof usePutTeamMember>;
  onDone: () => void;
  onCancel: () => void;
}

function Candidates({
  choice,
  wiring,
  value,
  onChange,
}: {
  choice: Choice;
  wiring: FieldWiring;
  value: string;
  onChange: (value: string) => void;
}) {
  const { candidates } = choice;
  if (candidates === null) {
    if (choice.error !== null) return <ErrorState error={choice.error} onRetry={choice.retry} />;
    return (
      <div role="status" aria-busy="true" aria-label="Loading the users" className="flex flex-col gap-2">
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-full" />
      </div>
    );
  }
  if (candidates.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        Every user who can be added is in this team already.
      </p>
    );
  }
  const { id, name, ...described } = wiring;
  return (
    <RadioGroup
      {...described}
      id={id}
      name={name}
      aria-label="User"
      value={value}
      onValueChange={onChange}
    >
      {candidates.map((user) => (
        <div key={user.id} className="flex min-h-11 items-center gap-2">
          <RadioGroupItem id={`${id}-${String(user.id)}`} value={String(user.id)} />
          <Label htmlFor={`${id}-${String(user.id)}`} className="min-w-0 flex-wrap gap-x-2">
            <span>{user.name}</span>
            <span className="font-normal break-all text-muted-foreground">{user.email}</span>
          </Label>
        </div>
      ))}
    </RadioGroup>
  );
}

/**
 * The form of who chooses from the list of users: not who is in the team,
 * and not who is disabled. The list is asked for when the dialog opens.
 */
function AddFromListForm(props: AddFormProps) {
  const users = useUsers();
  const { members } = props;
  const candidates = useMemo(() => {
    if (users.data === undefined) return null;
    const inTeam = new Set(members.map((member) => member.user_id));
    return users.data.users.filter((user) => !inTeam.has(user.id) && user.status !== "disabled");
  }, [users.data, members]);
  return (
    <AddForm
      {...props}
      choice={{
        candidates,
        error: users.error,
        retry: () => {
          void users.refetch();
        },
      }}
    />
  );
}

// Mounted while the dialog is open: every opening starts with an empty form.
function AddForm({ team, put, onDone, onCancel, choice }: AddFormProps & { choice?: Choice }) {
  const { mutateAsync } = put;
  const fromList = choice !== undefined;
  const anybody = choice === undefined || (choice.candidates ?? []).length > 0;
  const form = useForm({
    defaultValues: { user_id: "" },
    onSubmit: async ({ value }) => {
      try {
        const userId = idOf(value.user_id.trim());
        // Nothing is sent for what is no id: the console refuses it itself.
        if (userId === null) throw onUserId(fromList ? CHOOSE_A_USER : NO_USER_WITH_ID);
        await mutateAsync({ id: team.id, userId, body: { role: "member" } });
        onDone();
      } catch (error) {
        applyApiError(form, aboutTheUser(error));
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);

  return (
    <form
      ref={formRef}
      aria-label="Add member"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        void form.handleSubmit();
      }}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="user_id">
        {(field) =>
          choice !== undefined ? (
            <Field label="User" name={field.name} error={failure.fieldError(field.name)}>
              {(wiring) => (
                <Candidates
                  choice={choice}
                  wiring={wiring}
                  value={field.state.value}
                  onChange={field.handleChange}
                />
              )}
            </Field>
          ) : (
            <Field
              label="User ID"
              name={field.name}
              required
              hint={USER_ID_HINT}
              error={failure.fieldError(field.name)}
            >
              <Input
                inputMode="numeric"
                autoComplete="off"
                className="min-h-11 md:min-h-8"
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
          )
        }
      </form.Field>
      <DialogFooter>
        <Button type="button" variant="outline" className={dialogButton} onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit" className={dialogButton} disabled={put.isPending || !anybody}>
          {put.isPending ? "Adding" : "Add member"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function AddDialog({
  open,
  fromList,
  ...form
}: AddFormProps & { open: boolean; fromList: boolean }) {
  const returnFocus = useReturnFocus(open);
  const { onCancel } = form;
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
    >
      <DialogContent className={dialogFit} onCloseAutoFocus={returnFocus}>
        <DialogHeader>
          <DialogTitle>Add member</DialogTitle>
          <DialogDescription>The user joins this team as a member.</DialogDescription>
        </DialogHeader>
        {fromList ? <AddFromListForm {...form} /> : <AddForm {...form} />}
      </DialogContent>
    </Dialog>
  );
}

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
  // A team the gateway does not show any more is not shown here either, though
  // it was loaded before: the 404 of asking again wins over what is shown.
  // Every other failure of asking again keeps it.
  const hidden = team.error instanceof ApiError && team.error.status === 404;
  if (team.data === undefined || hidden) {
    if (team.error !== null) {
      return (
        <QueryProblem
          notFound
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
