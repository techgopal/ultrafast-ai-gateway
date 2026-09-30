import { useForm } from "@tanstack/react-form";
import { useId, useMemo, useRef, useState } from "react";
import { ConsoleRefusal } from "@/api/errors";
import {
  useCreateKey,
  useKeys,
  useRevokeKey,
  useTeamDetails,
  useTeams,
  useUsers,
} from "@/api/queries";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { dialogButton, dialogFit, useReturnFocus } from "@/components/dialog-fit";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure } from "@/components/form";
import { FormError } from "@/components/FormError";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { dayIn, endOfDay, today } from "@/lib/expiry";
import { idOf } from "@/lib/id";

type Key = components["schemas"]["KeyView"];
type User = components["schemas"]["UserView"];
type TeamDetail = components["schemas"]["TeamDetail"];
type CreateKeyRequest = components["schemas"]["CreateKeyRequest"];

export const NEW_KEY_TITLE = "Your new key";
export const NEW_KEY_DESCRIPTION = "Copy this key now. It is not shown again.";
export const SUSPENDED_HINT = "The owner is not active";
export const REVOKE_CONSEQUENCE = "Apps using this key stop working at once. This cannot be undone.";
export const KEY_REVOKED = "Key revoked.";
export const CHOOSE_A_TEAM = "Choose a team.";
export const CHOOSE_A_DATE = "Choose a date.";

const NO_OWNER = "No owner";
const NO_TEAM = "No team";

const control = "min-h-11 md:min-h-8";
/**
 * What a select shows of a long choice is one line, cut at its end: a long
 * email does not make the select, and with it the dialog, wider than the screen.
 */
const cutLongChoice =
  "*:data-[slot=select-value]:block *:data-[slot=select-value]:min-w-0 *:data-[slot=select-value]:truncate";
/** A select of a form: as wide as the form. */
const selectTrigger = `${control} w-full ${cutLongChoice}`;
/** A select among the filters: no wider than the page. */
const filterTrigger = `${control} max-w-full ${cutLongChoice}`;
/** The list of a select is no wider than the screen; a long choice wraps. */
const selectList = "max-w-[calc(100vw-2rem)]";

// ------------------------------------------------------------- the filters

/** The value of a filter that leaves nothing out. No id and no status is written so. */
const ANY = "*";
/** The value of the team filter, and of the team of a new key, for no team. */
const WITHOUT_TEAM = "-";

/** The statuses the gateway knows, in the order they are offered. */
const STATUSES = ["active", "suspended", "expired", "revoked"];

interface Filtering {
  search: string;
  team: string;
  status: string;
  showRevoked: boolean;
}

/** The text is looked for in the name, the owner and what is shown of the key. */
function matches(key: Key, { search, team, status, showRevoked }: Filtering): boolean {
  if (!showRevoked && key.status === "revoked") return false;
  if (status !== ANY && key.status !== status) return false;
  if (team === WITHOUT_TEAM ? key.team_id !== null : team !== ANY && String(key.team_id) !== team) {
    return false;
  }
  const text = search.trim().toLowerCase();
  if (text === "") return true;
  return [key.name, key.owner_email ?? "", key.display].some((value) =>
    value.toLowerCase().includes(text),
  );
}

interface Choice {
  value: string;
  label: string;
}

/** The teams the keys belong to, by name, and "No team" when a key has none. */
function teamChoices(keys: readonly Key[]): Choice[] {
  const names = new Map<number, string>();
  for (const key of keys) {
    if (key.team_id !== null) names.set(key.team_id, key.team_name ?? String(key.team_id));
  }
  const teams = [...names]
    .map(([id, label]) => ({ value: String(id), label }))
    .sort((a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: "base" }));
  const none = keys.some((key) => key.team_id === null)
    ? [{ value: WITHOUT_TEAM, label: NO_TEAM }]
    : [];
  return [{ value: ANY, label: "All teams" }, ...teams, ...none];
}

/**
 * The statuses of the gateway, and after them what else the keys have: a
 * status the console does not know can be chosen as it is. "revoked" is a
 * choice only while revoked keys are shown.
 */
function statusChoices(keys: readonly Key[], showRevoked: boolean): Choice[] {
  const others = [...new Set(keys.map((key) => key.status))]
    .filter((status) => !STATUSES.includes(status))
    .sort();
  const statuses = [...STATUSES, ...others].filter(
    (status) => showRevoked || status !== "revoked",
  );
  return [
    { value: ANY, label: "All statuses" },
    ...statuses.map((status) => ({ value: status, label: status })),
  ];
}

/** What is chosen, when it is still a choice; otherwise nothing is left out. */
function chosen(value: string, choices: readonly Choice[]): string {
  return choices.some((choice) => choice.value === value) ? value : ANY;
}

interface FilterSelectProps {
  label: string;
  value: string;
  choices: readonly Choice[];
  onChange: (value: string) => void;
}

function FilterSelect({ label, value, choices, onChange }: FilterSelectProps) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger aria-label={label} className={filterTrigger}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent className={selectList}>
        {choices.map((choice) => (
          <SelectItem key={choice.value} value={choice.value}>
            {choice.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

// ---------------------------------------------------------------- the form

/** A team a key can belong to. */
interface TeamChoice {
  id: number;
  name: string;
}

/** Who can own a new key, and in which teams: for who chooses the owner. */
interface Owners {
  /** Who can be chosen: the viewer first. */
  people: readonly User[];
  /** The teams a key of this owner can belong to. */
  teamsOf: (ownerId: number) => readonly TeamChoice[];
  /** Whether a key of this owner can belong to no team. */
  withoutTeam: (ownerId: number) => boolean;
}

function byName(a: { name: string }, b: { name: string }): number {
  return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
}

/** The teams a key of the viewer's own can belong to: those they are in, in any role. */
function ownTeams(me: Me): TeamChoice[] {
  return me.teams
    .filter((team) => can(me, { type: "createKeyForSelf", teamId: team.team_id }))
    .map((team) => ({ id: team.team_id, name: team.name }));
}

/**
 * Mirrors what the gateway takes (`CreateKey` in its policy, and the checks
 * of `POST /api/keys`): the owner is an active user; a key of another user
 * belongs to a team the viewer may make keys in, and that the owner is a
 * member of; only who may make keys for anyone may leave out the team.
 * `users` is what the gateway lists for the viewer; `open` are the teams in
 * which the viewer may make a key for another member, with their members.
 */
function ownersFor(me: Me, users: readonly User[], open: readonly TeamDetail[]): Owners {
  const anyone = can(me, { type: "createKeyForAnyone" });
  const own = ownTeams(me);
  const teamsOf = (ownerId: number): TeamChoice[] =>
    ownerId === me.user.id
      ? own
      : open
          .filter((detail) => detail.members.some((member) => member.user_id === ownerId))
          .map((detail) => ({ id: detail.team.id, name: detail.team.name }))
          .sort(byName);
  const others = users
    .filter((user) => user.id !== me.user.id && user.status === "active")
    .filter((user) => anyone || teamsOf(user.id).length > 0)
    .sort(byName);
  return {
    people: [me.user, ...others],
    teamsOf,
    withoutTeam: (ownerId) =>
      ownerId === me.user.id ? can(me, { type: "createKeyForSelf", teamId: null }) : anyone,
  };
}

/** The owners to choose from, for who chooses: `null` while they are not known. */
interface OwnerChoice {
  owners: Owners | null;
  /** Why they are not known. */
  error: unknown;
  retry: () => void;
}

const EXPIRY = [
  ["never", "Never"],
  ["30", "In 30 days"],
  ["90", "In 90 days"],
  ["date", "On a date"],
] as const;

const DAYS: Record<string, number> = { "30": 30, "90": 90 };

/**
 * When the key expires, as the form holds it. It is one value of the form,
 * under the name the gateway has for it: what is said about `expires_at`,
 * by the gateway or by the console, is said about the choice and the day
 * together, and goes when either of them is changed.
 */
interface Expiry {
  /** One of `EXPIRY`. */
  choice: string;
  /** The day, when the key expires on a date. */
  day: string;
}

interface KeyValues {
  name: string;
  owner_id: string;
  /** The id of a team, `WITHOUT_TEAM`, or nothing while a team has to be chosen. */
  team_id: string;
  expires_at: Expiry;
}

/** When the key stops working, as the gateway takes it; nothing for never. */
function expiryOf({ choice, day }: Expiry): string | undefined {
  if (choice === "date") {
    const end = endOfDay(day);
    if (end === null) throw new ConsoleRefusal(CHOOSE_A_DATE, "expires_at");
    return end;
  }
  const days = DAYS[choice];
  return days === undefined ? undefined : (endOfDay(dayIn(days)) ?? undefined);
}

/** The request for the key. What cannot be sent is refused by the console itself. */
function requestOf(values: KeyValues, me: Me): CreateKeyRequest {
  const body: CreateKeyRequest = { name: values.name };
  // Without an owner the gateway takes the caller.
  const owner = idOf(values.owner_id);
  if (owner !== null && owner !== me.user.id) body.owner_id = owner;
  if (values.team_id !== WITHOUT_TEAM) {
    const team = idOf(values.team_id);
    if (team === null) throw new ConsoleRefusal(CHOOSE_A_TEAM, "team_id");
    body.team_id = team;
  }
  const expires = expiryOf(values.expires_at);
  if (expires !== undefined) body.expires_at = expires;
  return body;
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
    expires_at: { choice: "never", day: "" },
  };
  const form = useForm({
    defaultValues: start,
    onSubmit: async ({ value }) => {
      try {
        const made = await mutateAsync(requestOf(value, me));
        onCreated(made.secret);
      } catch (error) {
        applyApiError(form, error);
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);

  /** The teams of a key of this owner, and whether it can have none. */
  function teamsFor(ownerId: string): { teams: readonly TeamChoice[]; none: boolean } {
    const id = idOf(ownerId);
    if (owners === null || id === null) {
      return { teams: ownTeams(me), none: can(me, { type: "createKeyForSelf", teamId: null }) };
    }
    return { teams: owners.teamsOf(id), none: owners.withoutTeam(id) };
  }

  return (
    <form
      ref={formRef}
      aria-label="Create key"
      noValidate
      // No wider than the dialog, whatever its fields hold.
      className="flex min-w-0 flex-col gap-4"
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
                  value={field.state.value}
                  onValueChange={(next) => {
                    field.handleChange(next);
                    // The team of the owner before is not one of this owner.
                    const none = teamsFor(next).none;
                    form.setFieldValue("team_id", none ? WITHOUT_TEAM : "");
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
        <form.Subscribe selector={(state) => state.values.owner_id}>
          {(ownerId) => {
            const { teams, none } = teamsFor(ownerId);
            return (
              <form.Field name="team_id">
                {(field) => (
                  <Field
                    label="Team"
                    name={field.name}
                    required={!none}
                    hint={none ? undefined : "A key for another user belongs to a team you lead."}
                    error={failure.fieldError(field.name)}
                  >
                    {({ id, name, required, ...described }) => (
                      <Select
                        name={name}
                        value={field.state.value}
                        onValueChange={field.handleChange}
                        {...(required === true ? { required } : {})}
                      >
                        <SelectTrigger id={id} {...described} className={selectTrigger}>
                          <SelectValue placeholder="Choose a team" />
                        </SelectTrigger>
                        <SelectContent className={selectList}>
                          {none ? <SelectItem value={WITHOUT_TEAM}>{NO_TEAM}</SelectItem> : null}
                          {teams.map((team) => (
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
            );
          }}
        </form.Subscribe>
      )}

      <form.Field name="expires_at">
        {(field) => (
          <Field
            label="Expires"
            name={field.name}
            hint="A key expires at the end of its day, in UTC."
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <div className="flex flex-col gap-2">
                <RadioGroup
                  {...described}
                  id={id}
                  name={name}
                  aria-label="Expires"
                  value={field.state.value.choice}
                  onValueChange={(choice) => {
                    field.handleChange({ ...field.state.value, choice });
                  }}
                >
                  {EXPIRY.map(([value, label]) => (
                    <div key={value} className="flex min-h-11 items-center gap-2 md:min-h-8">
                      <RadioGroupItem id={`${id}-${value}`} value={value} />
                      <Label htmlFor={`${id}-${value}`}>{label}</Label>
                    </div>
                  ))}
                </RadioGroup>
                {field.state.value.choice === "date" ? (
                  <Input
                    {...described}
                    type="date"
                    aria-label="Expiry date"
                    min={today()}
                    className={control}
                    value={field.state.value.day}
                    onBlur={field.handleBlur}
                    onChange={(event) => {
                      field.handleChange({ ...field.state.value, day: event.target.value });
                    }}
                  />
                ) : null}
              </div>
            )}
          </Field>
        )}
      </form.Field>

      <DialogFooter>
        <Button type="button" variant="outline" className={dialogButton} onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit" className={dialogButton} disabled={create.isPending || waiting}>
          {create.isPending ? "Creating the key" : "Create key"}
        </Button>
      </DialogFooter>
    </form>
  );
}

/**
 * The form of who chooses the owner. The users and the teams with their
 * members are asked for when the dialog opens: the page does not need them.
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

  let owners: Owners | null = null;
  if (users.data !== undefined && teams.data !== undefined) {
    const known = details.flatMap((detail) => (detail.data === undefined ? [] : [detail.data]));
    if (known.length === details.length) owners = ownersFor(me, users.data.users, known);
  }
  const missing = details.filter((detail) => detail.data === undefined);
  const error =
    (users.data === undefined ? users.error : null) ??
    (teams.data === undefined ? teams.error : null) ??
    missing.find((detail) => detail.error !== null)?.error ??
    null;

  return (
    <KeyForm
      {...props}
      choice={{
        owners,
        error,
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

function CreateDialog({ open, ...form }: CreateDialogProps) {
  const returnFocus = useReturnFocus(open);
  const { me, onCancel } = form;
  // Who may make a key for somebody else chooses the owner.
  const chooses =
    can(me, { type: "createKeyForAnyone" }) ||
    me.teams.some((team) => can(me, { type: "createKeyForMember", teamId: team.team_id }));
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
    >
      <DialogContent className={dialogFit} onCloseAutoFocus={returnFocus}>
        <DialogHeader>
          <DialogTitle>Create key</DialogTitle>
          <DialogDescription>The key itself is shown once, when it is created.</DialogDescription>
        </DialogHeader>
        {chooses ? <ChoosingKeyForm {...form} /> : <KeyForm {...form} />}
      </DialogContent>
    </Dialog>
  );
}

/** How a new key is used. It shows a placeholder, never the key. */
function KeyExample() {
  return (
    <div role="group" aria-label="How to use the key" className="flex flex-col gap-2 text-sm">
      <p className="text-muted-foreground">
        Apps call this gateway at the base URL, and send the key in the header.
      </p>
      <dl className="flex flex-col gap-1">
        <dt className="text-muted-foreground">Base URL</dt>
        <dd className="mb-1">
          <code className="font-mono break-words">{`${window.location.origin}/v1`}</code>
        </dd>
        <dt className="text-muted-foreground">Header</dt>
        <dd>
          <code className="font-mono break-words">{"Authorization: Bearer <key>"}</code>
        </dd>
      </dl>
    </div>
  );
}

// ---------------------------------------------------------------- the page

/**
 * In the table a cell is one line. A name or an email that is longer than
 * most wraps in its cell, so that it does not make the table much wider than
 * the page; what is shorter stays on its line. On a card the text wraps anyway.
 */
const longText = "md:block md:w-max md:max-w-64 md:whitespace-normal";

const columns: Column<Key>[] = [
  {
    id: "name",
    header: "Name",
    cell: (key) => <span className={`${longText} font-medium break-words`}>{key.name}</span>,
    sortValue: (key) => key.name,
  },
  {
    id: "display",
    header: "Key",
    cell: (key) => <span className="font-mono">{key.display}</span>,
  },
  {
    id: "owner",
    header: "Owner",
    cell: (key) =>
      key.owner_email === null ? (
        <span className="text-muted-foreground">{NO_OWNER}</span>
      ) : (
        <span className={`${longText} break-all`}>{key.owner_email}</span>
      ),
    sortValue: (key) => key.owner_email,
  },
  {
    id: "team",
    header: "Team",
    cell: (key) =>
      key.team_name === null ? (
        <span className="text-muted-foreground">{NO_TEAM}</span>
      ) : (
        <span className={`${longText} break-words`}>{key.team_name}</span>
      ),
    sortValue: (key) => key.team_name,
  },
  {
    id: "expires_at",
    header: "Expires",
    cell: (key) => <Timestamp value={key.expires_at} />,
    sortValue: (key) => key.expires_at,
  },
  {
    id: "status",
    header: "Status",
    cell: (key) => (
      <span className="inline-flex flex-col items-start gap-1">
        <StatusBadge status={key.status} />
        {key.status === "suspended" ? (
          <span className="text-xs text-muted-foreground">{SUSPENDED_HINT}</span>
        ) : null}
      </span>
    ),
    sortValue: (key) => key.status,
  },
];

function KeyList({ me }: { me: Me }) {
  const keys = useKeys();
  const create = useCreateKey();
  const once = useSecretOnce(create);
  const revoke = useRevokeKey();
  const toast = useToast();
  const showRevokedId = useId();
  const [creating, setCreating] = useState(false);
  // Which key the question is about. Kept while the dialog closes.
  const [target, setTarget] = useState<Key | null>(null);
  const [asking, setAsking] = useState(false);
  const [search, setSearch] = useState("");
  const [teamChosen, setTeam] = useState(ANY);
  const [statusChosen, setStatus] = useState(ANY);
  const [showRevoked, setShowRevoked] = useState(false);

  const all = useMemo(() => keys.data?.keys ?? [], [keys.data]);
  const teams = useMemo(() => teamChoices(all), [all]);
  const statuses = useMemo(() => statusChoices(all, showRevoked), [all, showRevoked]);
  // A choice that is gone, as "revoked" when the revoked keys are hidden, filters nothing.
  const team = chosen(teamChosen, teams);
  const status = chosen(statusChosen, statuses);
  const rows = useMemo(
    () => all.filter((key) => matches(key, { search, team, status, showRevoked })),
    [all, search, team, status, showRevoked],
  );

  const mayCreate = can(me, { type: "createKeyForSelf", teamId: null });
  function mayRevoke(key: Key): boolean {
    // What is revoked stays revoked: the gateway would change nothing.
    return (
      key.status !== "revoked" &&
      can(me, { type: "revokeKey", ownerId: key.owner_id, teamId: key.team_id })
    );
  }
  const rowActions = all.some(mayRevoke)
    ? (key: Key) =>
        mayRevoke(key) ? (
          <Button
            type="button"
            variant="outline"
            className={control}
            onClick={() => {
              setTarget(key);
              setAsking(true);
            }}
          >
            Revoke
          </Button>
        ) : null
    : undefined;

  function closeCreate() {
    setCreating(false);
    create.reset();
  }

  const createButton = mayCreate ? (
    <Button
      type="button"
      className={control}
      onClick={() => {
        setCreating(true);
      }}
    >
      Create key
    </Button>
  ) : undefined;

  const failed = keys.error !== null && keys.data === undefined;
  const empty =
    all.length === 0 ? (
      <EmptyState title="No virtual keys" description="There is no key for you to see." />
    ) : (
      <EmptyState
        title="No keys match"
        description={
          showRevoked
            ? "Change the filters to see more keys."
            : "Change the filters, or turn on Show revoked."
        }
      />
    );

  return (
    <>
      <PageHeader title="Virtual keys" actions={failed ? undefined : createButton} />
      {failed ? (
        <QueryProblem
          error={keys.error}
          onRetry={() => {
            void keys.refetch();
          }}
        />
      ) : (
        <>
          {keys.isPending || all.length > 0 ? (
            <div role="group" aria-label="Filters" className="flex flex-wrap items-center gap-2">
              <Input
                type="search"
                aria-label="Search"
                placeholder="Search name, owner or key"
                autoComplete="off"
                className={`${control} w-full sm:w-64`}
                value={search}
                onChange={(event) => {
                  setSearch(event.target.value);
                }}
              />
              <FilterSelect label="Team" value={team} choices={teams} onChange={setTeam} />
              <FilterSelect label="Status" value={status} choices={statuses} onChange={setStatus} />
              <div className="flex min-h-11 items-center gap-2 md:min-h-8">
                <Checkbox
                  id={showRevokedId}
                  checked={showRevoked}
                  onCheckedChange={(checked) => {
                    setShowRevoked(checked === true);
                  }}
                />
                <Label htmlFor={showRevokedId}>Show revoked</Label>
              </div>
            </div>
          ) : null}
          <DataTable
            caption="Virtual keys"
            columns={columns}
            rows={rows}
            loading={keys.isPending}
            getRowId={(key) => String(key.id)}
            empty={empty}
            {...(rowActions === undefined ? {} : { actions: rowActions })}
          />
        </>
      )}
      {mayCreate ? (
        <>
          <CreateDialog
            open={creating}
            me={me}
            create={create}
            onCancel={closeCreate}
            onCreated={(secret) => {
              setCreating(false);
              // Shows the key, and makes the mutation forget its answer.
              once.show(secret);
            }}
          />
          <SecretDialog
            title={NEW_KEY_TITLE}
            description={NEW_KEY_DESCRIPTION}
            secret={once.secret}
            onClose={once.clear}
          >
            <KeyExample />
          </SecretDialog>
        </>
      ) : null}
      <ConfirmDialog
        open={asking}
        onOpenChange={(open) => {
          if (open) return;
          setAsking(false);
          revoke.reset();
        }}
        title={`Revoke ${target?.name ?? "this key"}?`}
        body={REVOKE_CONSEQUENCE}
        confirmLabel="Revoke"
        tone="danger"
        onConfirm={async () => {
          if (target === null) return;
          await revoke.mutateAsync({ id: target.id });
          toast(KEY_REVOKED);
        }}
      />
    </>
  );
}

/** The virtual keys the signed-in user may see. */
export function Keys() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  return <KeyList me={session.me} />;
}
