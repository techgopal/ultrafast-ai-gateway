// The people, teams, keys and the rest that the tests share. Every value has
// a type generated from the API description, so a change of the API that
// these do not follow is a compile error.
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

/** The CSRF token of the session in the tests. */
export const csrfToken = "csrf-token-of-the-test-session";

// Secrets that are shown once. They are made up and work nowhere.
export const newKeySecret = "uf-sk-test-0000-shown-once-key";
export const newTokenSecret = "uf-at-test-0000-shown-once-token";
export const newInviteLink = "/accept-invite?token=test-0000-shown-once-invite";

export const users = {
  /** An admin. */
  maya: {
    id: 1,
    email: "maya@example.test",
    name: "Maya Okafor",
    role: "admin",
    status: "active",
    created_at: "2026-06-01T09:00:00Z",
    last_active_at: "2026-09-28T16:20:00Z",
  },
  /** Lead of Platform, member of Research. */
  arjun: {
    id: 2,
    email: "arjun@example.test",
    name: "Arjun Mehta",
    role: "member",
    status: "active",
    created_at: "2026-06-03T10:00:00Z",
    last_active_at: "2026-09-27T11:05:00Z",
  },
  /** Member of Platform. */
  lena: {
    id: 3,
    email: "lena@example.test",
    name: "Lena Fischer",
    role: "member",
    status: "active",
    created_at: "2026-06-10T08:30:00Z",
    last_active_at: "2026-09-26T09:45:00Z",
  },
  /** Member of Research. */
  tomas: {
    id: 4,
    email: "tomas@example.test",
    name: "Tomas Novak",
    role: "member",
    status: "active",
    created_at: "2026-07-01T12:00:00Z",
    last_active_at: "2026-09-20T14:00:00Z",
  },
  /** In no team. */
  priya: {
    id: 5,
    email: "priya@example.test",
    name: "Priya Raman",
    role: "member",
    status: "active",
    created_at: "2026-07-15T12:00:00Z",
    last_active_at: null,
  },
  /** Invited, has not set a password yet. */
  sam: {
    id: 6,
    email: "sam@example.test",
    name: "Sam Carter",
    role: "member",
    status: "invited",
    created_at: "2026-09-25T15:00:00Z",
    last_active_at: null,
  },
  /** Disabled. Owns a key, which is suspended. */
  dana: {
    id: 7,
    email: "dana@example.test",
    name: "Dana Whitfield",
    role: "member",
    status: "disabled",
    created_at: "2026-06-20T10:00:00Z",
    last_active_at: "2026-08-01T10:00:00Z",
  },
} as const satisfies Record<string, User>;

export const userList: User[] = Object.values(users);

export const teams = {
  platform: { id: 1, name: "Platform", member_count: 2, created_at: "2026-06-02T09:00:00Z" },
  research: { id: 2, name: "Research", member_count: 2, created_at: "2026-06-05T09:00:00Z" },
  /** Has no members. */
  growth: { id: 3, name: "Growth", member_count: 0, created_at: "2026-09-01T09:00:00Z" },
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
  name: string,
  status: string,
  owner: User | null,
  team: Team | null,
  dates: { expires_at?: string; revoked_at?: string } = {},
): Key {
  return {
    id,
    name,
    display: `uf-sk-...${String(id).padStart(4, "0")}`,
    status,
    owner_id: owner?.id ?? null,
    owner_email: owner?.email ?? null,
    team_id: team?.id ?? null,
    team_name: team?.name ?? null,
    expires_at: dates.expires_at ?? null,
    revoked_at: dates.revoked_at ?? null,
    created_at: "2026-08-01T09:00:00Z",
  };
}

export const keys = {
  active: key(1, "platform-prod", "active", users.arjun, teams.platform),
  suspended: key(2, "dana-notebook", "suspended", users.dana, null),
  expired: key(3, "research-trial", "expired", users.tomas, teams.research, {
    expires_at: "2026-09-01T00:00:00Z",
  }),
  revoked: key(4, "lena-old-laptop", "revoked", users.lena, teams.platform, {
    revoked_at: "2026-09-10T13:00:00Z",
  }),
  /** Belongs to a team and to no person. */
  noOwner: key(5, "platform-ci", "active", null, teams.platform),
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
  active: {
    id: 1,
    name: "terraform",
    display: "uf-at-...0001",
    expires_at: null,
    revoked_at: null,
    last_used_at: "2026-09-28T08:00:00Z",
    created_at: "2026-08-10T09:00:00Z",
  },
  neverUsed: {
    id: 2,
    name: "backup-script",
    display: "uf-at-...0002",
    expires_at: "2027-01-01T00:00:00Z",
    revoked_at: null,
    last_used_at: null,
    created_at: "2026-09-15T09:00:00Z",
  },
  revoked: {
    id: 3,
    name: "old-ci",
    display: "uf-at-...0003",
    expires_at: null,
    revoked_at: "2026-09-05T09:00:00Z",
    last_used_at: "2026-09-04T22:10:00Z",
    created_at: "2026-07-01T09:00:00Z",
  },
} satisfies Record<string, Token>;

export const tokenList: Token[] = Object.values(tokens);

/** Newest first, as the API gives them. */
export const auditEntries: AuditEntry[] = [
  {
    id: 5,
    at: "2026-09-25T15:00:00Z",
    actor_email: users.maya.email,
    action: "user.invite",
    target_type: "user",
    target_id: users.sam.id,
    summary: "Invited sam@example.test as member",
  },
  {
    id: 4,
    at: "2026-09-10T13:00:00Z",
    actor_email: users.lena.email,
    action: "key.revoke",
    target_type: "key",
    target_id: keys.revoked.id,
    summary: "Revoked key lena-old-laptop",
  },
  {
    id: 3,
    at: "2026-09-01T09:00:00Z",
    actor_email: users.maya.email,
    action: "team.create",
    target_type: "team",
    target_id: teams.growth.id,
    summary: "Created team Growth",
  },
  {
    id: 2,
    at: "2026-08-01T10:05:00Z",
    actor_email: users.maya.email,
    action: "user.update",
    target_type: "user",
    target_id: users.dana.id,
    summary: "Disabled dana@example.test",
  },
  {
    id: 1,
    at: "2026-06-01T09:00:00Z",
    actor_email: users.maya.email,
    action: "setup",
    target_type: "organization",
    target_id: null,
    summary: "Set up the gateway",
  },
];
