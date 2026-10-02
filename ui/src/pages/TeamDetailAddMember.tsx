import { useForm } from "@tanstack/react-form";
import { useMemo, useRef } from "react";
import { ConsoleRefusal } from "@/api/errors";
import { useUsers, type useAddTeamMember } from "@/api/queries";
import type { components } from "@/api/schema";
import { control, cutLongChoice, selectList } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, onField, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

type Team = components["schemas"]["TeamSummary"];
type Member = components["schemas"]["MemberDetail"];
type User = components["schemas"]["UserView"];

export const ENTER_AN_EMAIL = "Enter the user's email.";

/**
 * The refusals of the gateway that are about the user who is added, said by
 * the field. Each stays the answer of the gateway that it is, with the words
 * of the gateway: "No active user with that email." and "Already in this team.".
 * A 404 that has another code is the team, which the form does not say.
 */
export function aboutTheEmail(error: unknown): unknown {
  return onField(onField(error, "user_not_found", "email"), "already_member", "email");
}

interface AddFormProps {
  team: Team;
  members: readonly Member[];
  add: ReturnType<typeof useAddTeamMember>;
  onDone: () => void;
  onCancel: () => void;
}

/**
 * The users an admin can choose from, as a shortcut that fills the email:
 * the active users who are not in the team. `null` while they are not known,
 * and when they could not be read: the email can always be typed.
 */
type Candidates = readonly User[] | null;

function Shortcut({
  candidates,
  email,
  onChoose,
}: {
  candidates: readonly User[];
  email: string;
  onChoose: (email: string) => void;
}) {
  // The choice is the user whose email is in the field: typing another clears it.
  const chosen = candidates.find((user) => user.email === email)?.email ?? "";
  return (
    <Field label="Choose a user" name="user">
      {({ id, name, ...described }) => (
        <Select name={name} value={chosen} onValueChange={onChoose}>
          <SelectTrigger id={id} {...described} className={`${control} w-full ${cutLongChoice}`}>
            <SelectValue placeholder="Choose a user" />
          </SelectTrigger>
          <SelectContent className={selectList}>
            {candidates.map((user) => (
              <SelectItem key={user.id} value={user.email}>
                {`${user.name} (${user.email})`}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </Field>
  );
}

// Mounted while the dialog is open: every opening starts with an empty form.
function AddForm({
  team,
  add,
  onDone,
  onCancel,
  candidates,
}: AddFormProps & { candidates: Candidates }) {
  const { mutateAsync } = add;
  const form = useForm({
    defaultValues: { email: "" },
    onSubmit: async ({ value }) => {
      try {
        const email = value.email.trim();
        // Nothing is sent for nothing: the console refuses it itself.
        if (email === "") throw new ConsoleRefusal(ENTER_AN_EMAIL, "email");
        await mutateAsync({ id: team.id, body: { email } });
        onDone();
      } catch (error) {
        applyApiError(form, aboutTheEmail(error));
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
      <form.Field name="email">
        {(field) => (
          <>
            <Field label="Email" name={field.name} required error={failure.fieldError(field.name)}>
              <Input
                type="email"
                autoComplete="off"
                autoCapitalize="none"
                spellCheck={false}
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
            {candidates !== null && candidates.length > 0 ? (
              <Shortcut
                candidates={candidates}
                email={field.state.value}
                onChoose={field.handleChange}
              />
            ) : null}
          </>
        )}
      </form.Field>
      <FormDialogFooter
        running={add.isPending}
        submit="Add member"
        submitting="Adding"
        onCancel={onCancel}
      />
    </form>
  );
}

/** For an admin, who may read the users: the shortcut is read when the dialog opens. */
function AddWithShortcut(props: AddFormProps) {
  const users = useUsers();
  const { members } = props;
  const candidates = useMemo(() => {
    if (users.data === undefined) return null;
    const inTeam = new Set(members.map((member) => member.user_id));
    // The gateway adds an active user only.
    return users.data.users.filter((user) => !inTeam.has(user.id) && user.status === "active");
  }, [users.data, members]);
  return <AddForm {...props} candidates={candidates} />;
}

export function AddDialog({
  open,
  withShortcut,
  ...form
}: AddFormProps & { open: boolean; withShortcut: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.add.isPending}
      title="Add member"
      description="The user joins this team as a member."
      onCancel={form.onCancel}
    >
      {withShortcut ? <AddWithShortcut {...form} /> : <AddForm {...form} candidates={null} />}
    </FormDialog>
  );
}
