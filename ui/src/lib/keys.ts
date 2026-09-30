// The rules of a new virtual key: who can own it and which teams it can
// belong to, what of a choice is still offered when the choices change under
// the form, the request that is made of the form, and what is known of a
// team that was asked for. Pure functions: the form of `pages/KeysCreate`
// asks them, and the gateway decides.
import { ApiError, ConsoleRefusal } from "@/api/errors";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { messageOfError } from "@/components/ErrorState";
import { expiryOf, type Expiry } from "@/lib/expiry";
import { idOf } from "@/lib/id";

type User = components["schemas"]["UserView"];
type TeamDetail = components["schemas"]["TeamDetail"];
type CreateKeyRequest = components["schemas"]["CreateKeyRequest"];

export const CHOOSE_A_TEAM = "Choose a team.";

export const NO_TEAM = "No team";

/** The value of the team filter, and of the team of a new key, for no team. */
export const WITHOUT_TEAM = "-";

/** A team a key can belong to. */
export interface TeamChoice {
  id: number;
  name: string;
}

/** Who can own a new key, and in which teams: for who chooses the owner. */
export interface Owners {
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

/** Whether a team is there, as far as the form knows: see `ChoosingKeyForm`. */
export type IsThere = (teamId: number) => boolean;

/** Without a list of teams to ask, every team the viewer is in is taken to be there. */
const everyTeam: IsThere = () => true;

/**
 * The teams a key of the viewer's own can belong to: those they are in, in
 * any role. They come from what the session knows of the viewer, which can
 * be older than what the gateway says of the teams now: a team that is there
 * no more is not among them.
 */
export function ownTeams(me: Me, isThere: IsThere = everyTeam): TeamChoice[] {
  return me.teams
    .filter((team) => isThere(team.team_id))
    .filter((team) => can(me, { type: "createKeyForSelf", teamId: team.team_id }))
    .map((team) => ({ id: team.team_id, name: team.name }));
}

/**
 * Mirrors what the gateway takes (`CreateKey` in its policy, and the checks
 * of `POST /api/keys`): the owner is an active user; a key of another user
 * belongs to a team the viewer may make keys in, and that the owner is a
 * member of; only who may make keys for anyone may leave out the team.
 * `users` is what the gateway lists for the viewer; `open` are the teams in
 * which the viewer may make a key for another member, with their members;
 * `isThere` says which of the viewer's own teams are there.
 */
export function ownersFor(
  me: Me,
  users: readonly User[],
  open: readonly TeamDetail[],
  isThere: IsThere,
): Owners {
  const anyone = can(me, { type: "createKeyForAnyone" });
  const own = ownTeams(me, isThere);
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

export interface KeyValues {
  name: string;
  owner_id: string;
  /** The id of a team, `WITHOUT_TEAM`, or nothing while a team has to be chosen. */
  team_id: string;
  expires_at: Expiry;
}

/** The teams a key of one owner can belong to, and whether it can have none. */
export interface TeamChoices {
  teams: readonly TeamChoice[];
  none: boolean;
}

/**
 * The owner that is chosen, as long as they are offered. The choices can
 * change under the form, when a user or a team is gone since the dialog
 * opened: an owner who is offered no more is not the owner any more, and the
 * owner is the viewer again, as when the dialog opened.
 */
export function ownerOffered(value: string, me: Me, owners: Owners | null): string {
  if (owners === null || owners.people.some((user) => String(user.id) === value)) return value;
  return String(me.user.id);
}

/**
 * The team that is chosen, as long as it is offered. A team that is offered
 * no more is not chosen any more: the choice is no team again, or, where a
 * key must have a team, the first team that is left. What was not chosen
 * stays so. It is what the form shows and what is sent, so that a request
 * never names a team that is not offered.
 */
export function teamOffered(value: string, { teams, none }: TeamChoices): string {
  if (value === "" || value === WITHOUT_TEAM) return value;
  if (teams.some((team) => String(team.id) === value)) return value;
  if (none) return WITHOUT_TEAM;
  const [first] = teams;
  return first === undefined ? "" : String(first.id);
}

/** The owner and the team of the key, as the form holds them. */
export interface Chosen {
  owner_id: string;
  team_id: string;
}

/** The team of a key whose owner was just chosen: none where that can be, otherwise none is chosen yet. */
export function teamOfNewOwner({ none }: TeamChoices): string {
  return none ? WITHOUT_TEAM : "";
}

/**
 * The choice as it is offered now: what the form shows, what it sends, and
 * what it holds (`KeyForm` writes it back). When the owner is the owner no
 * more, the team goes with them, as when the owner is changed by hand: it was
 * chosen for a key of theirs, and nobody chose it for a key of the viewer's.
 */
export function choiceOffered(
  chosen: Chosen,
  me: Me,
  owners: Owners | null,
  teamsFor: (ownerId: string) => TeamChoices,
): Chosen {
  const owner_id = ownerOffered(chosen.owner_id, me, owners);
  if (owner_id !== chosen.owner_id) {
    return { owner_id, team_id: teamOfNewOwner(teamsFor(owner_id)) };
  }
  return { owner_id, team_id: teamOffered(chosen.team_id, teamsFor(owner_id)) };
}

/**
 * The choice the form shows and holds (`KeyForm` writes it back): the choice
 * as it is offered, but while the owners are read for who chooses them
 * (`waiting`), the choice as it is. The teams of another owner are not known
 * then, and `teamsFor` falls back to the viewer's own teams, by which a team
 * chosen for another owner must not be judged: it would be taken away.
 */
export function choiceShown(
  chosen: Chosen,
  me: Me,
  owners: Owners | null,
  teamsFor: (ownerId: string) => TeamChoices,
  waiting: boolean,
): Chosen {
  return waiting ? chosen : choiceOffered(chosen, me, owners, teamsFor);
}

/** The request for the key. What cannot be sent is refused by the console itself. */
export function requestOf(values: KeyValues, me: Me): CreateKeyRequest {
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

/** What is known of a team that was asked for. */
export interface TeamRead {
  data: TeamDetail | undefined;
  error: unknown;
  /** How often asking for it has failed. */
  errorUpdateCount: number;
}

/** The team answered 404: it is gone, or not the viewer's to see any more. */
export function isGone(read: TeamRead): boolean {
  return read.data === undefined && read.error instanceof ApiError && read.error.status === 404;
}

/** The team is asked for, for the first time: nothing is known of it yet. */
export function isFirstRead(read: TeamRead): boolean {
  return read.data === undefined && read.error === null && read.errorUpdateCount === 0;
}

/**
 * The team is asked for again after a failure, and has not answered yet. For
 * that time the read says nothing of how it failed: whether the team was gone
 * is known only to who remembers it (`goneAmong`).
 */
export function isAskedAgain(read: TeamRead): boolean {
  return read.data === undefined && read.error === null && read.errorUpdateCount > 0;
}

/**
 * The team could not be read, or is asked for again after that. A team that
 * is gone is not among them: there is nothing to read, and a 404 is no read
 * that failed. Nor is one whose answer came for a session that is over, which
 * says nothing.
 */
export function isMissing(read: TeamRead): boolean {
  if (read.data !== undefined || isGone(read) || isFirstRead(read)) return false;
  return read.error === null || messageOfError(read.error) !== null;
}

/**
 * The teams of `ids` that are gone: those that answer 404, and those that
 * did when they were last heard of (`before`) and are asked for again. Such a
 * team stays gone until it answers: while it is asked for, at every return to
 * the window, it is neither offered for that moment nor a team that could not
 * be loaded.
 */
export function goneAmong(
  ids: readonly number[],
  reads: readonly TeamRead[],
  before: readonly number[],
): number[] {
  return ids.filter((id, index) => {
    const read = reads[index];
    if (read === undefined) return false;
    return isGone(read) || (isAskedAgain(read) && before.includes(id));
  });
}
