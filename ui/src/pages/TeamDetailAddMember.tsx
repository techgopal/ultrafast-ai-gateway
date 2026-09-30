import { useForm } from "@tanstack/react-form";
import { useMemo, useRef } from "react";
import { ApiError, ConsoleRefusal } from "@/api/errors";
import { useUsers, type usePutTeamMember } from "@/api/queries";
import type { components } from "@/api/schema";
import { ErrorState } from "@/components/ErrorState";
import { Field, type FieldWiring } from "@/components/Field";
import { applyApiError, onField, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Skeleton } from "@/components/ui/skeleton";
import { idOf } from "@/lib/id";

type Team = components["schemas"]["TeamSummary"];
type Member = components["schemas"]["MemberDetail"];

export const NO_USER_WITH_ID = "No user with that ID.";
export const USER_ID_HINT = "Ask an admin for the user's ID.";
export const CHOOSE_A_USER = "Choose a user.";

/** What the console says about the one field of the form. It is no answer of the gateway. */
function onUserId(message: string): ConsoleRefusal {
  return new ConsoleRefusal(message, "user_id");
}

/**
 * The refusals of the gateway that are about the user who is added, said by
 * the field. Each stays the answer of the gateway that it is.
 */
export function aboutTheUser(error: unknown): unknown {
  // The gateway says "not found"; the form says what was not found. It may be
  // the team as well: `usePutTeamMember` asks for the team again, and when it
  // is gone the page shows that in place of this form.
  if (error instanceof ApiError && error.status === 404) {
    return onField(error, error.code, "user_id", NO_USER_WITH_ID);
  }
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
  const onSubmit = useSubmit(form);

  return (
    <form
      ref={formRef}
      aria-label="Add member"
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="user_id">
        {(field) =>
          choice !== undefined ? (
            <Field group label="User" name={field.name} error={failure.fieldError(field.name)}>
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
      <FormDialogFooter
        running={put.isPending}
        submit="Add member"
        submitting="Adding"
        disabled={!anybody}
        onCancel={onCancel}
      />
    </form>
  );
}

export function AddDialog({
  open,
  fromList,
  ...form
}: AddFormProps & { open: boolean; fromList: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.put.isPending}
      title="Add member"
      description="The user joins this team as a member."
      onCancel={form.onCancel}
    >
      {fromList ? <AddFromListForm {...form} /> : <AddForm {...form} />}
    </FormDialog>
  );
}
