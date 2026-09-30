// What the console shows to whom. These mirror the policy of the API
// (`crates/gateway/src/identity/policy.rs`) so that a control the API would
// refuse is not offered. The API still decides: every call handles a 403 and
// a 404.
import type { components } from "@/api/schema";

type MeResponse = components["schemas"]["MeResponse"];

/** The signed-in user and their teams, as `/api/auth/me` gives them. */
export type Me = Pick<MeResponse, "user" | "teams">;

export type ConsoleAction =
  | { type: "inviteUser" }
  | { type: "editUserRoleOrStatus" }
  | { type: "deleteUser" }
  /** Changes the name of the user: everybody their own, an admin every one. */
  | { type: "renameUser"; userId: number }
  | { type: "createTeam" }
  /** Sees the team and its members: its members do, in any role. */
  | { type: "viewTeam"; teamId: number }
  | { type: "renameTeam"; teamId: number }
  | { type: "deleteTeam"; teamId: number }
  /** Adds a user to the team as a member. */
  | { type: "addMember"; teamId: number }
  | { type: "makeLead"; teamId: number }
  | { type: "removeMember"; teamId: number }
  /** `teamId` is `null` for a key that belongs to no team. */
  | { type: "createKeyForSelf"; teamId: number | null }
  /** For another user: only in a team the caller leads. */
  | { type: "createKeyForMember"; teamId: number }
  /** For any user, in a team of theirs or in none: what only an admin may. */
  | { type: "createKeyForAnyone" }
  | { type: "revokeKey"; ownerId: number | null; teamId: number | null }
  | { type: "manageProviders" }
  | { type: "viewAudit" }
  /**
   * Sees how many users and teams there are, on the overview: who manages
   * some, which is an admin and the lead of a team. The gateway lists users
   * and teams for everybody, as far as they are theirs to see; to a member
   * that is themselves and their own teams, which is nothing to count.
   */
  | { type: "viewUserAndTeamCounts" };

export function isAdmin(me: Me): boolean {
  return me.user.role === "admin";
}

/** Whether the user is the lead of the team. Being an admin does not make one a lead. */
export function leads(me: Me, teamId: number): boolean {
  return me.teams.some((team) => team.team_id === teamId && team.role === "lead");
}

export function ledTeamIds(me: Me): number[] {
  return me.teams.filter((team) => team.role === "lead").map((team) => team.team_id);
}

function isIn(me: Me, teamId: number): boolean {
  return me.teams.some((team) => team.team_id === teamId);
}

/** Whether the console offers the action to the user. */
export function can(me: Me, action: ConsoleAction): boolean {
  if (isAdmin(me)) return true;
  switch (action.type) {
    case "inviteUser":
    case "editUserRoleOrStatus":
    case "deleteUser":
    case "createTeam":
    case "deleteTeam":
    case "makeLead":
    case "createKeyForAnyone":
    case "manageProviders":
    case "viewAudit":
      return false;
    case "renameUser":
      return action.userId === me.user.id;
    case "renameTeam":
    case "addMember":
    case "removeMember":
    case "createKeyForMember":
      return leads(me, action.teamId);
    case "viewUserAndTeamCounts":
      return ledTeamIds(me).length > 0;
    case "viewTeam":
      return isIn(me, action.teamId);
    case "createKeyForSelf":
      return action.teamId === null || isIn(me, action.teamId);
    case "revokeKey":
      return (
        action.ownerId === me.user.id || (action.teamId !== null && leads(me, action.teamId))
      );
  }
}

/** The longest path from the address that is followed. */
const LONGEST_PATH = 2000;
/** How often a path is decoded to see what it may come to mean. */
const DECODINGS = 3;

function hasControlCharacter(text: string): boolean {
  for (const character of text) {
    const code = character.codePointAt(0) ?? 0;
    if (code < 0x20 || code === 0x7f) return true;
  }
  return false;
}

/** The part before the query string and the hash. */
function pathOf(text: string): string {
  const end = text.search(/[?#]/);
  return end === -1 ? text : text.slice(0, end);
}

function staysHere(text: string): boolean {
  if (hasControlCharacter(text)) return false;
  const path = pathOf(text);
  if (!path.startsWith("/") || path.startsWith("//")) return false;
  // A browser reads a backslash in a path as a slash.
  return !path.includes("\\");
}

/**
 * A path taken from the address (such as `next`), when following it keeps the
 * user in the console; `null` otherwise. The path starts with one `/`, names
 * no other host in any encoding, and keeps its query string and hash.
 */
export function safePath(value: unknown): string | null {
  if (typeof value !== "string" || value.length > LONGEST_PATH) return null;
  let form = value;
  for (let round = 0; round <= DECODINGS; round += 1) {
    if (!staysHere(form)) return null;
    let decoded: string;
    try {
      decoded = decodePath(form);
    } catch {
      return null;
    }
    if (decoded === form) break;
    if (round === DECODINGS) return null;
    form = decoded;
  }
  // What the browser makes of it, as the last word.
  try {
    const { origin } = window.location;
    if (new URL(value, origin).origin !== origin) return null;
  } catch {
    return null;
  }
  return value;
}

/** Decodes the path and leaves the query string and the hash as they are. */
function decodePath(text: string): string {
  const path = pathOf(text);
  return decodeURIComponent(path) + text.slice(path.length);
}
