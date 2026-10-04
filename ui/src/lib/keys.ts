// The rules of a new virtual key: who can own it and which teams it can
// belong to, what of a choice is still offered when the choices change under
// the form, and the request that is made of the form. Pure functions: the
// form of `pages/KeysCreate` asks them, and the gateway decides.
import { ConsoleRefusal } from "@/api/errors";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { expiryOf, type Expiry } from "@/lib/expiry";
import { idOf } from "@/lib/id";
import { refOf, sortModels } from "@/lib/models";
import { tagsOf, type TagRow } from "@/lib/tags";

type User = components["schemas"]["UserView"];
type Model = components["schemas"]["ModelView"];
type Route = components["schemas"]["RouteView"];
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

/**
 * The teams a key of the viewer's own can belong to: those they are in, in
 * any role, as the session knows them.
 */
export function ownTeams(me: Me): TeamChoice[] {
  return me.teams
    .filter((team) => can(me, { type: "createKeyForSelf", teamId: team.team_id }))
    .map((team) => ({ id: team.team_id, name: team.name }));
}

/**
 * Mirrors what the gateway takes (`CreateKey` in its policy, and the checks
 * of `POST /api/keys`): the owner is an active user; a key of another user
 * belongs to a team the viewer may make keys in, and that the owner is a
 * member of; only who may make keys for anyone may leave out the team.
 * `users` is what the gateway lists for the viewer, each with the teams of
 * the user that the viewer may see: the teams of an owner are read from it.
 */
export function ownersFor(me: Me, users: readonly User[]): Owners {
  const anyone = can(me, { type: "createKeyForAnyone" });
  const own = ownTeams(me);
  const teamsOf = (ownerId: number): TeamChoice[] =>
    ownerId === me.user.id
      ? own
      : (users.find((user) => user.id === ownerId)?.teams ?? [])
          .filter((team) => can(me, { type: "createKeyForMember", teamId: team.team_id }))
          .map((team) => ({ id: team.team_id, name: team.name }))
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
  /** `all`: the key may call whatever its owner may; `some`: only `allowed`. */
  allow: "all" | "some";
  /** The names of the models (`provider/model`) and routes that were checked. */
  allowed: readonly string[];
  /** The tags as the editor holds them: rows that may still be wrong. */
  tags: readonly TagRow[];
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

export const CHOOSE_ALLOWED = "Choose at least one model or route.";

/**
 * The names to send as `allowed`, or nothing for no limit. Only names that
 * are offered are sent, in the order the list offers them (a name that is
 * offered no more is not sent: the gateway would refuse it). A limit with no
 * name to send is refused by the console itself, on the field.
 */
export function allowedOf(
  allow: "all" | "some",
  chosen: readonly string[],
  offered: readonly string[] | null,
): string[] | undefined {
  if (allow === "all") return undefined;
  const names = (offered ?? []).filter((name) => chosen.includes(name));
  if (names.length === 0) throw new ConsoleRefusal(CHOOSE_ALLOWED, "allowed");
  return names;
}

/** A model or a route that a key can be limited to. Its id is the name a call uses. */
export interface CallableItem {
  id: string;
  route: boolean;
}

/**
 * What the lists of models and routes give as choices for the allowlist of a
 * key: the models that are enabled (a disabled one cannot be called), as
 * `provider/model`, then the routes by name. The gateway gives the caller
 * only what they may use; for an admin it gives everything.
 */
export function callableItems(models: readonly Model[], routes: readonly Route[]): CallableItem[] {
  const names = new Set<string>();
  const items: CallableItem[] = [];
  const add = (id: string, route: boolean) => {
    if (names.has(id)) return;
    names.add(id);
    items.push({ id, route });
  };
  for (const model of sortModels(models.filter((one) => one.enabled))) add(refOf(model), false);
  for (const route of [...routes].sort((a, b) => a.name.localeCompare(b.name))) add(route.name, true);
  return items;
}

/** What the list of keys says of the models of a key: "All", or how many. */
export function allowedSummary(allowed: readonly string[] | null): string {
  return allowed === null ? "All" : String(allowed.length);
}

/**
 * The request for the key. What cannot be sent is refused by the console
 * itself. `offered` are the names of the models and routes that the form
 * offers, `null` while they are not known.
 */
export function requestOf(
  values: KeyValues,
  me: Me,
  offered: readonly string[] | null = null,
): CreateKeyRequest {
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
  const allowed = allowedOf(values.allow, values.allowed, offered);
  if (allowed !== undefined) body.allowed = allowed;
  const tags = tagsOf(values.tags);
  if (Object.keys(tags).length > 0) body.tags = tags;
  return body;
}
