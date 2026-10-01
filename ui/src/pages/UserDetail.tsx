import { Link, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { useDeleteUser, useReinviteUser, useUpdateUser, useUser } from "@/api/queries";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession, useSessionControl } from "@/auth/session";
import { control } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { NameDialog } from "@/components/NameDialog";
import { NotFoundContent } from "@/components/NotFoundContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { RoleBadge } from "@/components/RoleBadge";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { YouBadge } from "@/components/YouBadge";
import { idOf } from "@/lib/id";
import { INVITE_LINK_DESCRIPTION, INVITE_LINK_TITLE, inviteUrl } from "@/pages/Users";

type User = components["schemas"]["UserView"];

export const CONSEQUENCES = {
  role: "Their current sessions end and they must sign in again.",
  disable:
    "They are signed out, their access tokens are revoked, and their virtual keys stop working until they are enabled again.",
  enable: "They can sign in again, and their virtual keys work again.",
  reinvite: "Earlier links stop working.",
  deleteActive:
    "Their virtual keys keep working without an owner. Revoke the keys first if they should stop.",
  deleteNotActive: "Their virtual keys are revoked.",
} as const;

/** The same, said to the user who changes their own account. */
export const OWN_CONSEQUENCES = {
  role: "You will lose your admin rights and be signed out.",
  disable:
    "You will be signed out and cannot sign in again until another admin enables your account.",
} as const;

export const OWN_ACCOUNT_CHANGED_NOTICE = "You changed your own account. Sign in again.";

export const DONE = {
  name: "Name changed.",
  role: "Role changed.",
  disable: "User disabled.",
  enable: "User enabled.",
  delete: "User deleted.",
} as const;

type Asking = "name" | "role" | "disable" | "enable" | "reinvite" | "delete";

function Controls({ me, user }: { me: Me; user: User }) {
  const navigate = useNavigate();
  const toast = useToast();
  const update = useUpdateUser();
  const remove = useDeleteUser();
  const reinvite = useReinviteUser();
  const once = useSecretOnce(reinvite);
  const { end } = useSessionControl();
  const [asking, setAsking] = useState<Asking | null>(null);

  const own = user.id === me.user.id;
  const mayEdit = can(me, { type: "editUserRoleOrStatus" });
  // Nobody can delete their own account: the API refuses it to everybody.
  const mayDelete = !own && can(me, { type: "deleteUser" });
  const mayRename = can(me, { type: "renameUser", userId: user.id });
  if (!mayRename && !mayDelete) return null;

  const { id } = user;
  const otherRole = user.role === "admin" ? "member" : "admin";

  function ask(what: Asking) {
    return () => {
      setAsking(what);
    };
  }

  /**
   * After a change of a role or a status. The gateway ended the sessions of
   * the user: when that is the user who is signed in, the console does not
   * wait for a 401 to learn it, and ends the session as a sign-out does.
   */
  function changed(done: string) {
    if (own) end("left", OWN_ACCOUNT_CHANGED_NOTICE);
    else toast(done);
  }

  /** For the dialogs of `update`. */
  function closeUpdate(open: boolean) {
    if (open) return;
    setAsking(null);
    update.reset();
  }

  return (
    <>
      <div role="group" aria-label="Actions" className="flex flex-wrap gap-2">
        {mayRename ? (
          <Button type="button" variant="outline" className={control} onClick={ask("name")}>
            Edit name
          </Button>
        ) : null}
        {mayEdit ? (
          <>
            <Button type="button" variant="outline" className={control} onClick={ask("role")}>
              {otherRole === "admin" ? "Make admin" : "Make member"}
            </Button>
            {user.status === "disabled" ? (
              <Button type="button" variant="outline" className={control} onClick={ask("enable")}>
                Enable
              </Button>
            ) : (
              <Button type="button" variant="outline" className={control} onClick={ask("disable")}>
                Disable
              </Button>
            )}
            {user.status === "invited" ? (
              <Button
                type="button"
                variant="outline"
                className={control}
                onClick={ask("reinvite")}
              >
                New invite link
              </Button>
            ) : null}
          </>
        ) : null}
        {mayDelete ? (
          <Button type="button" variant="destructive" className={control} onClick={ask("delete")}>
            Delete
          </Button>
        ) : null}
      </div>

      <NameDialog
        open={asking === "name"}
        user={user}
        update={update}
        onCancel={() => {
          closeUpdate(false);
        }}
        onDone={() => {
          closeUpdate(false);
          toast(DONE.name);
        }}
      />
      {mayEdit ? (
        <>
          <ConfirmDialog
            open={asking === "role"}
            onOpenChange={closeUpdate}
            title={otherRole === "admin" ? "Make this user an admin?" : "Make this user a member?"}
            body={own ? OWN_CONSEQUENCES.role : CONSEQUENCES.role}
            confirmLabel={otherRole === "admin" ? "Make admin" : "Make member"}
            onConfirm={async () => {
              await update.mutateAsync({ id, body: { role: otherRole } });
              changed(DONE.role);
            }}
          />
          <ConfirmDialog
            open={asking === "disable"}
            onOpenChange={closeUpdate}
            title="Disable this user?"
            body={own ? OWN_CONSEQUENCES.disable : CONSEQUENCES.disable}
            confirmLabel="Disable"
            tone="danger"
            onConfirm={async () => {
              await update.mutateAsync({ id, body: { status: "disabled" } });
              changed(DONE.disable);
            }}
          />
          <ConfirmDialog
            open={asking === "enable"}
            onOpenChange={closeUpdate}
            title="Enable this user?"
            body={CONSEQUENCES.enable}
            confirmLabel="Enable"
            onConfirm={async () => {
              await update.mutateAsync({ id, body: { status: "active" } });
              toast(DONE.enable);
            }}
          />
          <ConfirmDialog
            open={asking === "reinvite"}
            onOpenChange={(open) => {
              if (open) return;
              setAsking(null);
              reinvite.reset();
            }}
            title="Create a new invite link?"
            body={CONSEQUENCES.reinvite}
            confirmLabel="Create link"
            onConfirm={async () => {
              const made = await reinvite.mutateAsync({ id });
              let link: string;
              try {
                link = inviteUrl(made.invite_link);
              } catch (error) {
                // The mutation does not keep the link that is not shown.
                reinvite.reset();
                throw error;
              }
              // Shows the link, and makes the mutation forget its answer.
              once.show(link);
            }}
          />
          <SecretDialog
            title={INVITE_LINK_TITLE}
            description={INVITE_LINK_DESCRIPTION}
            secret={once.secret}
            onClose={once.clear}
          />
        </>
      ) : null}
      {mayDelete ? (
        <ConfirmDialog
          open={asking === "delete"}
          onOpenChange={(open) => {
            if (open) return;
            setAsking(null);
            remove.reset();
          }}
          title="Delete this user?"
          body={user.status === "active" ? CONSEQUENCES.deleteActive : CONSEQUENCES.deleteNotActive}
          confirmLabel="Delete"
          tone="danger"
          onConfirm={async () => {
            await remove.mutateAsync({ id });
            toast(DONE.delete);
            await navigate({ to: "/users" });
          }}
        />
      ) : null}
    </>
  );
}

function Loading() {
  return (
    <div role="status" aria-busy="true" aria-label="Loading the user" className="flex flex-col gap-4">
      <Skeleton className="h-8 w-48" />
      <Skeleton className="h-4 w-full max-w-md" />
      <Skeleton className="h-4 w-full max-w-md" />
      <Skeleton className="h-4 w-full max-w-md" />
    </div>
  );
}

function Details({ id }: { id: number }) {
  const session = useSession();
  const user = useUser(id);

  if (session.status !== "signedIn") return null;
  if (user.data === undefined) {
    if (user.error !== null) {
      return (
        <QueryProblem
          notFound
          title="User"
          error={user.error}
          onRetry={() => {
            void user.refetch();
          }}
        />
      );
    }
    return <Loading />;
  }

  const shown = user.data;
  return (
    <>
      <Link
        to="/users"
        className="inline-flex min-h-11 w-fit items-center rounded-sm text-sm text-muted-foreground underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-8"
      >
        Back to users
      </Link>
      <PageHeader title={shown.name} subtitle={shown.email} />
      {shown.id === session.me.user.id ? (
        <p>
          <YouBadge />
        </p>
      ) : null}
      <dl
        aria-label="Details"
        className="grid max-w-md grid-cols-[auto_minmax(0,1fr)] gap-x-6 gap-y-3 text-sm"
      >
        <dt className="text-muted-foreground">Email</dt>
        <dd className="min-w-0 break-words">{shown.email}</dd>
        <dt className="text-muted-foreground">Role</dt>
        <dd>
          <RoleBadge role={shown.role} />
        </dd>
        <dt className="text-muted-foreground">Status</dt>
        <dd>
          <StatusBadge status={shown.status} />
        </dd>
        <dt className="text-muted-foreground">Created</dt>
        <dd>
          <Timestamp value={shown.created_at} />
        </dd>
        <dt className="text-muted-foreground">Last active</dt>
        <dd>
          <Timestamp value={shown.last_active_at} />
        </dd>
      </dl>
      <Controls me={session.me} user={shown} />
    </>
  );
}

/** The page of one user. `id` is the id as the address has it. */
export function UserDetail({ id }: { id: string }) {
  const number = idOf(id);
  // Not an id: the API is not asked.
  if (number === null) return <NotFoundContent />;
  return <Details id={number} />;
}
