import { Link, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import {
  useDeleteUser,
  usePasswordLink,
  useReinviteUser,
  useSetUserGuardrails,
  useUpdateUser,
  useUser,
} from "@/api/queries";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession, useSessionControl } from "@/auth/session";
import { AttachedGuardrails } from "@/components/AttachedGuardrails";
import { control } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { USER_HINT } from "@/components/GuardrailPicker";
import { GuardrailsHolderDialog, HOLDER_SAVED } from "@/components/GuardrailsHolderDialog";
import { NameDialog } from "@/components/NameDialog";
import { NotFoundContent } from "@/components/NotFoundContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { RoleBadge, TeamRoleBadge } from "@/components/RoleBadge";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { YouBadge } from "@/components/YouBadge";
import { idOf } from "@/lib/id";
import {
  INVITE_LINK_DESCRIPTION,
  INVITE_LINK_TITLE,
  inviteUrl,
  PASSWORD_LINK_DESCRIPTION,
  PASSWORD_LINK_TITLE,
  PASSWORD_LINK_UNUSABLE,
  signInName,
} from "@/pages/Users";

type User = components["schemas"]["UserView"];

export const CONSEQUENCES = {
  role: "Their current sessions end and they must sign in again.",
  disable:
    "They are signed out, their access tokens are revoked, and their virtual keys stop working until they are enabled again.",
  enable: "They can sign in again, and their virtual keys work again.",
  reinvite: "Earlier links stop working.",
  passwordLink:
    "The user can then sign in with a password as well as through single sign-on. Earlier links stop working.",
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

type Asking =
  | "name"
  | "role"
  | "disable"
  | "enable"
  | "reinvite"
  | "passwordLink"
  | "delete"
  | "guardrails";

function Controls({ me, user }: { me: Me; user: User }) {
  const navigate = useNavigate();
  const toast = useToast();
  const update = useUpdateUser();
  const remove = useDeleteUser();
  const reinvite = useReinviteUser();
  const setGuardrails = useSetUserGuardrails();
  const once = useSecretOnce(reinvite);
  const passwordLink = usePasswordLink();
  const linkOnce = useSecretOnce(passwordLink);
  const { end } = useSessionControl();
  const [asking, setAsking] = useState<Asking | null>(null);

  const own = user.id === me.user.id;
  const mayEdit = can(me, { type: "editUserRoleOrStatus" });
  // Nobody can delete their own account: the API refuses it to everybody.
  const mayDelete = !own && can(me, { type: "deleteUser" });
  const mayRename = can(me, { type: "renameUser", userId: user.id });
  const mayGuard = can(me, { type: "manageGuardrails" });
  // For an active user of single sign-on who has no password: the gateway
  // refuses anyone else.
  const mayLink =
    can(me, { type: "inviteUser" }) &&
    user.auth_provider === "oidc" &&
    !user.has_password &&
    user.status === "active";
  if (!mayRename && !mayDelete && !mayGuard) return null;

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
            {mayLink ? (
              <Button
                type="button"
                variant="outline"
                className={control}
                onClick={ask("passwordLink")}
              >
                Send password link
              </Button>
            ) : null}
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
        {mayGuard ? (
          <Button type="button" variant="outline" className={control} onClick={ask("guardrails")}>
            Edit guardrails
          </Button>
        ) : null}
        {mayDelete ? (
          <Button type="button" variant="destructive" className={control} onClick={ask("delete")}>
            Delete
          </Button>
        ) : null}
      </div>

      {mayGuard ? (
        <GuardrailsHolderDialog
          open={asking === "guardrails"}
          description="The user's guardrails check every call of every key the user owns, after the guardrails of the gateway and of the team."
          hint={USER_HINT}
          start={user.guardrail_ids}
          pending={setGuardrails.isPending}
          send={(ids) => setGuardrails.mutateAsync({ id, ids })}
          onCancel={() => {
            closeUpdate(false);
            setGuardrails.reset();
          }}
          onDone={() => {
            closeUpdate(false);
            setGuardrails.reset();
            toast(HOLDER_SAVED);
          }}
        />
      ) : null}
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
          {mayLink ? (
            <>
              <ConfirmDialog
                open={asking === "passwordLink"}
                onOpenChange={(open) => {
                  if (open) return;
                  setAsking(null);
                  passwordLink.reset();
                }}
                title="Create a password link?"
                body={CONSEQUENCES.passwordLink}
                confirmLabel="Create link"
                onConfirm={async () => {
                  const made = await passwordLink.mutateAsync({ id });
                  let link: string;
                  try {
                    link = inviteUrl(made.url, PASSWORD_LINK_UNUSABLE);
                  } catch (error) {
                    // The mutation does not keep the link that is not shown.
                    passwordLink.reset();
                    throw error;
                  }
                  // Shows the link, and makes the mutation forget its answer.
                  linkOnce.show(link);
                }}
              />
              <SecretDialog
                title={PASSWORD_LINK_TITLE}
                description={PASSWORD_LINK_DESCRIPTION}
                secret={linkOnce.secret}
                onClose={linkOnce.clear}
              />
            </>
          ) : null}
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
    <>
      <PageHeader title={"User"} />
      <div role="status" aria-busy="true" aria-label="Loading the user" className="flex flex-col gap-4">
        <Skeleton className="h-4 w-full max-w-md" />
        <Skeleton className="h-4 w-full max-w-md" />
        <Skeleton className="h-4 w-full max-w-md" />
      </div>
    </>
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
        <dt className="text-muted-foreground">Sign-in</dt>
        <dd>{signInName(shown)}</dd>
        <dt className="text-muted-foreground">Teams</dt>
        <dd className="min-w-0">
          {shown.teams.length === 0 ? (
            "—"
          ) : (
            <ul aria-label="Teams" className="flex flex-col gap-1">
              {shown.teams.map((team) => (
                <li key={team.team_id} className="flex flex-wrap items-center gap-2">
                  <span className="break-words">{team.name}</span>
                  <TeamRoleBadge role={team.role} />
                </li>
              ))}
            </ul>
          )}
        </dd>
        <dt className="text-muted-foreground">Created</dt>
        <dd>
          <Timestamp value={shown.created_at} />
        </dd>
        <dt className="text-muted-foreground">Last active</dt>
        <dd>
          <Timestamp value={shown.last_active_at} />
        </dd>
        {can(session.me, { type: "manageGuardrails" }) ? (
          <>
            <dt className="text-muted-foreground">Guardrails</dt>
            <dd className="min-w-0">
              <AttachedGuardrails ids={shown.guardrail_ids} />
            </dd>
          </>
        ) : null}
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
