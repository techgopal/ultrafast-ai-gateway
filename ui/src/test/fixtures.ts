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
export type Model = Schemas["ModelView"];
export type Grants = Schemas["GrantsView"];
export type SyncResult = Schemas["SyncResult"];
export type Token = Schemas["TokenView"];
export type AuditEntry = Schemas["AuditRow"];
export type Me = Schemas["MeResponse"];
export type Route = Schemas["RouteView"];
export type TargetHealth = Schemas["TargetHealth"];

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
    teams: [],
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
    teams: [
      { team_id: 1, name: "Platform", role: "lead" },
      { team_id: 2, name: "Research", role: "member" },
    ],
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
    teams: [{ team_id: 1, name: "Platform", role: "member" }],
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
    teams: [{ team_id: 2, name: "Research", role: "member" }],
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
    teams: [],
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
    teams: [],
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
    teams: [],
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
    allowed: null,
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
    api_version: null,
  },
  withoutCredential: {
    id: 2,
    name: "local-llm",
    kind: "openai",
    base_url: "http://llm.internal.example.test:8000/v1",
    has_credential: false,
    api_version: null,
  },
} satisfies Record<string, Provider>;

export const providerList: Provider[] = Object.values(providers);

export const models = {
  /** Enabled, granted to everyone. */
  openaiMini: {
    id: 1,
    provider_id: providers.withCredential.id,
    provider_name: providers.withCredential.name,
    name: "gpt-4o-mini",
    enabled: true,
    grants: { everyone: true, team_ids: [], user_ids: [] },
    created_at: "2026-09-01 10:00:00",
  },
  /** Enabled, granted to two teams and a user. */
  openaiFull: {
    id: 2,
    provider_id: providers.withCredential.id,
    provider_name: providers.withCredential.name,
    name: "gpt-4o",
    enabled: true,
    grants: { everyone: false, team_ids: [teams.platform.id, teams.research.id], user_ids: [users.priya.id] },
    created_at: "2026-09-01 10:00:01",
  },
  /** Disabled, as a synced model starts: granted to nobody. */
  openaiDisabled: {
    id: 3,
    provider_id: providers.withCredential.id,
    provider_name: providers.withCredential.name,
    name: "o3-mini",
    enabled: false,
    grants: { everyone: false, team_ids: [], user_ids: [] },
    created_at: "2026-09-01 10:00:02",
  },
  /** Enabled, and nobody has access: only an admin can call it. */
  localLlama: {
    id: 4,
    provider_id: providers.withoutCredential.id,
    provider_name: providers.withoutCredential.name,
    name: "llama3.1:8b",
    enabled: true,
    grants: { everyone: false, team_ids: [], user_ids: [] },
    created_at: "2026-09-02 08:00:00",
  },
} satisfies Record<string, Model>;

export const modelList: Model[] = Object.values(models);

/** What a member gets of the models: those they may call, with empty grants. */
export const callableModels: Model[] = [models.openaiMini, models.openaiFull].map((model) => ({
  ...model,
  grants: { everyone: false, team_ids: [], user_ids: [] },
}));

/** The settings a route has when it is made without any (the migration of the routes). */
const defaultSettings = {
  retries: 2,
  first_token_timeout_ms: 30_000,
  total_timeout_ms: 300_000,
  breaker_failures: 5,
  breaker_window_s: 60,
  breaker_open_s: 30,
} as const;

export const routes = {
  /** For everyone; two primaries and a fallback. */
  support: {
    id: 1,
    name: "support-chat",
    primaries: [
      { model_id: models.openaiMini.id, model: "openai/gpt-4o-mini", weight: 3, enabled: true },
      { model_id: models.openaiFull.id, model: "openai/gpt-4o", weight: 1, enabled: true },
    ],
    fallbacks: [{ model_id: models.localLlama.id, model: "local-llm/llama3.1:8b", enabled: true }],
    ...defaultSettings,
    everyone: true,
    team_ids: [],
    broken: false,
    created_at: "2026-09-10 09:00:00",
  },
  /** For two teams; settings that are not the defaults. */
  research: {
    id: 2,
    name: "research",
    primaries: [{ model_id: models.openaiFull.id, model: "openai/gpt-4o", weight: 1, enabled: true }],
    fallbacks: [],
    retries: 0,
    first_token_timeout_ms: 10_000,
    total_timeout_ms: 120_000,
    breaker_failures: 3,
    breaker_window_s: 30,
    breaker_open_s: 15,
    everyone: false,
    team_ids: [teams.platform.id, teams.research.id],
    broken: false,
    created_at: "2026-09-11 09:00:00",
  },
  /** For admins only, and no target of it is enabled. */
  legacy: {
    id: 3,
    name: "legacy.v1",
    primaries: [
      { model_id: models.openaiDisabled.id, model: "openai/o3-mini", weight: 1, enabled: false },
    ],
    fallbacks: [],
    ...defaultSettings,
    everyone: false,
    team_ids: [],
    broken: true,
    created_at: "2026-09-12 09:00:00",
  },
} satisfies Record<string, Route>;

export const routeList: Route[] = Object.values(routes);

/**
 * What the gateway shows of a route to somebody who is not an admin: ids,
 * weights, settings, `everyone` and `team_ids` read as 0, false or empty.
 */
export function asMemberSees(route: Route): Route {
  return {
    ...route,
    primaries: route.primaries.map((p) => ({ ...p, model_id: 0, weight: 0 })),
    fallbacks: route.fallbacks.map((f) => ({ ...f, model_id: 0 })),
    retries: 0,
    first_token_timeout_ms: 0,
    total_timeout_ms: 0,
    breaker_failures: 0,
    breaker_window_s: 0,
    breaker_open_s: 0,
    everyone: false,
    team_ids: [],
  };
}

/** What a member of Platform gets of the routes: those their teams or everyone may use. */
export const routesForMember: Route[] = [routes.support, routes.research].map(asMemberSees);

/** What `GET /api/routing/health` answers: the targets that were called, one in each state. */
export const healthTargets = {
  healthy: {
    provider: "openai",
    model: "gpt-4o-mini",
    state: "closed",
    successes: 120,
    failures: 2,
    last_failure_at: "2026-09-30 11:00:00",
    last_status: 503,
  },
  failing: {
    provider: "openai",
    model: "gpt-4o",
    state: "open",
    successes: 40,
    failures: 9,
    last_failure_at: "2026-09-30 11:30:00",
    last_status: 429,
  },
  testing: {
    provider: "local-llm",
    model: "llama3.1:8b",
    state: "half_open",
    successes: 5,
    failures: 5,
    last_failure_at: "2026-09-30 11:45:00",
    last_status: null,
  },
  /** Never failed. */
  clean: {
    provider: "openai",
    model: "o3-mini",
    state: "closed",
    successes: 7,
    failures: 0,
    last_failure_at: null,
    last_status: null,
  },
} satisfies Record<string, TargetHealth>;

export const healthList: TargetHealth[] = Object.values(healthTargets);

/** What the sync of a provider answers when the provider listed new names. */
export const syncResult: SyncResult = { added: ["gpt-4.1", "gpt-4.1-mini"], existing: 4 };

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
    status: "active",
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
    status: "active",
  },
  revoked: {
    id: 3,
    name: "old-ci",
    display: displayOf("uf-at-", "c07d"),
    expires_at: null,
    revoked_at: "2026-09-05 09:00:00",
    last_used_at: "2026-09-04 22:10:00",
    created_at: "2026-07-01 09:00:00",
    status: "revoked",
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
    status: "expired",
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
