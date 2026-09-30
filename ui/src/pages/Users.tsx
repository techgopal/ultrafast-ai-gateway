import { useForm } from "@tanstack/react-form";
import { Link } from "@tanstack/react-router";
import { useMemo, useRef, useState } from "react";
import { ConsoleRefusal } from "@/api/errors";
import { useInviteUser, useUsers } from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { applyApiError, onField, submitOnce, useFormFailure } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { ROLE_NAMES, RoleBadge } from "@/components/RoleBadge";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { YouBadge } from "@/components/YouBadge";

type User = components["schemas"]["UserView"];

export const INVITE_LINK_TITLE = "Invite link";
export const INVITE_LINK_DESCRIPTION =
  "Send this link to the user. It works once and expires in 7 days.";

export const INVITE_LINK_UNUSABLE = "The gateway returned an invite link that cannot be used.";

/**
 * The invite link as the user can open it. The API gives a path of the
 * console, which starts with one `/`; anything else could name another
 * host, and is refused by the console itself: the dialog shows the message
 * where it shows a refusal of the gateway, which this is not.
 */
export function inviteUrl(path: string): string {
  const { origin } = window.location;
  const unusable = new ConsoleRefusal(INVITE_LINK_UNUSABLE);
  // A browser reads a backslash as a slash.
  if (!/^\/[^/\\]/.test(path)) throw unusable;
  let url: URL;
  try {
    url = new URL(path, origin);
  } catch {
    throw unusable;
  }
  if (url.origin !== origin) throw unusable;
  return url.href;
}

interface InviteFormProps {
  invite: ReturnType<typeof useInviteUser>;
  onInvited: (link: string) => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with an empty form.
function InviteForm({ invite, onInvited, onCancel }: InviteFormProps) {
  const { mutateAsync, reset } = invite;
  const form = useForm({
    defaultValues: { name: "", email: "", role: "member" },
    onSubmit: async ({ value }) => {
      try {
        const made = await mutateAsync(value);
        let link: string;
        try {
          link = inviteUrl(made.invite_link);
        } catch (error) {
          // The mutation does not keep the link that is not shown.
          reset();
          throw error;
        }
        onInvited(link);
      } catch (error) {
        applyApiError(form, onField(error, "user_exists", "email"));
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);

  return (
    <form
      ref={formRef}
      aria-label="Invite user"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={submitOnce(form)}
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
      <form.Field name="email">
        {(field) => (
          <Field label="Email" name={field.name} required error={failure.fieldError(field.name)}>
            <Input
              type="email"
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
      <form.Field name="role">
        {(field) => (
          <Field label="Role" name={field.name} error={failure.fieldError(field.name)}>
            {({ id, name, ...described }) => (
              <RadioGroup
                {...described}
                id={id}
                name={name}
                aria-label="Role"
                value={field.state.value}
                onValueChange={field.handleChange}
              >
                {(["member", "admin"] as const).map((role) => (
                  <div key={role} className="flex min-h-11 items-center gap-2 md:min-h-8">
                    <RadioGroupItem id={`${id}-${role}`} value={role} />
                    <Label htmlFor={`${id}-${role}`}>{ROLE_NAMES[role]}</Label>
                  </div>
                ))}
              </RadioGroup>
            )}
          </Field>
        )}
      </form.Field>
      <FormDialogFooter
        running={invite.isPending}
        submit="Create invite link"
        submitting="Creating the link"
        onCancel={onCancel}
      />
    </form>
  );
}

interface InviteDialogProps extends InviteFormProps {
  open: boolean;
}

function InviteDialog({ open, invite, onInvited, onCancel }: InviteDialogProps) {
  return (
    <FormDialog
      open={open}
      running={invite.isPending}
      title="Invite user"
      description="They get a link to set their password."
      onCancel={onCancel}
    >
      <InviteForm invite={invite} onInvited={onInvited} onCancel={onCancel} />
    </FormDialog>
  );
}

export function Users() {
  const session = useSession();
  const users = useUsers();
  const invite = useInviteUser();
  const once = useSecretOnce(invite);
  const [inviting, setInviting] = useState(false);
  const ownId = session.status === "signedIn" ? session.me.user.id : null;

  const columns = useMemo(
    (): Column<User>[] => [
      {
        id: "name",
        header: "Name",
        cell: (user) => (
          <span className="inline-flex flex-wrap items-center gap-2">
            <Link
              to="/users/$id"
              params={{ id: String(user.id) }}
              className="inline-flex min-h-11 items-center rounded-sm font-medium underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-8"
            >
              {user.name}
            </Link>
            {user.id === ownId ? <YouBadge /> : null}
          </span>
        ),
        sortValue: (user) => user.name,
      },
      { id: "email", header: "Email", cell: (user) => user.email, sortValue: (user) => user.email },
      {
        id: "role",
        header: "Role",
        cell: (user) => <RoleBadge role={user.role} />,
        sortValue: (user) => user.role,
      },
      {
        id: "status",
        header: "Status",
        cell: (user) => <StatusBadge status={user.status} />,
        sortValue: (user) => user.status,
      },
      {
        id: "last_active_at",
        header: "Last active",
        cell: (user) => <Timestamp value={user.last_active_at} />,
        sortValue: (user) => user.last_active_at,
      },
    ],
    [ownId],
  );

  if (session.status !== "signedIn") return null;
  const mayInvite = can(session.me, { type: "inviteUser" });

  function closeInvite() {
    setInviting(false);
    invite.reset();
  }

  const inviteButton = mayInvite ? (
    <Button
      type="button"
      className="min-h-11 md:min-h-8"
      onClick={() => {
        setInviting(true);
      }}
    >
      Invite user
    </Button>
  ) : undefined;

  const failed = users.error !== null && users.data === undefined;
  return (
    <>
      <PageHeader title="Users" actions={failed ? undefined : inviteButton} />
      {failed ? (
        <QueryProblem
          error={users.error}
          onRetry={() => {
            void users.refetch();
          }}
        />
      ) : (
        <DataTable
          caption="Users"
          columns={columns}
          rows={users.data?.users ?? []}
          loading={users.isPending}
          getRowId={(user) => String(user.id)}
          empty={<EmptyState title="No users" description="Nobody is here for you to see." />}
        />
      )}
      {mayInvite ? (
        <>
          <InviteDialog
            open={inviting}
            invite={invite}
            onCancel={closeInvite}
            onInvited={(link) => {
              setInviting(false);
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
    </>
  );
}
