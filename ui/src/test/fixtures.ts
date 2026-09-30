// The people, teams, keys and the rest that the tests share. The fixtures
// imitate what the gateway produces (forms of secrets, timestamps, audit
// entries); `fixtures.test.ts` pins those forms. Every value has a type
// generated from the API description, so a change of the API that these do
// not follow is a compile error.
import type { components } from "@/api/schema";

type Schemas = components["schemas"];
export type User = Schemas["UserView"];
export type Team = Schemas["TeamSummary"];
export type TeamDetail = Schemas["TeamDetail"];
export type Key = Schemas["KeyView"];
export type Provider = Schemas["ProviderView"];
export type Token = Schemas["TokenView"];
export type AuditEntry = Schemas["AuditRow"];
export type Me = Schemas["MeResponse"];

/**
 * The time of the fixtures: what they call past (the expired key and token,
 * every `created_at`) lies before it, and so before every day the tests run
 * on. A test whose result depends on the time pins the clock to it
 * (`theClockIs` in `pages.ts`).
 */
export const now = "2026-09-30T12:00:00Z";

/**
 * A time that has not come, whenever the tests run: the one the tests of the
 * gateway use. A fixture that must not be past its time has this one. A
 * nearer time would hold only until its day, and then fail every test that
 * reads it through the clock of the machine.
 */
export const farFuture = "2999-01-01 00:00:00";

/** The CSRF token of the session in the tests: 64 hex characters, as the gateway makes it. */
export const csrfToken = "c5".repeat(32);

// Secrets that are shown once: a prefix and 64 hex characters, as the gateway
// makes them. They are made up, in a pattern no random secret has, and work nowhere.
export const newKeySecret = `uf-sk-${"0123456789abcdef".repeat(4)}`;
export const newTokenSecret = `uf-at-${"fedcba9876543210".repeat(4)}`;
export const newInviteToken = `uf-inv-${"00ff".repeat(16)}`;
export const newInviteLink = `/accept-invite?token=${newInviteToken}`;

/** What the gateway shows of a secret: the prefix, an ellipsis and the last 4 characters. */
function displayOf(prefix: "uf-sk-" | "uf-at-", last4: string): string {
  return `${prefix}\u2026${last4}`;
}

export const users = {
  /** An admin. */
  maya: {
    id: 1,
    email: "maya@example.test",
    name: "Maya Okafor",
    role: "admin",
    status: "active",
    created_at: "2026-06-01 09:00:00",
    last_active_at: "2026-09-28 16:20:00",
  },
  /** Lead of Platform, member of Research. */
  arjun: {
    id: 2,
    email: "arjun@example.test",
    name: "Arjun Mehta",
    role: "member",
    status: "active",
    created_at: "2026-06-03 10:00:00",
    last_active_at: "2026-09-27 11:05:00",
  },
  /** Member of Platform. */
  lena: {
    id: 3,
    email: "lena@example.test",
    name: "Lena Fischer",
    role: "member",
    status: "active",
    created_at: "2026-06-10 08:30:00",
    last_active_at: "2026-09-26 09:45:00",
  },
  /** Member of Research. */
  tomas: {
    id: 4,
    email: "tomas@example.test",
    name: "Tomas Novak",
    role: "member",
    status: "active",
    created_at: "2026-07-01 12:00:00",
    last_active_at: "2026-09-20 14:00:00",
  },
  /** In no team. */
  priya: {
    id: 5,
    email: "priya@example.test",
    name: "Priya Raman",
    role: "member",
    status: "active",
    created_at: "2026-07-15 12:00:00",
    last_active_at: null,
  },
  /** Invited, has not set a password yet. */
  sam: {
    id: 6,
    email: "sam@example.test",
    name: "Sam Carter",
    role: "member",
    status: "invited",
    created_at: "2026-09-25 15:00:00",
    last_active_at: null,
  },
  /** Disabled. Owns a key, which is suspended. */
  dana: {
    id: 7,
    email: "dana@example.test",
    name: "Dana Whitfield",
    role: "member",
    status: "disabled",
    created_at: "2026-06-20 10:00:00",
    last_active_at: "2026-08-01 10:00:00",
  },
} as const satisfies Record<string, User>;

export const userList: User[] = Object.values(users);

export const teams = {
  platform: { id: 1, name: "Platform", member_count: 2, created_at: "2026-06-02 09:00:00" },
  research: { id: 2, name: "Research", member_count: 2, created_at: "2026-06-05 09:00:00" },
  /** Has no members. */
  growth: { id: 3, name: "Growth", member_count: 0, created_at: "2026-09-01 09:00:00" },
} as const satisfies Record<string, Team>;

export const teamList: Team[] = Object.values(teams);

function member(user: User, role: Schemas["TeamRole"]): Schemas["MemberDetail"] {
  return { user_id: user.id, email: user.email, name: user.name, role };
}

export const teamDetails = {
  platform: {
    team: teams.platform,
    members: [member(users.arjun, "lead"), member(users.lena, "member")],
  },
  research: {
    team: teams.research,
    members: [member(users.arjun, "member"), member(users.tomas, "member")],
  },
  growth: { team: teams.growth, members: [] },
} satisfies Record<keyof typeof teams, TeamDetail>;

export const teamDetailList: TeamDetail[] = Object.values(teamDetails);

/** What `/api/auth/me` answers for each person who can sign in. */
export const me = {
  maya: { user: users.maya, teams: [], csrf_token: csrfToken },
  arjun: {
    user: users.arjun,
    teams: [
      { team_id: teams.platform.id, name: teams.platform.name, role: "lead" },
      { team_id: teams.research.id, name: teams.research.name, role: "member" },
    ],
    csrf_token: csrfToken,
  },
  lena: {
    user: users.lena,
    teams: [{ team_id: teams.platform.id, name: teams.platform.name, role: "member" }],
    csrf_token: csrfToken,
  },
  tomas: {
    user: users.tomas,
    teams: [{ team_id: teams.research.id, name: teams.research.name, role: "member" }],
    csrf_token: csrfToken,
  },
  priya: { user: users.priya, teams: [], csrf_token: csrfToken },
} satisfies Record<string, Me>;

function key(
  id: number,
  last4: string,
  name: string,
  status: string,
  owner: User | null,
  team: Team | null,
  dates: { expires_at?: string; revoked_at?: string } = {},
): Key {
  return {
    id,
    name,
    display: displayOf("uf-sk-", last4),
    status,
    owner_id: owner?.id ?? null,
    owner_email: owner?.email ?? null,
    team_id: team?.id ?? null,
    team_name: team?.name ?? null,
    expires_at: dates.expires_at ?? null,
    revoked_at: dates.revoked_at ?? null,
    created_at: "2026-08-01 09:00:00",
  };
}

export const keys = {
  /** The key that `POST /api/keys` answers with; its display ends as `newKeySecret` does. */
  active: key(1, newKeySecret.slice(-4), "platform-prod", "active", users.arjun, teams.platform),
  suspended: key(2, "7d2f", "dana-notebook", "suspended", users.dana, null),
  expired: key(3, "03ab", "research-trial", "expired", users.tomas, teams.research, {
    expires_at: "2026-09-01 00:00:00",
  }),
  revoked: key(4, "e41c", "lena-old-laptop", "revoked", users.lena, teams.platform, {
    revoked_at: "2026-09-10 13:00:00",
  }),
  /** Belongs to a team and to no person. */
  noOwner: key(5, "9b60", "platform-ci", "active", null, teams.platform),
} satisfies Record<string, Key>;

export const keyList: Key[] = Object.values(keys);

export const providers = {
  withCredential: {
    id: 1,
    name: "openai",
    kind: "openai",
    base_url: "https://api.openai.example.test/v1",
    has_credential: true,
  },
  withoutCredential: {
    id: 2,
    name: "local-llm",
    kind: "openai",
    base_url: "http://llm.internal.example.test:8000/v1",
    has_credential: false,
  },
} satisfies Record<string, Provider>;

export const providerList: Provider[] = Object.values(providers);

export const tokens = {
  /** The token that `POST /api/tokens` answers with; its display ends as `newTokenSecret` does. */
  active: {
    id: 1,
    name: "terraform",
    display: displayOf("uf-at-", newTokenSecret.slice(-4)),
    expires_at: null,
    revoked_at: null,
    last_used_at: "2026-09-28 08:00:00",
    created_at: "2026-08-10 09:00:00",
  },
  /** Has an expiry, which has not come. */
  neverUsed: {
    id: 2,
    name: "backup-script",
    display: displayOf("uf-at-", "5a1e"),
    expires_at: farFuture,
    revoked_at: null,
    last_used_at: null,
    created_at: "2026-09-15 09:00:00",
  },
  revoked: {
    id: 3,
    name: "old-ci",
    display: displayOf("uf-at-", "c07d"),
    expires_at: null,
    revoked_at: "2026-09-05 09:00:00",
    last_used_at: "2026-09-04 22:10:00",
    created_at: "2026-07-01 09:00:00",
  },
  /** Its time has passed; it was not revoked. */
  expired: {
    id: 4,
    name: "migration",
    display: displayOf("uf-at-", "1f9b"),
    expires_at: "2026-09-01 23:59:59",
    revoked_at: null,
    last_used_at: "2026-08-30 10:00:00",
    created_at: "2026-08-01 09:00:00",
  },
} satisfies Record<string, Token>;

export const tokenList: Token[] = Object.values(tokens);

/** Newest first, as the API gives them. */
export const auditEntries: AuditEntry[] = [
  {
    id: 5,
    at: "2026-09-25 15:00:00",
    actor_email: users.maya.email,
    action: "user.invite",
    target_type: "user",
    target_id: users.sam.id,
    summary: `Invited ${users.sam.email} as ${users.sam.role}`,
  },
  {
    id: 4,
    at: "2026-09-10 13:00:00",
    actor_email: users.lena.email,
    action: "key.revoke",
    target_type: "key",
    target_id: keys.revoked.id,
    summary: `Revoked key ${keys.revoked.name} (${keys.revoked.display})`,
  },
  {
    id: 3,
    at: "2026-09-01 09:00:00",
    actor_email: users.maya.email,
    action: "team.create",
    target_type: "team",
    target_id: teams.growth.id,
    summary: `Created team ${teams.growth.name}`,
  },
  {
    id: 2,
    at: "2026-08-01 10:05:00",
    actor_email: users.maya.email,
    action: "user.update",
    target_type: "user",
    target_id: users.dana.id,
    summary: `Changed status of ${users.dana.email} from active to disabled`,
  },
  {
    id: 1,
    at: "2026-06-01 09:00:00",
    actor_email: users.maya.email,
    action: "setup.create_admin",
    target_type: "user",
    target_id: users.maya.id,
    summary: `Created the first admin ${users.maya.email}`,
  },
];

/**
 * A stretch of the audit log as the gateway gives it: `count` sign-ins of the
 * admin, newest first, with ids that fall by one from `newest`. Each is a
 * minute after the one before it, so the time of an entry tells which it is.
 */
export function auditEntriesFrom(newest: number, count: number): AuditEntry[] {
  const start = Date.UTC(2026, 8, 26);
  return Array.from({ length: count }, (_, index) => {
    const id = newest - index;
    return {
      id,
      at: new Date(start + id * 60_000).toISOString().slice(0, 19).replace("T", " "),
      actor_email: users.maya.email,
      action: "auth.login",
      target_type: "user",
      target_id: users.maya.id,
      summary: `${users.maya.email} signed in`,
    };
  });
}
