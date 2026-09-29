import { useForm } from "@tanstack/react-form";
import { Link, useNavigate } from "@tanstack/react-router";
import { useRef, useState } from "react";
import { useDeleteUser, useReinviteUser, useUpdateUser, useUser } from "@/api/queries";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { dialogButton, dialogFit, useReturnFocus } from "@/components/dialog-fit";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure } from "@/components/form";
import { FormError } from "@/components/FormError";
import { NotFoundContent } from "@/components/NotFoundContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
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
import { Skeleton } from "@/components/ui/skeleton";
import {
  INVITE_LINK_DESCRIPTION,
  INVITE_LINK_TITLE,
  inviteUrl,
  RoleBadge,
  YouBadge,
} from "@/pages/Users";

type User = components["schemas"]["UserView"];

export const CONSEQUENCES = {
  role: "Their current sessions end and they must sign in again.",
  disable:
    "They are signed out, their access tokens are revoked, and their virtual keys stop working until they are enabled again.",
  enable: "They can sign in again, and their virtual keys work again.",
  reinvite: "You get a new link to send them. Earlier links stop working.",
  deleteActive:
    "Their virtual keys keep working without an owner. Revoke the keys first if they should stop.",
  deleteNotActive: "Their virtual keys are revoked.",
} as const;

export const DONE = {
  name: "Name changed.",
  role: "Role changed.",
  disable: "User disabled.",
  enable: "User enabled.",
  delete: "User deleted.",
} as const;

/** The id of the address, when it is a positive integer. */
export function idOf(text: string): number | null {
  if (!/^[1-9]\d{0,15}$/.test(text)) return null;
  const id = Number(text);
  return Number.isSafeInteger(id) ? id : null;
}

type Asking = "name" | "role" | "disable" | "enable" | "reinvite" | "delete";

interface NameFormProps {
  user: User;
  update: ReturnType<typeof useUpdateUser>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with the name as it is.
function NameForm({ user, update, onDone, onCancel }: NameFormProps) {
  const { mutateAsync } = update;
  const form = useForm({
    defaultValues: { name: user.name },
    onSubmit: async ({ value }) => {
      try {
        await mutateAsync({ id: user.id, body: { name: value.name } });
        onDone();
      } catch (error) {
        applyApiError(form, error);
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);

  return (
    <form
      ref={formRef}
      aria-label="Edit name"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        void form.handleSubmit();
      }}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="name">
        {(field) => (
          <Field label="Name" name={field.name} required error={failure.fieldError(field.name)}>
            <Input
              autoComplete="off"
              className="min-h-11 md:min-h-8"
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      <DialogFooter>
        <Button type="button" variant="outline" className={dialogButton} onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit" className={dialogButton} disabled={update.isPending}>
          {update.isPending ? "Saving" : "Save"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function NameDialog({ open, ...form }: NameFormProps & { open: boolean }) {
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
          <DialogTitle>Edit name</DialogTitle>
          <DialogDescription>The name is shown in the console and in the audit log.</DialogDescription>
        </DialogHeader>
        <NameForm {...form} />
      </DialogContent>
    </Dialog>
  );
}

const control = "min-h-11 md:min-h-8";

function Controls({ me, user }: { me: Me; user: User }) {
  const navigate = useNavigate();
  const toast = useToast();
  const update = useUpdateUser();
  const remove = useDeleteUser();
  const reinvite = useReinviteUser();
  const once = useSecretOnce(reinvite);
  const [asking, setAsking] = useState<Asking | null>(null);

  const own = user.id === me.user.id;
  const mayEdit = can(me, { type: "editUserRoleOrStatus" });
  const mayDelete = can(me, { type: "deleteUser" });
  // The API lets everybody change their own name, and admins every name.
  const mayRename = own || mayEdit;
  if (!mayRename && !mayDelete) return null;

  const { id } = user;
  const otherRole = user.role === "admin" ? "member" : "admin";

  function ask(what: Asking) {
    return () => {
      setAsking(what);
    };
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
        <Button type="button" variant="outline" className={control} onClick={ask("name")}>
          Edit name
        </Button>
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
                Resend invite
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
            body={CONSEQUENCES.role}
            confirmLabel={otherRole === "admin" ? "Make admin" : "Make member"}
            onConfirm={async () => {
              await update.mutateAsync({ id, body: { role: otherRole } });
              toast(DONE.role);
            }}
          />
          <ConfirmDialog
            open={asking === "disable"}
            onOpenChange={closeUpdate}
            title="Disable this user?"
            body={CONSEQUENCES.disable}
            confirmLabel="Disable"
            tone="danger"
            onConfirm={async () => {
              await update.mutateAsync({ id, body: { status: "disabled" } });
              toast(DONE.disable);
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
            title="Send a new invite?"
            body={CONSEQUENCES.reinvite}
            confirmLabel="Create link"
            onConfirm={async () => {
              const made = await reinvite.mutateAsync({ id });
              // Shows the link, and makes the mutation forget its answer.
              once.show(inviteUrl(made.invite_link));
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
