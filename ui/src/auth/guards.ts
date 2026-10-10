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
  /**
   * Reads the list of every user, to choose one from (as when a member is
   * added to a team). The gateway lists every user to an admin only: a
   * lead gets the users of the teams they lead, anybody else themselves.
   */
  | { type: "listAllUsers" }
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
  /**
   * Removes a user from the team. A lead removes members and may leave; only
   * an admin removes another lead. `role` is the role of the user in the team.
   */
  | { type: "removeMember"; teamId: number; userId: number; role: string }
  /** `teamId` is `null` for a key that belongs to no team. */
  | { type: "createKeyForSelf"; teamId: number | null }
  /** For another user: only in a team the caller leads. */
  | { type: "createKeyForMember"; teamId: number }
  /**
   * For this other user in this team: a lead makes one only for a member of
   * a team they lead who is neither one of its leads nor an admin. Such a key
   * calls only what is granted to everyone or to the team. `ownerRole` is the
   * user's role, `ownerTeamRole` their role in the team.
   */
  | { type: "createKeyForUser"; teamId: number; ownerRole: string; ownerTeamRole: string }
  /** For any user, in a team of theirs or in none: what only an admin may. */
  | { type: "createKeyForAnyone" }
  | { type: "revokeKey"; ownerId: number | null; teamId: number | null }
  /** Replaces the tags of a key: admins only, as in the gateway; the creator sets them when the key is made. */
  | { type: "editKeyTags"; ownerId: number | null; teamId: number | null }
  | { type: "manageProviders" }
  /** Adds, syncs, enables, grants and deletes models: what only an admin may. */
  | { type: "manageModels" }
  /** Creates, changes and deletes routes: what only an admin may. Everybody sees the routes they may use. */
  | { type: "manageRoutes" }
  /** Reads the health of the targets of the routes. */
  | { type: "viewRoutingHealth" }
  | { type: "viewAudit" }
  /**
   * Sets and deletes rate limits and budgets, and changes the settings: what
   * only an admin may. Everybody else reads the limits and budgets that apply
   * to them, and nothing else.
   */
  | { type: "manageLimits" }
  | { type: "manageBudgets" }
  | { type: "manageSettings" }
  /** Sees and changes alert rules and channels, and reads their history: what only an admin may. */
  | { type: "manageAlerts" }
  /** Sees, changes and attaches guardrails (to routes and keys), and tries them: what only an admin may. */
  | { type: "manageGuardrails" }
  /**
   * Sees how many users and teams there are, on the overview: who manages
   * some, which is an admin and the lead of a team. The gateway lists users
   * and teams for everybody, as far as they are theirs to see; to a member
   * that is themselves and their own teams, which is nothing to count.
   */
  | { type: "viewUserAndTeamCounts" }
  /**
   * Sees the calls and the usage of more than themselves: an admin sees all,
   * the lead of a team its people. A member sees their own only, so the logs
   * offer them no filter by key, user or team, and the overview no top keys.
   */
  | { type: "viewOthersUsage" }
  /** Makes a prompt template: an admin, and the lead of any team. Everybody signed in reads and uses them. */
  | { type: "createPrompt" }
  /** Adds a version to a template or deletes it: an admin, and its maker while they lead a team. */
  | { type: "managePrompt"; createdBy: number | null }
  /** Makes chat calls in the playground, as a key of their own would: everybody signed in. */
  | { type: "usePlayground" };

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
    case "listAllUsers":
    case "editUserRoleOrStatus":
    case "deleteUser":
    case "createTeam":
    case "deleteTeam":
    case "makeLead":
    case "createKeyForAnyone":
    case "manageProviders":
    case "manageModels":
    case "manageRoutes":
    case "viewRoutingHealth":
    case "viewAudit":
    case "manageLimits":
    case "manageBudgets":
    case "manageSettings":
    case "manageAlerts":
    case "manageGuardrails":
    case "editKeyTags":
      return false;
    case "renameUser":
      return action.userId === me.user.id;
    case "renameTeam":
    case "addMember":
    case "createKeyForMember":
      return leads(me, action.teamId);
    case "createKeyForUser":
      return (
        leads(me, action.teamId) && action.ownerRole !== "admin" && action.ownerTeamRole === "member"
      );
    case "removeMember":
      return (
        leads(me, action.teamId) && (action.role !== "lead" || action.userId === me.user.id)
      );
    case "viewUserAndTeamCounts":
    case "viewOthersUsage":
      return ledTeamIds(me).length > 0;
    case "usePlayground":
      return true;
    case "createPrompt":
      return ledTeamIds(me).length > 0;
    case "managePrompt":
      return action.createdBy === me.user.id && ledTeamIds(me).length > 0;
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
