import { useForm } from "@tanstack/react-form";
import { useRef, useState, type ReactNode } from "react";
import { useCreateToken, useRevokeToken, useTokens } from "@/api/queries";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { ExpiryField } from "@/components/ExpiryField";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Part } from "@/components/Part";
import { QueryProblem } from "@/components/QueryProblem";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { expiryOf, NO_EXPIRY } from "@/lib/expiry";

type Token = components["schemas"]["TokenView"];
type CreateTokenRequest = components["schemas"]["CreateTokenRequest"];

export const NEW_TOKEN_TITLE = "Your new access token";
export const NEW_TOKEN_DESCRIPTION = "Copy this token now. It is not shown again.";
export const REVOKE_CONSEQUENCE =
  "Scripts using this token stop working at once. This cannot be undone.";

export const DONE = {
  revoke: "Token revoked.",
} as const;

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
  const onSubmit = useSubmit(form);

  return (
    <form
      ref={formRef}
      aria-label="Create token"
      noValidate
      className="flex min-w-0 flex-col gap-4"
      onSubmit={onSubmit}
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
    cell: (token) => <StatusBadge status={token.status} />,
    sortValue: (token) => token.status,
  },
];

/** The access tokens of who is signed in: nobody sees or manages those of another. */
export function Tokens() {
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
