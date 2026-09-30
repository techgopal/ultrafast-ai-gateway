import { useForm } from "@tanstack/react-form";
import { Link } from "@tanstack/react-router";
import { useRef, useState } from "react";
import { useCreateTeam, useTeams } from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { DataTable, type Column } from "@/components/DataTable";
import { dialogButton, dialogFit, useReturnFocus } from "@/components/dialog-fit";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { applyApiError, onField, useFormFailure } from "@/components/form";
import { FormError } from "@/components/FormError";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
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

type Team = components["schemas"]["TeamSummary"];

export const TEAM_CREATED = "Team created.";

interface NameFormProps {
  /** The name of the form, which is what it does. */
  label: string;
  /** The name as it is; empty for a new team. */
  name: string;
  submit: string;
  submitting: string;
  pending: boolean;
  /** Sends the name. What it rejects with is shown by the form. */
  send: (name: string) => Promise<unknown>;
  onDone: () => void;
  onCancel: () => void;
}

/**
 * The form of a team, which has one field. It is mounted while its dialog is
 * open: every opening starts with a new form.
 */
function TeamNameForm({
  label,
  name,
  submit,
  submitting,
  pending,
  send,
  onDone,
  onCancel,
}: NameFormProps) {
  const form = useForm({
    defaultValues: { name },
    onSubmit: async ({ value }) => {
      try {
        await send(value.name);
        onDone();
      } catch (error) {
        applyApiError(form, onField(error, "team_exists", "name"));
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);

  return (
    <form
      ref={formRef}
      aria-label={label}
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
        <Button type="submit" className={dialogButton} disabled={pending}>
          {pending ? submitting : submit}
        </Button>
      </DialogFooter>
    </form>
  );
}

interface TeamNameDialogProps extends Omit<NameFormProps, "label"> {
  open: boolean;
  title: string;
  description: string;
}

/** The dialog that names a team: a new one, or one that is renamed. */
export function TeamNameDialog({ open, title, description, ...form }: TeamNameDialogProps) {
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
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <TeamNameForm label={title} {...form} />
      </DialogContent>
    </Dialog>
  );
}

const columns: Column<Team>[] = [
  {
    id: "name",
    header: "Name",
    cell: (team) => (
      <Link
        to="/teams/$id"
        params={{ id: String(team.id) }}
        className="inline-flex min-h-11 items-center rounded-sm font-medium underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-8"
      >
        {team.name}
      </Link>
    ),
    sortValue: (team) => team.name,
  },
  {
    id: "member_count",
    header: "Members",
    cell: (team) => team.member_count,
    sortValue: (team) => team.member_count,
  },
  {
    id: "created_at",
    header: "Created",
    cell: (team) => <Timestamp value={team.created_at} />,
    sortValue: (team) => team.created_at,
  },
];

export function Teams() {
  const session = useSession();
  const teams = useTeams();
  const create = useCreateTeam();
  const toast = useToast();
  const [creating, setCreating] = useState(false);

  if (session.status !== "signedIn") return null;
  const mayCreate = can(session.me, { type: "createTeam" });

  function close() {
    setCreating(false);
    create.reset();
  }

  const newButton = mayCreate ? (
    <Button
      type="button"
      className="min-h-11 md:min-h-8"
      onClick={() => {
        setCreating(true);
      }}
    >
      New team
    </Button>
  ) : undefined;

  const failed = teams.error !== null && teams.data === undefined;
  return (
    <>
      <PageHeader title="Teams" actions={failed ? undefined : newButton} />
      {failed ? (
        <QueryProblem
          error={teams.error}
          onRetry={() => {
            void teams.refetch();
          }}
        />
      ) : (
        <DataTable
          caption="Teams"
          columns={columns}
          rows={teams.data?.teams ?? []}
          loading={teams.isPending}
          getRowId={(team) => String(team.id)}
          empty={<EmptyState title="No teams" description="There is no team for you to see." />}
        />
      )}
      {mayCreate ? (
        <TeamNameDialog
          open={creating}
          title="New team"
          description="Members are added on the page of the team."
          name=""
          submit="Create team"
          submitting="Creating the team"
          pending={create.isPending}
          send={(name) => create.mutateAsync({ name })}
          onCancel={close}
          onDone={() => {
            close();
            toast(TEAM_CREATED);
          }}
        />
      ) : null}
    </>
  );
}
