import { describe, expect, test } from "vitest";
import { ApiError, ConsoleRefusal } from "@/api/errors";
import * as fixtures from "@/test/fixtures";
import { NO_EXPIRY } from "./expiry";
import {
  allowedOf,
  allowedSummary,
  callableItems,
  CHOOSE_A_TEAM,
  CHOOSE_ALLOWED,
  choiceOffered,
  choiceShown,
  ownerOffered,
  ownersFor,
  ownTeams,
  requestOf,
  teamOffered,
  teamOfNewOwner,
  WITHOUT_TEAM,
  type KeyValues,
  type Owners,
  type TeamChoices,
} from "./keys";

const { maya, arjun, lena, tomas, priya, sam, dana } = fixtures.users;
const { platform, research, growth } = fixtures.teams;

const PLATFORM = { id: platform.id, name: platform.name };
const RESEARCH = { id: research.id, name: research.name };

function names(users: readonly { name: string }[]): string[] {
  return users.map((user) => user.name);
}

describe("the teams of a key of one's own", () => {
  test("are the teams the viewer is in, in any role", () => {
    expect(ownTeams(fixtures.me.arjun)).toEqual([PLATFORM, RESEARCH]);
    expect(ownTeams(fixtures.me.lena)).toEqual([PLATFORM]);
    expect(ownTeams(fixtures.me.maya)).toEqual([]);
  });
});

describe("who can own a new key", () => {
  test("an admin chooses among the active users, themselves first, the others by name", () => {
    const owners = ownersFor(fixtures.me.maya, fixtures.userList);
    // Not Sam, who is invited, and not Dana, who is disabled.
    expect(names(owners.people)).toEqual(names([maya, arjun, lena, priya, tomas]));
    expect(names(owners.people)).not.toContain(sam.name);
    expect(names(owners.people)).not.toContain(dana.name);
  });

  test("for an admin the teams of an owner are those the user list gives them, by name, and none", () => {
    const owners = ownersFor(fixtures.me.maya, fixtures.userList);
    expect(owners.teamsOf(arjun.id)).toEqual([PLATFORM, RESEARCH]);
    expect(owners.teamsOf(tomas.id)).toEqual([RESEARCH]);
    expect(owners.teamsOf(priya.id)).toEqual([]);
    expect(owners.withoutTeam(arjun.id)).toBe(true);
    expect(owners.withoutTeam(priya.id)).toBe(true);
    // The admin is in no team: a key of their own has none.
    expect(owners.teamsOf(maya.id)).toEqual([]);
    expect(owners.withoutTeam(maya.id)).toBe(true);
    expect(growth.member_count).toBe(0);
  });

  test("the teams come from the users: nothing else is read", () => {
    // A team the user list gives Priya is one of hers, whatever else is known.
    const priyaInGrowth: fixtures.User = {
      ...priya,
      teams: [{ team_id: growth.id, name: growth.name, role: "member" }],
    };
    const owners = ownersFor(fixtures.me.maya, [maya, priyaInGrowth]);
    expect(owners.teamsOf(priya.id)).toEqual([{ id: growth.id, name: growth.name }]);
  });

  test("a lead chooses themselves and the users who are in a team they lead", () => {
    // Arjun leads Platform and is a member of Research: Tomas is in Research only.
    const owners = ownersFor(fixtures.me.arjun, fixtures.userList);
    expect(names(owners.people)).toEqual(names([arjun, lena]));
    // A key of another user belongs to a team the lead leads: never to none.
    expect(owners.teamsOf(lena.id)).toEqual([PLATFORM]);
    expect(owners.withoutTeam(lena.id)).toBe(false);
    // A key of their own: any team they are in, or none.
    expect(owners.teamsOf(arjun.id)).toEqual([PLATFORM, RESEARCH]);
    expect(owners.withoutTeam(arjun.id)).toBe(true);
  });

  test("a lead is not offered a team they only belong to, for another user", () => {
    const owners = ownersFor(fixtures.me.arjun, fixtures.userList);
    expect(owners.teamsOf(tomas.id)).toEqual([]);
  });

  test("a lead who leads nothing is offered alone, with their own teams", () => {
    const owners = ownersFor(fixtures.me.arjun, []);
    expect(names(owners.people)).toEqual([arjun.name]);
    expect(owners.teamsOf(arjun.id)).toEqual([PLATFORM, RESEARCH]);
  });

  test("a user of a team is an owner only while they are active", () => {
    const disabled: fixtures.User = { ...lena, status: "disabled" };
    const owners = ownersFor(fixtures.me.arjun, [arjun, disabled]);
    expect(names(owners.people)).toEqual([arjun.name]);
  });
});

/** What the form of an admin is offered, with the teams of the fixtures. */
function adminOwners(users: readonly fixtures.User[] = fixtures.userList): Owners {
  return ownersFor(fixtures.me.maya, users);
}

function teamsFor(owners: Owners) {
  return (ownerId: string): TeamChoices => ({
    teams: owners.teamsOf(Number(ownerId)),
    none: owners.withoutTeam(Number(ownerId)),
  });
}

describe("the owner that is chosen", () => {
  test("stays while they are offered", () => {
    expect(ownerOffered(String(tomas.id), fixtures.me.maya, adminOwners())).toBe(String(tomas.id));
  });

  test("is the viewer again when they are offered no more", () => {
    const without = adminOwners(fixtures.userList.filter((user) => user.id !== tomas.id));
    expect(ownerOffered(String(tomas.id), fixtures.me.maya, without)).toBe(String(maya.id));
    expect(ownerOffered("", fixtures.me.maya, without)).toBe(String(maya.id));
  });

  test("stays as it is while the owners are not known", () => {
    expect(ownerOffered(String(tomas.id), fixtures.me.maya, null)).toBe(String(tomas.id));
  });
});

describe("the team that is chosen", () => {
  const both: TeamChoices = { teams: [PLATFORM, RESEARCH], none: true };

  test("stays while it is offered", () => {
    expect(teamOffered(String(research.id), both)).toBe(String(research.id));
  });

  test("what was not chosen stays so", () => {
    expect(teamOffered(WITHOUT_TEAM, both)).toBe(WITHOUT_TEAM);
    expect(teamOffered("", both)).toBe("");
    expect(teamOffered("", { teams: [PLATFORM], none: false })).toBe("");
  });

  test("a team that is offered no more gives way to no team, where a key can have none", () => {
    expect(teamOffered(String(research.id), { teams: [PLATFORM], none: true })).toBe(WITHOUT_TEAM);
    expect(teamOffered(String(research.id), { teams: [], none: true })).toBe(WITHOUT_TEAM);
  });

  test("where a key must have a team, to the first team that is left, or to none chosen", () => {
    expect(teamOffered(String(research.id), { teams: [PLATFORM], none: false })).toBe(
      String(platform.id),
    );
    expect(teamOffered(String(research.id), { teams: [], none: false })).toBe("");
  });

  test("the team of an owner who was just chosen: none where that can be, otherwise none is chosen yet", () => {
    expect(teamOfNewOwner({ teams: [PLATFORM], none: true })).toBe(WITHOUT_TEAM);
    expect(teamOfNewOwner({ teams: [PLATFORM], none: false })).toBe("");
  });
});

describe("the choice as it is offered", () => {
  test("a choice that is offered is what it is", () => {
    const owners = adminOwners();
    const chosen = { owner_id: String(tomas.id), team_id: String(research.id) };
    expect(choiceOffered(chosen, fixtures.me.maya, owners, teamsFor(owners))).toEqual(chosen);
  });

  test("a team that is offered no more goes, and the owner stays", () => {
    // Tomas is not in Research any more, as the user list now says.
    const owners = ownersFor(
      fixtures.me.maya,
      fixtures.userList.map((user) => (user.id === tomas.id ? { ...user, teams: [] } : user)),
    );
    expect(
      choiceOffered(
        { owner_id: String(tomas.id), team_id: String(research.id) },
        fixtures.me.maya,
        owners,
        teamsFor(owners),
      ),
    ).toEqual({ owner_id: String(tomas.id), team_id: WITHOUT_TEAM });
  });

  test("an owner who is offered no more takes the team with them, also a team the viewer is in", () => {
    // Arjun chose Lena and Platform. Lena is gone; Arjun is in Platform himself.
    const owners = ownersFor(fixtures.me.arjun, [arjun]);
    expect(owners.teamsOf(arjun.id)).toContainEqual(PLATFORM);
    expect(
      choiceOffered(
        { owner_id: String(lena.id), team_id: String(platform.id) },
        fixtures.me.arjun,
        owners,
        teamsFor(owners),
      ),
    ).toEqual({ owner_id: String(arjun.id), team_id: WITHOUT_TEAM });
  });

  test("it is a choice that is offered: offered again, it is the same", () => {
    const owners = adminOwners(fixtures.userList.filter((user) => user.id !== tomas.id));
    const once = choiceOffered(
      { owner_id: String(tomas.id), team_id: String(research.id) },
      fixtures.me.maya,
      owners,
      teamsFor(owners),
    );
    expect(once).toEqual({ owner_id: String(maya.id), team_id: WITHOUT_TEAM });
    expect(choiceOffered(once, fixtures.me.maya, owners, teamsFor(owners))).toEqual(once);
  });

  test("while the owners are not known the owner is left as it is", () => {
    const own = (): TeamChoices => ({ teams: [], none: true });
    expect(
      choiceOffered(
        { owner_id: String(tomas.id), team_id: WITHOUT_TEAM },
        fixtures.me.maya,
        null,
        own,
      ),
    ).toEqual({ owner_id: String(tomas.id), team_id: WITHOUT_TEAM });
  });
});

describe("the choice the form shows and holds", () => {
  // Without the owners, the teams of an owner are the viewer's own.
  const own = (): TeamChoices => ({ teams: [PLATFORM], none: true });
  const chosen = { owner_id: String(tomas.id), team_id: String(research.id) };

  test("while the owners are read for who chooses them, it is the choice as it is: a team of another owner is not judged by the viewer's own teams", () => {
    expect(choiceShown(chosen, fixtures.me.maya, null, own, true)).toEqual(chosen);
  });

  test("otherwise it is the choice as it is offered", () => {
    expect(choiceShown(chosen, fixtures.me.maya, null, own, false)).toEqual({
      owner_id: String(tomas.id),
      team_id: WITHOUT_TEAM,
    });
    const owners = adminOwners();
    expect(choiceShown(chosen, fixtures.me.maya, owners, teamsFor(owners), false)).toEqual(chosen);
  });
});

describe("the request for a key", () => {
  const values: KeyValues = {
    name: "laptop",
    owner_id: String(maya.id),
    team_id: WITHOUT_TEAM,
    expires_at: NO_EXPIRY,
    allow: "all",
    allowed: [],
    tags: [],
  };

  test("the tags are sent as an object, and a key with none sends none", () => {
    expect(
      requestOf({ ...values, tags: [{ name: "team", value: "a" }, { name: "", value: "" }] }, fixtures.me.maya),
    ).toEqual({ name: "laptop", tags: { team: "a" } });
    expect(requestOf({ ...values, tags: [{ name: "", value: "" }] }, fixtures.me.maya)).toEqual({
      name: "laptop",
    });
  });

  test("tags the gateway would refuse are refused by the console, on the field", () => {
    const refusal = (() => {
      try {
        requestOf({ ...values, tags: [{ name: "a b", value: "v" }] }, fixtures.me.maya);
      } catch (error) {
        return error;
      }
      return null;
    })();
    expect(refusal).toBeInstanceOf(ConsoleRefusal);
    expect(refusal).toMatchObject({ field: "tags" });
  });

  test("a key of one's own without a team and an expiry is its name", () => {
    expect(requestOf(values, fixtures.me.maya)).toEqual({ name: "laptop" });
  });

  test("another owner, a team and an expiry are sent as the gateway takes them", () => {
    expect(
      requestOf(
        {
          name: "notebook",
          owner_id: String(tomas.id),
          team_id: String(research.id),
          expires_at: { choice: "date", day: "2027-01-31" },
          allow: "all",
          allowed: [],
          tags: [],
        },
        fixtures.me.maya,
      ),
    ).toEqual({
      name: "notebook",
      owner_id: tomas.id,
      team_id: research.id,
      expires_at: "2027-01-31 23:59:59",
    });
  });

  test("the viewer is not sent as the owner: without one the gateway takes the caller", () => {
    const own = { ...values, owner_id: String(arjun.id), team_id: String(platform.id) };
    expect(requestOf(own, fixtures.me.arjun)).toEqual({ name: "laptop", team_id: platform.id });
    // The same values in the form of another viewer name the owner.
    expect(requestOf(own, fixtures.me.maya)).toEqual({
      name: "laptop",
      owner_id: arjun.id,
      team_id: platform.id,
    });
  });

  test.each(["", "abc", "0", "1.5"])("a team that is no id (%j) is refused by the console, on the team", (team) => {
    let refusal: unknown;
    try {
      requestOf({ ...values, team_id: team }, fixtures.me.maya);
    } catch (error) {
      refusal = error;
    }
    expect(refusal).toBeInstanceOf(ConsoleRefusal);
    expect(refusal).toMatchObject({ message: CHOOSE_A_TEAM, field: "team_id" });
    expect(refusal).not.toBeInstanceOf(ApiError);
  });
});

describe("the models a key may call", () => {
  const offered = ["openai/gpt-4o-mini", "openai/gpt-4o", "support"];

  test("all models I can use sends no list", () => {
    expect(allowedOf("all", ["openai/gpt-4o"], offered)).toBeUndefined();
    expect(allowedOf("all", [], null)).toBeUndefined();
  });

  test("a choice is sent as the names, in the order the list offers them", () => {
    expect(allowedOf("some", ["support", "openai/gpt-4o-mini"], offered)).toEqual([
      "openai/gpt-4o-mini",
      "support",
    ]);
  });

  test("a name that is offered no more is not sent", () => {
    expect(allowedOf("some", ["openai/gpt-4o", "retired/model"], offered)).toEqual([
      "openai/gpt-4o",
    ]);
  });

  test.each([
    ["nothing is chosen", [], offered],
    ["everything chosen is offered no more", ["retired/model"], offered],
    ["the lists are not known", ["openai/gpt-4o"], null],
  ])("a list is refused by the console, on the field, when %s", (_, chosen, list) => {
    let refusal: unknown;
    try {
      allowedOf("some", chosen, list);
    } catch (error) {
      refusal = error;
    }
    expect(refusal).toBeInstanceOf(ConsoleRefusal);
    expect(refusal).toMatchObject({ message: CHOOSE_ALLOWED, field: "allowed" });
    expect(refusal).not.toBeInstanceOf(ApiError);
  });

  test("the request carries the list, and nothing without one", () => {
    const values: KeyValues = {
      name: "laptop",
      owner_id: String(maya.id),
      team_id: WITHOUT_TEAM,
      expires_at: NO_EXPIRY,
      allow: "some",
      allowed: ["support"],
      tags: [],
    };
    expect(requestOf(values, fixtures.me.maya, offered)).toEqual({
      name: "laptop",
      allowed: ["support"],
    });
    expect(requestOf({ ...values, allow: "all" }, fixtures.me.maya, offered)).toEqual({
      name: "laptop",
    });
  });

  test("the list says All, or how many", () => {
    expect(allowedSummary(null)).toBe("All");
    expect(allowedSummary(["a/b"])).toBe("1");
    expect(allowedSummary(["a/b", "support", "c/d"])).toBe("3");
  });
});

describe("the models and routes a key can be limited to", () => {
  test("are the enabled models, then the routes, each by its name", () => {
    const items = callableItems(fixtures.modelList, fixtures.routeList);
    // The disabled model cannot be called, so it is no choice.
    expect(items.map((item) => item.id)).toEqual([
      "local-llm/llama3.1:8b",
      "openai/gpt-4o",
      "openai/gpt-4o-mini",
      "legacy.v1",
      "research",
      "support-chat",
    ]);
    expect(items.map((item) => item.route)).toEqual([false, false, false, true, true, true]);
  });

  test("a name is there once", () => {
    expect(callableItems([fixtures.models.openaiMini, fixtures.models.openaiMini], [])).toHaveLength(1);
  });
});
