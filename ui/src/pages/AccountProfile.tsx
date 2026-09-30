import { useState } from "react";
import { useUpdateUser } from "@/api/queries";
import { can, type Me } from "@/auth/guards";
import { control } from "@/components/classes";
import { NameDialog } from "@/components/NameDialog";
import { Part } from "@/components/Part";
import { RoleBadge, TeamRoleBadge } from "@/components/RoleBadge";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";

export const DONE = {
  name: "Name changed.",
} as const;

/** Who is signed in, as `/api/auth/me` says it. Only the name can be changed here. */
export function Profile({ me }: { me: Me }) {
  const update = useUpdateUser();
  const toast = useToast();
  const [editing, setEditing] = useState(false);
  const mayRename = can(me, { type: "renameUser", userId: me.user.id });

  function close() {
    setEditing(false);
    update.reset();
  }

  return (
    <Part title="Profile">
      <dl className="grid max-w-md grid-cols-[auto_minmax(0,1fr)] items-center gap-x-6 gap-y-3 text-sm">
        <dt className="text-muted-foreground">Name</dt>
        <dd className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2">
          <span className="min-w-0 break-words">{me.user.name}</span>
          {mayRename ? (
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                setEditing(true);
              }}
            >
              Edit name
            </Button>
          ) : null}
        </dd>
        <dt className="text-muted-foreground">Email</dt>
        <dd className="min-w-0 break-words">{me.user.email}</dd>
        <dt className="text-muted-foreground">Role</dt>
        <dd>
          <RoleBadge role={me.user.role} />
        </dd>
        <dt className="self-start text-muted-foreground">Teams</dt>
        <dd className="min-w-0">
          {me.teams.length === 0 ? (
            <span className="text-muted-foreground">No teams</span>
          ) : (
            <ul className="flex flex-col gap-2">
              {me.teams.map((team) => (
                <li key={team.team_id} className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <span className="min-w-0 break-words">{team.name}</span>{" "}
                  <TeamRoleBadge role={team.role} />
                </li>
              ))}
            </ul>
          )}
        </dd>
      </dl>
      {mayRename ? (
        // The session asks again who is signed in: the shell shows the new name too.
        <NameDialog
          open={editing}
          user={me.user}
          update={update}
          onCancel={close}
          onDone={() => {
            close();
            toast(DONE.name);
          }}
        />
      ) : null}
    </Part>
  );
}
