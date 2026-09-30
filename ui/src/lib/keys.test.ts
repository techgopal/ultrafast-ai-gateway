import { describe, expect, test } from "vitest";
import { ApiError, ConsoleRefusal, NetworkError, SessionOverError } from "@/api/errors";
import * as fixtures from "@/test/fixtures";
import { NO_EXPIRY } from "./expiry";
import {
  choiceOffered,
  choiceShown,
  CHOOSE_A_TEAM,
  goneAmong,
  isAskedAgain,
  isFirstRead,
  isGone,
  isMissing,
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
  type TeamRead,
} from "./keys";

const { maya, arjun, lena, tomas, priya, sam, dana } = fixtures.users;
const { platform, research, growth } = fixtures.teams;
const everyTeam = () => true;
const allDetails = fixtures.teamDetailList;

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

  test("a team that is not there is not among them", () => {
    expect(ownTeams(fixtures.me.arjun, (id) => id !== platform.id)).toEqual([RESEARCH]);
    expect(ownTeams(fixtures.me.arjun, () => false)).toEqual([]);
  });
});

describe("who can own a new key", () => {
  test("an admin chooses among the active users, themselves first, the others by name", () => {
    const owners = ownersFor(fixtures.me.maya, fixtures.userList, allDetails, everyTeam);
    // Not Sam, who is invited, and not Dana, who is disabled.
    expect(names(owners.people)).toEqual(names([maya, arjun, lena, priya, tomas]));
    expect(names(owners.people)).not.toContain(sam.name);
    expect(names(owners.people)).not.toContain(dana.name);
  });

  test("for an admin the teams of an owner are those the owner is a member of, by name, and none", () => {
    const owners = ownersFor(fixtures.me.maya, fixtures.userList, allDetails, everyTeam);
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

  test("a lead chooses themselves and the members of the teams they lead, which are read", () => {
    // Arjun leads Platform and is a member of Research: only Platform is read.
    const owners = ownersFor(
      fixtures.me.arjun,
      fixtures.userList,
      [fixtures.teamDetails.platform],
      everyTeam,
    );
    expect(names(owners.people)).toEqual(names([arjun, lena]));
    // A key of another user belongs to a team the lead leads: never to none.
    expect(owners.teamsOf(lena.id)).toEqual([PLATFORM]);
    expect(owners.withoutTeam(lena.id)).toBe(false);
    // A key of their own: any team they are in, or none.
    expect(owners.teamsOf(arjun.id)).toEqual([PLATFORM, RESEARCH]);
    expect(owners.withoutTeam(arjun.id)).toBe(true);
  });

  test("a lead whose team was not read is offered alone, with their own teams", () => {
    const owners = ownersFor(fixtures.me.arjun, fixtures.userList, [], everyTeam);
    expect(names(owners.people)).toEqual([arjun.name]);
    expect(owners.teamsOf(arjun.id)).toEqual([PLATFORM, RESEARCH]);
  });

  test("a team of the viewer's own that is not there is no team of a key of their own", () => {
    const owners = ownersFor(fixtures.me.arjun, fixtures.userList, [], (id) => id !== platform.id);
    expect(owners.teamsOf(arjun.id)).toEqual([RESEARCH]);
    expect(owners.withoutTeam(arjun.id)).toBe(true);
  });

  test("a member of a team that is open is an owner only while they are active", () => {
    const disabled: fixtures.User = { ...lena, status: "disabled" };
    const owners = ownersFor(
      fixtures.me.arjun,
      [arjun, disabled],
      [fixtures.teamDetails.platform],
      everyTeam,
    );
    expect(names(owners.people)).toEqual([arjun.name]);
  });
});

/** What the form of an admin is offered, with the teams of the fixtures. */
function adminOwners(users: readonly fixtures.User[] = fixtures.userList): Owners {
  return ownersFor(fixtures.me.maya, users, allDetails, everyTeam);
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
    const owners = ownersFor(
      fixtures.me.maya,
      fixtures.userList,
      [fixtures.teamDetails.platform],
      everyTeam,
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
    const owners = ownersFor(
      fixtures.me.arjun,
      [arjun],
      [fixtures.teamDetails.platform],
      everyTeam,
    );
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
  };

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

describe("what is known of a team that was asked for", () => {
  const detail = fixtures.teamDetails.research;
  const notFound = new ApiError(404, "not_found", "Not found.");
  const failed = new ApiError(500, "internal_error", "Something went wrong.");

  const reads = {
    "asked for, for the first time": { data: undefined, error: null, errorUpdateCount: 0 },
    read: { data: detail, error: null, errorUpdateCount: 0 },
    "read, after a failure": { data: detail, error: null, errorUpdateCount: 2 },
    "read before, and the read again failed": { data: detail, error: failed, errorUpdateCount: 1 },
    "answered 404": { data: undefined, error: notFound, errorUpdateCount: 1 },
    "answered 500": { data: undefined, error: failed, errorUpdateCount: 1 },
    "not reached": { data: undefined, error: new NetworkError(), errorUpdateCount: 1 },
    "answered for a session that is over": {
      data: undefined,
      error: new SessionOverError(),
      errorUpdateCount: 1,
    },
    "asked for again after a failure": { data: undefined, error: null, errorUpdateCount: 1 },
  } satisfies Record<string, TeamRead>;

  // gone, first read, asked again, missing
  const expected: Record<keyof typeof reads, [boolean, boolean, boolean, boolean]> = {
    "asked for, for the first time": [false, true, false, false],
    read: [false, false, false, false],
    "read, after a failure": [false, false, false, false],
    "read before, and the read again failed": [false, false, false, false],
    "answered 404": [true, false, false, false],
    "answered 500": [false, false, false, true],
    "not reached": [false, false, false, true],
    "answered for a session that is over": [false, false, false, false],
    "asked for again after a failure": [false, false, true, true],
  };

  test.each(Object.keys(reads) as (keyof typeof reads)[])("a team that is %s", (name) => {
    const read: TeamRead = reads[name];
    expect([isGone(read), isFirstRead(read), isAskedAgain(read), isMissing(read)]).toEqual(
      expected[name],
    );
  });

  test("a 404 is no read that failed: a team that is gone is not missing", () => {
    expect(isGone(reads["answered 404"])).toBe(true);
    expect(isMissing(reads["answered 404"])).toBe(false);
  });
});

describe("the teams that are gone", () => {
  const notFound = new ApiError(404, "not_found", "Not found.");
  const gone: TeamRead = { data: undefined, error: notFound, errorUpdateCount: 1 };
  const again: TeamRead = { data: undefined, error: null, errorUpdateCount: 1 };
  const first: TeamRead = { data: undefined, error: null, errorUpdateCount: 0 };
  const read: TeamRead = { data: fixtures.teamDetails.platform, error: null, errorUpdateCount: 0 };
  const failed: TeamRead = {
    data: undefined,
    error: new ApiError(500, "internal_error", "Something went wrong."),
    errorUpdateCount: 2,
  };
  const ids = [platform.id, research.id, growth.id];

  test("are those that answer 404", () => {
    expect(goneAmong(ids, [read, gone, read], [])).toEqual([research.id]);
    expect(goneAmong(ids, [gone, gone, read], [])).toEqual([platform.id, research.id]);
    expect(goneAmong(ids, [read, read, read], [])).toEqual([]);
  });

  test("a team that was gone stays gone while it is asked for again", () => {
    expect(goneAmong(ids, [read, again, read], [research.id])).toEqual([research.id]);
  });

  test("a team that is asked for again after another failure is not gone", () => {
    expect(goneAmong(ids, [read, again, read], [])).toEqual([]);
    expect(goneAmong(ids, [read, again, read], [platform.id])).toEqual([]);
  });

  test("a team that was gone and answers is gone no more, whatever it answers", () => {
    expect(goneAmong(ids, [read, read, read], [research.id])).toEqual([]);
    expect(goneAmong(ids, [read, failed, read], [research.id])).toEqual([]);
  });

  test("a team that is asked for, for the first time, is not gone, also if one of its id was", () => {
    expect(goneAmong(ids, [read, first, read], [research.id])).toEqual([]);
  });

  test("a team that is asked for no more is not among them", () => {
    expect(goneAmong([platform.id], [read], [research.id])).toEqual([]);
    // More ids than reads: what is not read is not gone.
    expect(goneAmong(ids, [gone], [])).toEqual([platform.id]);
  });
});
