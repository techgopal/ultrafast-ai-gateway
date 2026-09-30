import { useForm } from "@tanstack/react-form";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { ApiError, ConsoleRefusal } from "@/api/errors";
import {
  useChangePassword,
  useCreateToken,
  useRevokeToken,
  useTokens,
  useUpdateUser,
} from "@/api/queries";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession, useSessionControl } from "@/auth/session";
import { PASSWORD_POLICY, PASSWORDS_DIFFER, TOO_MANY_ATTEMPTS } from "@/components/AuthForm";
import { control } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { messageOfError } from "@/components/ErrorState";
import { ExpiryField } from "@/components/ExpiryField";
import { Field } from "@/components/Field";
import { applyApiError, onField, onStatus, submitOnce, useFormFailure } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { NameDialog } from "@/components/NameDialog";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { RoleBadge, TeamRoleBadge } from "@/components/RoleBadge";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { expiryOf, NO_EXPIRY } from "@/lib/expiry";
import { tokenStatus } from "@/lib/token-status";

type Token = components["schemas"]["TokenView"];
type CreateTokenRequest = components["schemas"]["CreateTokenRequest"];

export const PASSWORD_CHANGED =
  "Password changed. Your other sessions and all your access tokens were ended.";
export const WRONG_CURRENT_PASSWORD = "Current password is incorrect.";
export const NEW_TOKEN_TITLE = "Your new access token";
export const NEW_TOKEN_DESCRIPTION = "Copy this token now. It is not shown again.";
export const REVOKE_CONSEQUENCE =
  "Scripts using this token stop working at once. This cannot be undone.";

export const DONE = {
  name: "Name changed.",
  revoke: "Token revoked.",
} as const;

interface PartProps {
  title: string;
  description?: string;
  /** What can be done with the part as a whole, beside its title. */
  action?: ReactNode;
  children: ReactNode;
}

/** One of the parts of the page, under its own heading. */
function Part({ title, description, action, children }: PartProps) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="flex flex-col gap-4 border-t pt-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 id={id} className="text-lg font-semibold">
            {title}
          </h2>
          {description === undefined ? null : (
            <p className="mt-1 text-sm text-muted-foreground">{description}</p>
          )}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

// -------------------------------------------------------------- the profile

/** Who is signed in, as `/api/auth/me` says it. Only the name can be changed here. */
function Profile({ me }: { me: Me }) {
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

// ------------------------------------------------------------- the password

const PASSWORD_FIELDS = ["current_password", "new_password", "confirm_password"] as const;

const NO_PASSWORDS = { current_password: "", new_password: "", confirm_password: "" };

/**
 * A 401 of the password call that is not about the password: the session is
 * over. The client takes no 401 of this call for the end of a session, since
 * a wrong current password is a 401 too; the two differ by their code.
 */
function saysTheSessionIsOver(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401 && error.code !== "invalid_credentials";
}

/**
 * What the gateway refused, in the words of this form. A wrong current
 * password is said by its field. Too many attempts are about no field: the
 * gateway refuses before it looks at the password, and the limit is the one
 * of the sign-in. It is said as on the sign-in page: by the status, in the
 * same text, at the top of the form, where it stays until the form is sent
 * again.
 */
function inTheWordsOfTheForm(error: unknown): unknown {
  const wrong = onField(error, "invalid_credentials", "current_password", WRONG_CURRENT_PASSWORD);
  return onStatus(wrong, 429, TOO_MANY_ATTEMPTS);
}

/**
 * Changes the password of who is signed in. The session this is done in goes
 * on; the gateway ends the other sessions and revokes all access tokens.
 *
 * The passwords are held by the form while they are typed, and by nothing
 * after an answer: the three fields are emptied after a success and after a
 * refusal, and the mutation is reset. An empty form is never sent, so what
 * was refused cannot be sent again by a second press.
 *
 * That matters for the limit of the gateway, which is the one of the
 * sign-in: an attempt with a wrong current password counts against it, and
 * an empty current password is a wrong one. These do not count: a new
 * password the gateway does not take (a 422, which comes after the current
 * password was found right), a refusal for too many attempts (a 429), and
 * a 401 of a call without a session.
 */
function PasswordForm({ email }: { email: string }) {
  const change = useChangePassword();
  const { mutateAsync, reset } = change;
  const { end } = useSessionControl();
  const toast = useToast();
  const [refusals, setRefusals] = useState(0);
  const [changes, setChanges] = useState(0);
  const form = useForm({
    defaultValues: NO_PASSWORDS,
    onSubmit: async ({ value }) => {
      if (value.new_password !== value.confirm_password) {
        // Nothing is sent, so nothing is emptied: the user corrects what they typed.
        applyApiError(form, new ConsoleRefusal(PASSWORDS_DIFFER, "confirm_password"));
        return;
      }
      let refused = false;
      let refusal: unknown;
      try {
        await mutateAsync({
          current_password: value.current_password,
          new_password: value.new_password,
        });
      } catch (error) {
        refused = true;
        refusal = error;
      }
      for (const name of PASSWORD_FIELDS) form.setFieldValue(name, "");
      reset();
      if (!refused) {
        toast(PASSWORD_CHANGED);
        setChanges((count) => count + 1);
        return;
      }
      if (saysTheSessionIsOver(refusal)) {
        end("expired");
        return;
      }
      // An answer of a session that is over says nothing, and moves no focus.
      if (messageOfError(refusal) === null) return;
      // After the fields were emptied: the error of a field is about the
      // field as it is now, and goes when something is typed into it.
      applyApiError(form, inTheWordsOfTheForm(refusal));
      setRefusals((count) => count + 1);
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const currentRef = useRef<HTMLInputElement>(null);
  const submitRef = useRef<HTMLButtonElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const submit = submitOnce(form);

  // All three fields are empty after a refusal: the user starts again at the
  // first, whichever field the refusal was about. This runs after the focus
  // that `useFormFailure` sets.
  useEffect(() => {
    if (refusals > 0) currentRef.current?.focus();
  }, [refusals]);

  // The button that was pressed was disabled while the call ran, and so lost
  // the focus: after a success it has it again. A focus that is somewhere,
  // as in the field from which the form was sent, stays where it is.
  useEffect(() => {
    if (changes > 0 && document.activeElement === document.body) submitRef.current?.focus();
  }, [changes]);

  return (
    // The browser checks that the fields are filled: there is no `noValidate`.
    <form
      ref={formRef}
      aria-label="Change password"
      className="flex max-w-md flex-col gap-4"
      onSubmit={(event) => {
        // A form with an empty field is not sent, however it was submitted.
        if (PASSWORD_FIELDS.some((name) => form.state.values[name] === "")) {
          event.preventDefault();
          return;
        }
        submit(event);
      }}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      {/* Whose password it is, for the password manager of the browser. It is not shown and not sent. */}
      <input type="text" name="username" autoComplete="username" value={email} readOnly hidden />
      <form.Field name="current_password">
        {(field) => (
          <Field
            label="Current password"
            name={field.name}
            required
            error={failure.fieldError(field.name)}
          >
            <Input
              ref={currentRef}
              type="password"
              autoComplete="current-password"
              className={`${control} w-full`}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      <form.Field name="new_password">
        {(field) => (
          <Field
            label="New password"
            name={field.name}
            required
            hint={PASSWORD_POLICY}
            error={failure.fieldError(field.name)}
          >
            <Input
              type="password"
              autoComplete="new-password"
              className={`${control} w-full`}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      <form.Field name="confirm_password">
        {(field) => (
          <Field
            label="Confirm new password"
            name={field.name}
            required
            error={failure.fieldError(field.name)}
          >
            <Input
              type="password"
              autoComplete="new-password"
              className={`${control} w-full`}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      <Button
        ref={submitRef}
        type="submit"
        className={`${control} w-fit`}
        disabled={change.isPending}
      >
        {change.isPending ? "Changing the password" : "Change password"}
      </Button>
    </form>
  );
}

// --------------------------------------------------------------- the tokens

interface TokenFormProps {
  create: ReturnType<typeof useCreateToken>;
  /** The token was made: `secret` is the token itself, which is shown once. */
  onCreated: (secret: string) => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts with an empty form.
function TokenForm({ create, onCreated, onCancel }: TokenFormProps) {
  const { mutateAsync } = create;
  const form = useForm({
    defaultValues: { name: "", expires_at: NO_EXPIRY },
    onSubmit: async ({ value }) => {
      try {
        const body: CreateTokenRequest = { name: value.name };
        const expires = expiryOf(value.expires_at);
        if (expires !== undefined) body.expires_at = expires;
        const made = await mutateAsync(body);
        onCreated(made.secret);
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
      aria-label="Create token"
      noValidate
      className="flex min-w-0 flex-col gap-4"
      onSubmit={submitOnce(form)}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="name">
        {(field) => (
          <Field label="Name" name={field.name} required error={failure.fieldError(field.name)}>
            <Input
              autoComplete="off"
              className={control}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      <form.Field name="expires_at">
        {(field) => (
          <ExpiryField
            name={field.name}
            value={field.state.value}
            onChange={field.handleChange}
            onBlur={field.handleBlur}
            error={failure.fieldError(field.name)}
            hint="A token expires at the end of its day, in UTC."
          />
        )}
      </form.Field>
      <FormDialogFooter
        running={create.isPending}
        submit="Create token"
        submitting="Creating the token"
        onCancel={onCancel}
      />
    </form>
  );
}

function CreateDialog({ open, ...form }: TokenFormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.create.isPending}
      title="Create token"
      description="The token itself is shown once, when it is created."
      onCancel={form.onCancel}
    >
      <TokenForm {...form} />
    </FormDialog>
  );
}

const columns: Column<Token>[] = [
  {
    id: "name",
    header: "Name",
    cell: (token) => <span className="font-medium break-words">{token.name}</span>,
    sortValue: (token) => token.name,
  },
  {
    id: "display",
    header: "Token",
    cell: (token) => <span className="font-mono">{token.display}</span>,
  },
  {
    id: "expires_at",
    header: "Expires",
    cell: (token) => <Timestamp value={token.expires_at} />,
    sortValue: (token) => token.expires_at,
  },
  {
    id: "last_used_at",
    header: "Last used",
    cell: (token) => <Timestamp value={token.last_used_at} />,
    sortValue: (token) => token.last_used_at,
  },
  {
    id: "status",
    header: "Status",
    // The API has no status for a token: see `tokenStatus`.
    cell: (token) => <StatusBadge status={tokenStatus(token)} />,
    sortValue: (token) => tokenStatus(token),
  },
];

/** The access tokens of who is signed in: nobody sees or manages those of another. */
function Tokens() {
  const tokens = useTokens();
  const create = useCreateToken();
  const once = useSecretOnce(create);
  const revoke = useRevokeToken();
  const toast = useToast();
  const [creating, setCreating] = useState(false);
  // Which token the question is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Token | null>(null);
  const [asking, setAsking] = useState(false);

  const list = tokens.data?.tokens ?? [];
  const failed = tokens.error !== null && tokens.data === undefined;
  // What is revoked stays revoked: the gateway would change nothing.
  const mayRevoke = (token: Token) => token.revoked_at === null;
  const rowActions = list.some(mayRevoke)
    ? (token: Token) =>
        mayRevoke(token) ? (
          <Button
            type="button"
            variant="outline"
            className={control}
            onClick={() => {
              setTarget(token);
              setAsking(true);
            }}
          >
            Revoke
          </Button>
        ) : null
    : undefined;

  let content: ReactNode;
  if (!failed) {
    content = (
      <DataTable
        caption="Access tokens"
        columns={columns}
        rows={list}
        loading={tokens.isPending}
        getRowId={(token) => String(token.id)}
        empty={
          <EmptyState
            title="No access tokens"
            description="Create one to use the admin API from a script."
          />
        }
        {...(rowActions === undefined ? {} : { actions: rowActions })}
      />
    );
  } else {
    // The rest of the page is the user's own whatever the tokens are.
    content = (
      <QueryProblem
        part
        error={tokens.error}
        onRetry={() => {
          void tokens.refetch();
        }}
      />
    );
  }

  return (
    <Part
      title="Access tokens"
      description="An access token lets a script use the admin API as you."
      action={
        failed ? undefined : (
          <Button
            type="button"
            className={control}
            onClick={() => {
              setCreating(true);
            }}
          >
            Create token
          </Button>
        )
      }
    >
      {content}
      <CreateDialog
        open={creating}
        create={create}
        onCancel={() => {
          setCreating(false);
          create.reset();
        }}
        onCreated={(secret) => {
          setCreating(false);
          // Shows the token, and makes the mutation forget its answer.
          once.show(secret);
        }}
      />
      <SecretDialog
        title={NEW_TOKEN_TITLE}
        description={NEW_TOKEN_DESCRIPTION}
        secret={once.secret}
        onClose={once.clear}
      >
        {/* How it is used, with a placeholder: never the token. */}
        <div role="group" aria-label="How to use the token">
          <p className="text-sm text-muted-foreground">
            Use it as{" "}
            <code className="font-mono break-words text-foreground">
              {"Authorization: Bearer <token>"}
            </code>{" "}
            with the admin API.
          </p>
        </div>
      </SecretDialog>
      <ConfirmDialog
        open={asking}
        onOpenChange={(open) => {
          if (open) return;
          setAsking(false);
          revoke.reset();
        }}
        title={`Revoke ${target?.name ?? "this token"}?`}
        body={REVOKE_CONSEQUENCE}
        confirmLabel="Revoke"
        tone="danger"
        onConfirm={async () => {
          if (target === null) return;
          await revoke.mutateAsync({ id: target.id });
          toast(DONE.revoke);
        }}
      />
    </Part>
  );
}

// ----------------------------------------------------------------- the page

/** The account of who is signed in: their profile, their password, their access tokens. */
export function Account() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  return (
    <>
      <PageHeader title="Account" />
      <Profile me={session.me} />
      <Part
        title="Password"
        description="Changing it ends your other sessions and revokes all your access tokens."
      >
        <PasswordForm email={session.me.user.email} />
      </Part>
      <Tokens />
    </>
  );
}
