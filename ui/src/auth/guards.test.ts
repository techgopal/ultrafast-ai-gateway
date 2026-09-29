import { describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import {
  can,
  isAdmin,
  ledTeamIds,
  leads,
  safePath,
  type ConsoleAction,
  type Me,
} from "./guards";

// Platform is team 1, Research is team 2.
const platform = fixtures.teams.platform.id;
const research = fixtures.teams.research.id;
const growth = fixtures.teams.growth.id;

const admin: Me = fixtures.me.maya;
/** Lead of Platform, member of Research. */
const lead: Me = fixtures.me.arjun;
/** Member of Platform. */
const member: Me = fixtures.me.lena;
/** In no team. */
const loner: Me = fixtures.me.priya;

describe("guards", () => {
  test("the four people are who the table takes them for", () => {
    expect([platform, research]).toEqual([1, 2]);
    expect(admin.user.role).toBe("admin");
    expect(lead.teams.map((t) => [t.team_id, t.role])).toEqual([
      [1, "lead"],
      [2, "member"],
    ]);
    expect(member.teams.map((t) => [t.team_id, t.role])).toEqual([[1, "member"]]);
    expect(loner.teams).toEqual([]);
    expect([lead, member, loner].map((me) => me.user.role)).toEqual(["member", "member", "member"]);
  });

  test("isAdmin, leads and ledTeamIds", () => {
    expect([admin, lead, member, loner].map(isAdmin)).toEqual([true, false, false, false]);
    expect(leads(lead, platform)).toBe(true);
    expect(leads(lead, research)).toBe(false);
    expect(leads(lead, growth)).toBe(false);
    expect(leads(member, platform)).toBe(false);
    expect(leads(loner, platform)).toBe(false);
    // An admin leads a team only when the team says so.
    expect(leads(admin, platform)).toBe(false);
    expect(ledTeamIds(lead)).toEqual([platform]);
    expect(ledTeamIds(admin)).toEqual([]);
    expect(ledTeamIds(member)).toEqual([]);
    expect(ledTeamIds(loner)).toEqual([]);
  });

  const leadId = lead.user.id;
  const memberId = member.user.id;
  const lonerId = loner.user.id;

  // Each row: the action, then what the admin, the lead, the member and the loner may do.
  const table: [string, ConsoleAction, [boolean, boolean, boolean, boolean]][] = [
    ["invite user", { type: "inviteUser" }, [true, false, false, false]],
    ["edit user role or status", { type: "editUserRoleOrStatus" }, [true, false, false, false]],
    ["delete user", { type: "deleteUser" }, [true, false, false, false]],
    ["rename the lead", { type: "renameUser", userId: leadId }, [true, true, false, false]],
    ["rename the member", { type: "renameUser", userId: memberId }, [true, false, true, false]],
    ["rename the loner", { type: "renameUser", userId: lonerId }, [true, false, false, true]],
    ["rename the admin", { type: "renameUser", userId: admin.user.id }, [true, false, false, false]],
    ["create team", { type: "createTeam" }, [true, false, false, false]],
    ["rename team 1", { type: "renameTeam", teamId: 1 }, [true, true, false, false]],
    ["rename team 2", { type: "renameTeam", teamId: 2 }, [true, false, false, false]],
    ["delete team 1", { type: "deleteTeam", teamId: 1 }, [true, false, false, false]],
    ["add member to team 1", { type: "addMember", teamId: 1 }, [true, true, false, false]],
    ["add member to team 2", { type: "addMember", teamId: 2 }, [true, false, false, false]],
    ["make lead in team 1", { type: "makeLead", teamId: 1 }, [true, false, false, false]],
    ["remove member from team 1", { type: "removeMember", teamId: 1 }, [true, true, false, false]],
    ["remove member from team 2", { type: "removeMember", teamId: 2 }, [true, false, false, false]],
    ["create key for self, no team", { type: "createKeyForSelf", teamId: null }, [true, true, true, true]],
    ["create key for self in team 1", { type: "createKeyForSelf", teamId: 1 }, [true, true, true, false]],
    ["create key for self in team 2", { type: "createKeyForSelf", teamId: 2 }, [true, true, false, false]],
    ["create key for self in team 3", { type: "createKeyForSelf", teamId: growth }, [true, false, false, false]],
    ["create key for a member of team 1", { type: "createKeyForMember", teamId: 1 }, [true, true, false, false]],
    ["create key for a member of team 2", { type: "createKeyForMember", teamId: 2 }, [true, false, false, false]],
    ["revoke the lead's own key", { type: "revokeKey", ownerId: leadId, teamId: null }, [true, true, false, false]],
    ["revoke the member's key of team 1", { type: "revokeKey", ownerId: memberId, teamId: 1 }, [true, true, true, false]],
    ["revoke the member's key without a team", { type: "revokeKey", ownerId: memberId, teamId: null }, [true, false, true, false]],
    ["revoke a key of team 2", { type: "revokeKey", ownerId: 4, teamId: 2 }, [true, false, false, false]],
    ["revoke the loner's key", { type: "revokeKey", ownerId: lonerId, teamId: null }, [true, false, false, true]],
    ["revoke a key whose owner is gone", { type: "revokeKey", ownerId: null, teamId: null }, [true, false, false, false]],
    ["manage providers", { type: "manageProviders" }, [true, false, false, false]],
    ["view audit", { type: "viewAudit" }, [true, false, false, false]],
  ];

  test.each(table)("%s", (_, action, expected) => {
    expect([admin, lead, member, loner].map((me) => can(me, action))).toEqual(expected);
  });

  test("the table has every action", () => {
    const seen = new Set(table.map(([, action]) => action.type));
    const all: Record<ConsoleAction["type"], true> = {
      inviteUser: true,
      editUserRoleOrStatus: true,
      deleteUser: true,
      renameUser: true,
      createTeam: true,
      renameTeam: true,
      deleteTeam: true,
      addMember: true,
      makeLead: true,
      removeMember: true,
      createKeyForSelf: true,
      createKeyForMember: true,
      revokeKey: true,
      manageProviders: true,
      viewAudit: true,
    };
    expect([...seen].sort()).toEqual(Object.keys(all).sort());
  });
});

describe("safePath", () => {
  test.each([
    ["/", "/"],
    ["/keys", "/keys"],
    ["/keys?status=active#x", "/keys?status=active#x"],
    ["/teams/3", "/teams/3"],
    ["/keys?q=%2F%2Fx", "/keys?q=%2F%2Fx"],
  ])("%s is kept", (given, expected) => {
    expect(safePath(given)).toBe(expected);
  });

  test.each([
    ["nothing", undefined],
    ["a number", 7],
    ["an empty text", ""],
    ["two slashes", "//evil.example"],
    ["three slashes", "///evil.example"],
    ["a slash and a backslash", "/\\evil.example"],
    ["a backslash further in the path", "/a\\b"],
    ["an absolute address", "https://evil.example"],
    ["a script", "javascript:alert(1)"],
    ["no slash at the start", "keys"],
    ["encoded slashes", "/%2F%2Fevil.example"],
    ["an encoded slash after the slash", "/%2Fevil.example"],
    ["an encoded backslash", "/%5Cevil.example"],
    ["twice encoded slashes", "/%252F%252Fevil.example"],
    ["encoded from the start", "%2F%2Fevil.example"],
    ["an encoding that is not valid", "/keys%E0%A4%A"],
    ["a newline", "/keys\n"],
    ["a tab between the slashes", "/\t/evil.example"],
    ["an encoded newline", "/keys%0A"],
    ["an encoded tab", "/%09/evil.example"],
    ["a carriage return", "/keys\r"],
    ["more than 2000 characters", `/${"a".repeat(2000)}`],
  ])("%s is refused", (_, given) => {
    expect(safePath(given)).toBeNull();
  });

  test("2000 characters are still taken", () => {
    const path = `/${"a".repeat(1999)}`;
    expect(safePath(path)).toBe(path);
  });
});
