import { useForm, useSelector } from "@tanstack/react-form";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTeamDetails, useTeams, useUsers, type useCreateKey } from "@/api/queries";
import { can, type Me } from "@/auth/guards";
import { control, cutLongChoice, selectList } from "@/components/classes";
import { ErrorState } from "@/components/ErrorState";
import { ExpiryField } from "@/components/ExpiryField";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { NO_EXPIRY } from "@/lib/expiry";
import { idOf } from "@/lib/id";
import {
  choiceOffered,
  goneAmong,
  isFirstRead,
  isMissing,
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

export const TEAMS_NOT_LOADED = "Some teams could not be loaded.";

/** A select of a form: as wide as the form. */
const selectTrigger = `${control} w-full ${cutLongChoice}`;

/** The owners to choose from, for who chooses: `null` while they are not known. */
interface OwnerChoice {
  owners: Owners | null;
  /** Why they are not known. */
  error: unknown;
  /**
   * Some teams could not be read. The owners are known without them: a key
   * of another user cannot be put into one of them until it is read.
   */
  teamsMissing: boolean;
  /** Asks again for what is not known, and for the teams that could not be read. */
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
  };
  const form = useForm({
    defaultValues: start,
    onSubmit: async ({ value }) => {
      try {
        // What is sent is what the form shows: see `choiceOffered`.
        const choice = choiceOffered(value, me, owners, teamsFor);
        const made = await mutateAsync(requestOf({ ...value, ...choice }, me));
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
  // does not bring back a choice that the form showed no more.
  const ownerHeld = useSelector(form.store, (state) => state.values.owner_id);
  const teamHeld = useSelector(form.store, (state) => state.values.team_id);
  const shown = choiceOffered({ owner_id: ownerHeld, team_id: teamHeld }, me, owners, teamsFor);
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
      {choice?.teamsMissing === true && owners !== null ? (
        <Alert variant="destructive">
          <AlertDescription>
            <p>{TEAMS_NOT_LOADED}</p>
            <Button
              type="button"
              variant="outline"
              className="mt-2 max-md:min-h-11"
              onClick={choice.retry}
            >
              Retry
            </Button>
          </AlertDescription>
        </Alert>
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
 * The form of who chooses the owner. The users and the teams with their
 * members are asked for when the dialog opens: the page does not need them.
 *
 * A team that cannot be read does not keep a key from being made: the form
 * goes on without it and says so, with Retry. A team that answers 404 is
 * gone since the list was read: it is offered no more, for a key of another
 * user and for a key of the viewer's own, nothing is said, and the list is
 * read again, which says which teams there are.
 */
function ChoosingKeyForm(props: Omit<KeyFormProps, "choice">) {
  const { me } = props;
  const users = useUsers();
  const teams = useTeams();
  // The teams in which a key can be made for another member.
  const ids = useMemo(
    () =>
      (teams.data?.teams ?? [])
        .filter((team) => can(me, { type: "createKeyForMember", teamId: team.id }))
        .map((team) => team.id),
    [teams.data, me],
  );
  const details = useTeamDetails(ids);

  // The teams that are gone. They are remembered from one render to the
  // next, since a team that is asked for again says nothing until it answers.
  const [goneBefore, setGoneBefore] = useState<readonly number[]>([]);
  const gone = goneAmong(ids, details, goneBefore);
  // As one text: the list is read again when it changes, and so once for a
  // team, also when the list still names it and the team is asked for again.
  const goneText = gone.join(" ");
  if (goneText !== goneBefore.join(" ")) setGoneBefore(gone);
  const { refetch: readTeams } = teams;
  useEffect(() => {
    if (goneText !== "") void readTeams();
  }, [goneText, readTeams]);

  let owners: Owners | null = null;
  if (users.data !== undefined && teams.data !== undefined && !details.some(isFirstRead)) {
    const known = details.flatMap((detail) => (detail.data === undefined ? [] : [detail.data]));
    // A team is there when the list names it, and it did not answer 404.
    const listed = new Set(teams.data.teams.map((team) => team.id));
    owners = ownersFor(
      me,
      users.data.users,
      known,
      (teamId) => listed.has(teamId) && !gone.includes(teamId),
    );
  }
  const missing = details.filter(
    (detail, index) => isMissing(detail) && !gone.some((id) => id === ids[index]),
  );
  const error =
    (users.data === undefined ? users.error : null) ??
    (teams.data === undefined ? teams.error : null) ??
    null;

  return (
    <KeyForm
      {...props}
      choice={{
        owners,
        error,
        teamsMissing: missing.length > 0,
        retry: () => {
          if (users.data === undefined) void users.refetch();
          // A team that cannot be read may be gone: the list says which there are.
          if (teams.data === undefined || missing.length > 0) void teams.refetch();
          for (const detail of missing) void detail.refetch();
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
