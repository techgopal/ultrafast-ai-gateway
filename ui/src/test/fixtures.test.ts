// The fixtures imitate what the gateway produces. These tests pin the forms,
// each read from the gateway source named beside it, so that a fixture
// cannot drift away from them.
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import * as fixtures from "./fixtures";

// What is past and what is to come is said of the time of the fixtures, not
// of the day the tests run on.
beforeEach(() => {
  vi.setSystemTime(new Date(fixtures.now));
});

afterEach(() => {
  vi.useRealTimers();
});

// crates/gateway/src/secrets.rs, generate_secret: prefix + 32 random bytes as hex.
const SECRET = /^uf-(sk|at|inv)-[0-9a-f]{64}$/;
// The same function: prefix + U+2026 + the last 4 characters of the secret.
const DISPLAY = /^uf-(sk|at)-…[0-9a-f]{4}$/;
// crates/gateway/src/api/users.rs, INVITE_PAGE and new_invite.
const INVITE_LINK = /^\/accept-invite\?token=uf-inv-[0-9a-f]{64}$/;
// crates/gateway/src/store/mod.rs, TIMESTAMP: UTC, compared as text.
const TIMESTAMP = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/;
// crates/gateway/src/store/sessions.rs, random_hex.
const CSRF_TOKEN = /^[0-9a-f]{64}$/;

/** Every action the gateway writes to the audit log, with its target type. */
const AUDIT_ACTIONS: Readonly<Record<string, string>> = {
  // crates/gateway/src/api/auth.rs
  "setup.create_admin": "user",
  "auth.login": "user",
  "auth.logout": "user",
  "user.accept_invite": "user",
  "user.change_password": "user",
  // crates/gateway/src/api/users.rs
  "user.invite": "user",
  "user.reinvite": "user",
  "user.update": "user",
  "user.delete": "user",
  // crates/gateway/src/api/teams.rs
  "team.create": "team",
  "team.rename": "team",
  "team.delete": "team",
  "team.member_put": "team",
  "team.member_remove": "team",
  // crates/gateway/src/api/keys.rs
  "key.create": "key",
  "key.revoke": "key",
  // crates/gateway/src/api/providers.rs
  "provider.create": "provider",
  "provider.update": "provider",
  "provider.delete": "provider",
  // crates/gateway/src/api/tokens.rs
  "token.create": "token",
  "token.revoke": "token",
};

// crates/gateway/src/api/keys.rs, key_status.
const KEY_STATUSES = ["active", "suspended", "expired", "revoked"];

/** The time in UTC in the form of the gateway, which compares timestamps as text. */
function utc(time: Date): string {
  return time.toISOString().slice(0, 19).replace("T", " ");
}

/** The time it is, which is the time of the fixtures: the clock is pinned. */
function now(): string {
  return utc(new Date());
}

/** Every timestamp of the fixtures, with where it is. */
function timestamps(): [string, unknown][] {
  return timestampsIn(
    {
      users: fixtures.userList,
      teams: fixtures.teamList,
      teamDetails: fixtures.teamDetailList,
      me: fixtures.me,
      keys: fixtures.keyList,
      providers: fixtures.providerList,
      tokens: fixtures.tokenList,
      audit: fixtures.auditEntries,
    },
    "fixtures",
  );
}

/** Every value of a field whose name ends in `_at` or is `at`, anywhere in the value. */
function timestampsIn(value: unknown, path: string): [string, unknown][] {
  if (Array.isArray(value)) {
    return value.flatMap((item, index) => timestampsIn(item, `${path}[${index}]`));
  }
  if (typeof value !== "object" || value === null) return [];
  return Object.entries(value).flatMap(([name, inner]): [string, unknown][] => {
    const here = `${path}.${name}`;
    if (name === "at" || name.endsWith("_at")) return inner === null ? [] : [[here, inner]];
    return timestampsIn(inner, here);
  });
}

describe("the fixtures have the forms of the gateway", () => {
  test("the patterns see what they should", () => {
    expect(DISPLAY.test("uf-sk-…7d2f")).toBe(true);
    expect(DISPLAY.test("uf-sk-...7d2f")).toBe(false);
    expect(DISPLAY.test("uf-inv-…7d2f")).toBe(false);
    expect(SECRET.test(`uf-sk-${"a".repeat(63)}`)).toBe(false);
    expect(SECRET.test(`uf-sk-${"A".repeat(64)}`)).toBe(false);
    expect(TIMESTAMP.test("2026-09-01T09:00:00Z")).toBe(false);
    expect(timestampsIn({ a: [{ created_at: "x", b: { at: "y", c_at: null } }] }, "f")).toEqual([
      ["f.a[0].created_at", "x"],
      ["f.a[0].b.at", "y"],
    ]);
  });

  test("secrets that are shown once", () => {
    expect(fixtures.newKeySecret).toMatch(SECRET);
    expect(fixtures.newKeySecret.startsWith("uf-sk-")).toBe(true);
    expect(fixtures.newTokenSecret).toMatch(SECRET);
    expect(fixtures.newTokenSecret.startsWith("uf-at-")).toBe(true);
    expect(fixtures.newInviteToken).toMatch(SECRET);
    expect(fixtures.newInviteToken.startsWith("uf-inv-")).toBe(true);
    expect(fixtures.newInviteLink).toMatch(INVITE_LINK);
    expect(fixtures.newInviteLink.endsWith(fixtures.newInviteToken)).toBe(true);
    expect(fixtures.csrfToken).toMatch(CSRF_TOKEN);
  });

  test("the display of keys and tokens", () => {
    expect(fixtures.keyList.length).toBeGreaterThan(0);
    for (const key of fixtures.keyList) {
      expect(key.display, key.name).toMatch(DISPLAY);
      expect(key.display.startsWith("uf-sk-"), key.name).toBe(true);
    }
    expect(fixtures.tokenList.length).toBeGreaterThan(0);
    for (const token of fixtures.tokenList) {
      expect(token.display, token.name).toMatch(DISPLAY);
      expect(token.display.startsWith("uf-at-"), token.name).toBe(true);
    }
    const displays = [...fixtures.keyList, ...fixtures.tokenList].map((item) => item.display);
    expect(new Set(displays).size).toBe(displays.length);
    // The key and the token that the create calls answer with belong to the secret shown.
    expect(fixtures.keys.active.display.slice(-4)).toBe(fixtures.newKeySecret.slice(-4));
    expect(fixtures.tokens.active.display.slice(-4)).toBe(fixtures.newTokenSecret.slice(-4));
  });

  test("every timestamp", () => {
    const found = timestamps();
    expect(found.length).toBeGreaterThan(30);
    for (const [path, value] of found) {
      expect(value, path).toEqual(expect.stringMatching(TIMESTAMP));
      // A real date and time.
      const asDate = new Date(`${String(value).replace(" ", "T")}Z`);
      expect(asDate.toISOString().slice(0, 19).replace("T", " "), path).toBe(value);
    }
  });

  test("the clock of these tests is the time of the fixtures", () => {
    expect(fixtures.now).toBe("2026-09-30T12:00:00Z");
    expect(now()).toBe("2026-09-30 12:00:00");
  });

  // A time that is to come on the day a fixture is written, and near, is past
  // some day: a test that reads it through the clock of the machine then
  // fails by the calendar. What must not have come yet is far away, as in the
  // tests of the gateway (2999-01-01), and everything else is past already.
  test("a time that has not come is a hundred years away at least", () => {
    const at = now();
    const inAHundredYears = utc(new Date(Date.parse(fixtures.now) + 100 * 365.25 * 86_400_000));
    expect(inAHundredYears.slice(0, 4)).toBe("2126");
    const toCome = timestamps().filter(([, value]) => typeof value === "string" && value > at);
    // The one there is: the token that has an expiry and is not expired.
    expect(toCome.map(([path]) => path)).toEqual(["fixtures.tokens[1].expires_at"]);
    for (const [path, value] of toCome) {
      expect(typeof value === "string" && value >= inAHundredYears, `${path}: ${String(value)}`).toBe(
        true,
      );
    }
    expect(fixtures.farFuture >= inAHundredYears).toBe(true);
    expect(fixtures.farFuture).toMatch(TIMESTAMP);
    // The stretches of the audit log that the tests make are past as well.
    for (const entry of fixtures.auditEntriesFrom(2000, 3)) {
      expect(entry.at < at, String(entry.id)).toBe(true);
    }
  });

  test("audit entries", () => {
    const targets: Readonly<Record<string, readonly { id: number }[]>> = {
      user: fixtures.userList,
      team: fixtures.teamList,
      key: fixtures.keyList,
      provider: fixtures.providerList,
      token: fixtures.tokenList,
    };
    const emails = fixtures.userList.map((user) => user.email);
    expect(fixtures.auditEntries.length).toBeGreaterThan(0);
    for (const entry of fixtures.auditEntries) {
      expect(Object.keys(AUDIT_ACTIONS), entry.summary).toContain(entry.action);
      expect(entry.target_type, entry.action).toBe(AUDIT_ACTIONS[entry.action]);
      // The gateway names a target with every entry it writes.
      expect(entry.target_id, entry.action).not.toBeNull();
      expect(
        targets[entry.target_type]?.map((target) => target.id),
        entry.action,
      ).toContain(entry.target_id);
      expect(emails, entry.action).toContain(entry.actor_email);
    }
    // Newest first, with ids that fall.
    const ids = fixtures.auditEntries.map((entry) => entry.id);
    expect(ids).toEqual([...ids].sort((a, b) => b - a));
    const times = fixtures.auditEntries.map((entry) => entry.at);
    expect(times).toEqual([...times].sort().reverse());
  });

  test("the summaries of the audit entries", () => {
    const summaryOf = (action: string) =>
      fixtures.auditEntries.find((entry) => entry.action === action)?.summary;
    // crates/gateway/src/api/auth.rs, create_admin
    expect(summaryOf("setup.create_admin")).toBe("Created the first admin maya@example.test");
    // crates/gateway/src/api/users.rs, update_summary
    expect(summaryOf("user.update")).toBe(
      "Changed status of dana@example.test from active to disabled",
    );
    // crates/gateway/src/api/users.rs, invite
    expect(summaryOf("user.invite")).toBe("Invited sam@example.test as member");
    // crates/gateway/src/api/teams.rs, create
    expect(summaryOf("team.create")).toBe("Created team Growth");
    // crates/gateway/src/api/keys.rs, revoke
    expect(summaryOf("key.revoke")).toBe("Revoked key lena-old-laptop (uf-sk-…e41c)");
    // Each entry of the fixtures is one of the above.
    expect(fixtures.auditEntries).toHaveLength(5);
  });

  test("a stretch of the audit log that a test makes", () => {
    const entries = fixtures.auditEntriesFrom(120, 50);
    expect(entries).toHaveLength(50);
    // Newest first, with ids that fall by one: what `before` cuts is exact.
    expect(entries.map((entry) => entry.id)).toEqual(
      Array.from({ length: 50 }, (_, index) => 120 - index),
    );
    const times = entries.map((entry) => entry.at);
    expect(times).toEqual([...times].sort().reverse());
    expect(new Set(times).size).toBe(50);
    for (const entry of entries) {
      expect(entry.at, String(entry.id)).toMatch(TIMESTAMP);
      expect(entry.at < now(), String(entry.id)).toBe(true);
      expect(entry.target_type).toBe(AUDIT_ACTIONS[entry.action]);
      // crates/gateway/src/api/auth.rs, login
      expect(entry).toMatchObject({
        action: "auth.login",
        actor_email: "maya@example.test",
        target_id: fixtures.users.maya.id,
        summary: "maya@example.test signed in",
      });
    }
    // The ids of the gateway are positive.
    expect(entries.every((entry) => entry.id > 0)).toBe(true);
  });

  test("the status of keys", () => {
    const at = now();
    const byId = new Map(fixtures.userList.map((user) => [user.id, user]));
    for (const key of fixtures.keyList) {
      expect(KEY_STATUSES, key.name).toContain(key.status);
      // `revoked`, `expired`, `suspended` or `active`, the first that applies.
      const owner = key.owner_id === null ? undefined : byId.get(key.owner_id);
      const expected =
        key.revoked_at !== null
          ? "revoked"
          : key.expires_at !== null && key.expires_at <= at
            ? "expired"
            : owner !== undefined && owner.status !== "active"
              ? "suspended"
              : "active";
      expect(key.status, key.name).toBe(expected);
    }
    expect(fixtures.keyList.map((key) => key.status).sort()).toEqual(
      ["active", "active", "expired", "revoked", "suspended"].sort(),
    );

    const owner = fixtures.userList.find((user) => user.id === fixtures.keys.suspended.owner_id);
    expect(owner).toBeDefined();
    expect(owner?.status).not.toBe("active");

    const { expires_at: expired } = fixtures.keys.expired;
    expect(expired).not.toBeNull();
    expect(expired !== null && expired <= at).toBe(true);
    for (const key of fixtures.keyList.filter((k) => k.status === "active")) {
      expect(key.expires_at === null || key.expires_at > at, key.name).toBe(true);
      expect(key.revoked_at, key.name).toBeNull();
    }
    expect(fixtures.keys.revoked.revoked_at).not.toBeNull();
    expect(fixtures.keys.noOwner.owner_id).toBeNull();
    expect(fixtures.keys.noOwner.owner_email).toBeNull();
  });

  test("the tokens: one of each state the gateway tells apart", () => {
    // crates/gateway/src/store/sessions.rs: a token works while it is not revoked
    // and its time, when it has one, has not come.
    const at = now();
    const tokens: Record<keyof typeof fixtures.tokens, fixtures.Token> = fixtures.tokens;
    const { active, neverUsed, revoked, expired } = tokens;
    expect(fixtures.tokenList).toEqual([active, neverUsed, revoked, expired]);
    expect([active.revoked_at, active.expires_at]).toEqual([null, null]);
    expect(neverUsed.revoked_at).toBeNull();
    expect(neverUsed.expires_at !== null && neverUsed.expires_at > at).toBe(true);
    expect(neverUsed.last_used_at).toBeNull();
    expect(revoked.revoked_at).not.toBeNull();
    expect(expired.revoked_at).toBeNull();
    expect(expired.expires_at !== null && expired.expires_at <= at).toBe(true);
    const ids = fixtures.tokenList.map((token) => token.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  test("keys refer to users and teams that exist", () => {
    for (const key of fixtures.keyList) {
      if (key.owner_id === null) {
        expect(key.owner_email, key.name).toBeNull();
      } else {
        const owner = fixtures.userList.find((user) => user.id === key.owner_id);
        expect(owner, key.name).toBeDefined();
        expect(key.owner_email, key.name).toBe(owner?.email);
      }
      if (key.team_id === null) {
        expect(key.team_name, key.name).toBeNull();
      } else {
        const team = fixtures.teamList.find((t) => t.id === key.team_id);
        expect(team, key.name).toBeDefined();
        expect(key.team_name, key.name).toBe(team?.name);
      }
    }
  });

  test("teams, members and the caller's teams agree", () => {
    for (const detail of fixtures.teamDetailList) {
      expect(detail.team.member_count, detail.team.name).toBe(detail.members.length);
      for (const member of detail.members) {
        const user = fixtures.userList.find((u) => u.id === member.user_id);
        expect(user, member.email).toBeDefined();
        expect(member.email).toBe(user?.email);
        expect(member.name).toBe(user?.name);
        // crates/gateway/src/api/teams.rs: a disabled user cannot be added to a team.
        expect(user?.status, member.email).not.toBe("disabled");
      }
    }
    for (const me of Object.values(fixtures.me)) {
      const memberships = fixtures.teamDetailList.flatMap((detail) =>
        detail.members
          .filter((member) => member.user_id === me.user.id)
          .map((member) => ({
            team_id: detail.team.id,
            name: detail.team.name,
            role: member.role,
          })),
      );
      expect(me.teams, me.user.email).toEqual(memberships);
    }
  });

  test("users and providers", () => {
    for (const user of fixtures.userList) {
      // crates/gateway/src/identity/mod.rs, normalize_email: trimmed and lowercase.
      expect(user.email).toBe(user.email.trim().toLowerCase());
      // Nobody was active before they had a password.
      if (user.status === "invited") expect(user.last_active_at, user.email).toBeNull();
    }
    const emails = fixtures.userList.map((user) => user.email);
    expect(new Set(emails).size).toBe(emails.length);
    for (const provider of fixtures.providerList) {
      // crates/gateway/src/config.rs, validate_provider_name and validate_base_url;
      // crates/gateway/src/api/providers.rs: kind must be openai or anthropic.
      expect(provider.name).toMatch(/^[a-z0-9][a-z0-9_-]{0,39}$/);
      expect(["openai", "anthropic"]).toContain(provider.kind);
      expect(provider.base_url).toMatch(/^https?:\/\/[^/:@?#\s][^@?#\s]*$/);
    }
    expect(fixtures.providerList.map((p) => p.has_credential).sort()).toEqual([false, true]);
  });
});
