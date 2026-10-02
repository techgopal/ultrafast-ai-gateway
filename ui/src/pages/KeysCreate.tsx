import { useForm, useSelector } from "@tanstack/react-form";
import { useEffect, useRef } from "react";
import { useUsers, type useCreateKey } from "@/api/queries";
import { can, type Me } from "@/auth/guards";
import { control, cutLongChoice, selectList } from "@/components/classes";
import { ErrorState } from "@/components/ErrorState";
import { ExpiryField } from "@/components/ExpiryField";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
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
import { Skeleton } from "@/components/ui/skeleton";
import { AllowedField, useCallable } from "@/pages/KeysAllowed";
import { NO_EXPIRY } from "@/lib/expiry";
import { idOf } from "@/lib/id";
import {
  choiceOffered,
  choiceShown,
  NO_TEAM,
  ownersFor,
  ownTeams,
  requestOf,
  teamOfNewOwner,
  WITHOUT_TEAM,
  type KeyValues,
  type Owners,
  type TeamChoices,
} from "@/lib/keys";

/** A select of a form: as wide as the form. */
const selectTrigger = `${control} w-full ${cutLongChoice}`;

/** The owners to choose from, for who chooses: `null` while they are not known. */
interface OwnerChoice {
  owners: Owners | null;
  /** Why they are not known. */
  error: unknown;
  /** Asks again for what is not known. */
  retry: () => void;
}

function person(user: { name: string; email: string }): string {
  return `${user.name} (${user.email})`;
}

interface KeyFormProps {
  me: Me;
  create: ReturnType<typeof useCreateKey>;
  /** The key was made: `secret` is the key itself, which is shown once. */
  onCreated: (secret: string) => void;
  onCancel: () => void;
  /** For who chooses the owner. Without it the owner is the viewer. */
  choice?: OwnerChoice;
}

// Mounted while the dialog is open: every opening starts with an empty form.
function KeyForm({ me, create, onCreated, onCancel, choice }: KeyFormProps) {
  const { mutateAsync } = create;
  const owners = choice?.owners ?? null;
  const waiting = choice !== undefined && owners === null;
  const start: KeyValues = {
    name: "",
    owner_id: String(me.user.id),
    team_id: WITHOUT_TEAM,
    expires_at: NO_EXPIRY,
    allow: "all",
    allowed: [],
  };
  const form = useForm({
    defaultValues: start,
    onSubmit: async ({ value }) => {
      // Nothing is sent while the owners are not known: the button is off for
      // that time, and a submit that comes all the same has no choice to send.
      if (waiting) return;
      try {
        // What is sent is what the form shows: see `choiceOffered`.
        const choice = choiceOffered(value, me, owners, teamsFor);
        const made = await mutateAsync(
          requestOf({ ...value, ...choice }, me, callable.items?.map((item) => item.id) ?? null),
        );
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
  // The models and routes are read when the key is limited to some.
  const allowHeld = useSelector(form.store, (state) => state.values.allow);
  const callable = useCallable(allowHeld === "some");

  /** The teams of a key of this owner, and whether it can have none. */
  function teamsFor(ownerId: string): TeamChoices {
    const id = idOf(ownerId);
    if (owners === null || id === null) {
      return { teams: ownTeams(me), none: can(me, { type: "createKeyForSelf", teamId: null }) };
    }
    return { teams: owners.teamsOf(id), none: owners.withoutTeam(id) };
  }

  // The choices can change under the form. What it shows then is the choice
  // as it is offered now, and that is written back into the form: the form
  // holds one value, the one it shows. So an error of the gateway about a
  // choice goes when the choice does, and a team or an owner that comes back
  // does not bring back a choice that the form showed no more. While the
  // owners are not known the choice is left as it is (`choiceShown`).
  const ownerHeld = useSelector(form.store, (state) => state.values.owner_id);
  const teamHeld = useSelector(form.store, (state) => state.values.team_id);
  const shown = choiceShown(
    { owner_id: ownerHeld, team_id: teamHeld },
    me,
    owners,
    teamsFor,
    waiting,
  );
  const offered = teamsFor(shown.owner_id);
  useEffect(() => {
    if (shown.owner_id !== ownerHeld) form.setFieldValue("owner_id", shown.owner_id);
    if (shown.team_id !== teamHeld) form.setFieldValue("team_id", shown.team_id);
  }, [form, shown.owner_id, shown.team_id, ownerHeld, teamHeld]);

  return (
    <form
      ref={formRef}
      aria-label="Create key"
      noValidate
      // No wider than the dialog, whatever its fields hold.
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

      {choice === undefined ? (
        <dl className="flex flex-col gap-2 text-sm">
          <dt className="leading-none font-medium">Owner</dt>
          <dd className="flex min-w-0 flex-wrap gap-x-2">
            <span>{me.user.name}</span>
            <span className="break-all text-muted-foreground">{me.user.email}</span>
          </dd>
        </dl>
      ) : null}
      {choice !== undefined && owners === null ? (
        choice.error !== null ? (
          <ErrorState error={choice.error} onRetry={choice.retry} />
        ) : (
          <div
            role="status"
            aria-busy="true"
            aria-label="Loading the users and teams"
            className="flex flex-col gap-2"
          >
            <Skeleton className="h-4 w-24" />
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-4 w-24" />
            <Skeleton className="h-8 w-full" />
          </div>
        )
      ) : null}
      {owners !== null ? (
        <form.Field name="owner_id">
          {(field) => (
            <Field label="Owner" name={field.name} error={failure.fieldError(field.name)}>
              {({ id, name, ...described }) => (
                <Select
                  name={name}
                  value={shown.owner_id}
                  onValueChange={(next) => {
                    field.handleChange(next);
                    // The team of the owner before is not one of this owner.
                    form.setFieldValue("team_id", teamOfNewOwner(teamsFor(next)));
                  }}
                >
                  <SelectTrigger id={id} {...described} className={selectTrigger}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent className={selectList}>
                    {owners.people.map((user) => (
                      <SelectItem key={user.id} value={String(user.id)}>
                        {person(user)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            </Field>
          )}
        </form.Field>
      ) : null}
      {waiting ? null : (
        <form.Field name="team_id">
          {(field) => (
            <Field
              label="Team"
              name={field.name}
              required={!offered.none}
              hint={offered.none ? undefined : "A key for another user belongs to a team you lead."}
              error={failure.fieldError(field.name)}
            >
              {({ id, name, required, ...described }) => (
                <Select
                  name={name}
                  value={shown.team_id}
                  onValueChange={field.handleChange}
                  {...(required === true ? { required } : {})}
                >
                  <SelectTrigger id={id} {...described} className={selectTrigger}>
                    <SelectValue placeholder="Choose a team" />
                  </SelectTrigger>
                  <SelectContent className={selectList}>
                    {offered.none ? <SelectItem value={WITHOUT_TEAM}>{NO_TEAM}</SelectItem> : null}
                    {offered.teams.map((team) => (
                      <SelectItem key={team.id} value={String(team.id)}>
                        {team.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            </Field>
          )}
        </form.Field>
      )}

      <form.Field name="expires_at">
        {(field) => (
          <ExpiryField
            name={field.name}
            value={field.state.value}
            onChange={field.handleChange}
            onBlur={field.handleBlur}
            error={failure.fieldError(field.name)}
            hint="A key expires at the end of its day, in UTC."
          />
        )}
      </form.Field>

      <form.Field name="allow">
        {(allow) => (
          <form.Field name="allowed">
            {(allowed) => (
              <AllowedField
                mode={allow.state.value}
                onMode={allow.handleChange}
                chosen={allowed.state.value}
                onChosen={allowed.handleChange}
                callable={callable}
                error={failure.fieldError(allowed.name)}
              />
            )}
          </form.Field>
        )}
      </form.Field>

      <FormDialogFooter
        running={create.isPending}
        submit="Create key"
        submitting="Creating the key"
        disabled={waiting}
        onCancel={onCancel}
      />
    </form>
  );
}

/**
 * The form of who chooses the owner. The users are asked for when the dialog
 * opens: the page does not need them. Each comes with the teams of the user
 * that the viewer may see, from which the teams of a key are chosen.
 */
function ChoosingKeyForm(props: Omit<KeyFormProps, "choice">) {
  const { me } = props;
  const users = useUsers();
  const owners = users.data === undefined ? null : ownersFor(me, users.data.users);
  return (
    <KeyForm
      {...props}
      choice={{
        owners,
        error: users.data === undefined ? users.error : null,
        retry: () => {
          void users.refetch();
        },
      }}
    />
  );
}

interface CreateDialogProps extends Omit<KeyFormProps, "choice"> {
  open: boolean;
}

export function CreateDialog({ open, ...form }: CreateDialogProps) {
  const { me } = form;
  // Who may make a key for somebody else chooses the owner.
  const chooses =
    can(me, { type: "createKeyForAnyone" }) ||
    me.teams.some((team) => can(me, { type: "createKeyForMember", teamId: team.team_id }));
  return (
    <FormDialog
      open={open}
      running={form.create.isPending}
      title="Create key"
      description="The key itself is shown once, when it is created."
      onCancel={form.onCancel}
    >
      {chooses ? <ChoosingKeyForm {...form} /> : <KeyForm {...form} />}
    </FormDialog>
  );
}
