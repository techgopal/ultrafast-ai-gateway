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
export type Log = Schemas["LogView"];
export type LogDetail = Schemas["LogDetailView"];
export type LogAttempt = Schemas["LogAttempt"];
export type UsageRow = Schemas["UsageRow"];
export type UsagePage = Schemas["UsagePage"];
export type Limit = Schemas["LimitView"];
export type Budget = Schemas["BudgetView"];
export type Settings = Schemas["SettingsView"];

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
    tags: {},
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

/** What the gateway lists of the providers to somebody who is not an admin: no base URL. */
export const providersForMember: Provider[] = providerList.map((provider) => ({
  ...provider,
  base_url: null,
}));

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
    input_price_micros: 150_000,
    output_price_micros: 600_000,
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
    input_price_micros: 2_500_000,
    output_price_micros: 10_000_000,
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
    input_price_micros: null,
    output_price_micros: null,
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
    input_price_micros: null,
    output_price_micros: null,
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
  cache_enabled: false,
  cache_ttl_s: 300,
  cache_scope: "team",
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
    cache_enabled: true,
    cache_ttl_s: 600,
    cache_scope: "user",
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
 * weights, settings, the cache, `everyone` and `team_ids` read as 0, false or empty.
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
    cache_enabled: false,
    cache_ttl_s: 0,
    cache_scope: "team",
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

function log(
  id: number,
  at: string,
  who: { key: Key | null; user: User | null; team: Team | null },
  call: Partial<Log>,
): Log {
  return {
    id,
    at,
    key_id: who.key?.id ?? null,
    key_name: who.key?.name ?? null,
    user_id: who.user?.id ?? null,
    user_email: who.user?.email ?? null,
    team_id: who.team?.id ?? null,
    team_name: who.team?.name ?? null,
    requested: "gpt-4o",
    endpoint: "/v1/chat/completions",
    stream: false,
    status: 200,
    provider: "openai",
    model: "gpt-4o",
    input_tokens: 120,
    output_tokens: 48,
    cost_micros: 1_250_000,
    priced: true,
    cached: false,
    estimated: false,
    duration_ms: 850,
    tags: {},
    ...call,
  };
}

const platformCall = { key: keys.active, user: users.arjun, team: teams.platform };

/** Newest first, as the API gives them. */
export const logs = {
  answered: log(5, "2026-09-30 11:55:00", platformCall, {}),
  /** The call the second target answered, after the first failed. */
  failedOver: log(4, "2026-09-30 11:40:00", platformCall, {
    requested: "chat-fast",
    provider: "anthropic",
    model: "claude-haiku",
    stream: true,
    cost_micros: 4_000,
    duration_ms: 2_400,
  }),
  cached: log(3, "2026-09-30 11:30:00", platformCall, {
    cost_micros: 0,
    duration_ms: 3,
    cached: true,
  }),
  /** No target answered: only the name asked for is known. */
  failed: log(2, "2026-09-30 11:20:00", { key: keys.expired, user: users.tomas, team: teams.research }, {
    requested: "chat-fast",
    status: 502,
    provider: null,
    model: null,
    input_tokens: null,
    output_tokens: null,
    cost_micros: 0,
    priced: false,
    duration_ms: 30_000,
  }),
  /** Tokens but no price. */
  unpriced: log(1, "2026-09-30 11:10:00", { key: null, user: users.tomas, team: null }, {
    requested: "local-llama",
    provider: "ollama",
    model: "llama3",
    cost_micros: 0,
    priced: false,
  }),
} satisfies Record<string, Log>;

export const logList: Log[] = Object.values(logs);

/**
 * A stream the caller left: charged an estimate, priced, and marked. It is
 * not in `logList`, so the counts of the list do not move.
 */
export const estimatedLog: Log = log(6, "2026-09-30 11:58:00", platformCall, {
  stream: true,
  status: 499,
  input_tokens: 10,
  output_tokens: 30,
  cost_micros: 2_000,
  estimated: true,
});

/** What `GET /api/logs/{id}` adds: the targets tried, in order. */
export const logAttempts: Record<number, LogAttempt[]> = {
  5: [{ provider: "openai", model: "gpt-4o", outcome: "ok", status: 200, duration_ms: 850 }],
  4: [
    { provider: "openai", model: "gpt-4o-mini", outcome: "circuit_open", status: null, duration_ms: 0 },
    { provider: "openai", model: "gpt-4o", outcome: "retryable", status: 503, duration_ms: 900 },
    { provider: "anthropic", model: "claude-haiku", outcome: "ok", status: 200, duration_ms: 1_500 },
  ],
  3: [{ provider: "openai", model: "gpt-4o", outcome: "cached", status: null, duration_ms: 3 }],
  2: [
    { provider: "openai", model: "gpt-4o", outcome: "fatal", status: 401, duration_ms: 120 },
    { provider: "anthropic", model: "claude-haiku", outcome: "skipped", status: null, duration_ms: 0 },
  ],
  1: [{ provider: "ollama", model: "llama3", outcome: "ok", status: 200, duration_ms: 850 }],
};

export function logDetail(id: number): LogDetail | undefined {
  const row = logList.find((one) => one.id === id);
  return row === undefined ? undefined : { ...row, attempts: logAttempts[id] ?? [] };
}

function usageRow(group: string, label: string, rest: Partial<UsageRow>): UsageRow {
  return {
    group,
    label,
    requests: 0,
    errors: 0,
    cancelled: 0,
    input_tokens: 0,
    output_tokens: 0,
    cost_micros: 0,
    unpriced_requests: 0,
    ...rest,
  };
}

/** `GET /api/usage?group=day`: only days with calls have a row. */
export const usageByDay: UsagePage = {
  from: "2026-09-01",
  to: "2026-09-30",
  total: usageRow("total", "Total", {
    requests: 150,
    errors: 6,
    cancelled: 3,
    input_tokens: 15_000,
    output_tokens: 6_000,
    cost_micros: 12_340_000,
    unpriced_requests: 4,
  }),
  rows: [
    usageRow("2026-09-28", "2026-09-28", {
      requests: 50,
      errors: 1,
      cancelled: 2,
      input_tokens: 5_000,
      output_tokens: 2_000,
      cost_micros: 4_000_000,
      unpriced_requests: 1,
    }),
    usageRow("2026-09-29", "2026-09-29", {
      requests: 30,
      errors: 5,
      cancelled: 1,
      input_tokens: 3_000,
      output_tokens: 1_000,
      cost_micros: 2_340_000,
      unpriced_requests: 3,
    }),
    usageRow("2026-09-30", "2026-09-30", {
      requests: 70,
      input_tokens: 7_000,
      output_tokens: 3_000,
      cost_micros: 6_000_000,
    }),
  ],
};

const modelRow = (label: string, requests: number, cost: number) =>
  usageRow(label, label, { requests, cost_micros: cost, input_tokens: requests * 100, output_tokens: requests * 40 });

/** `GET /api/usage?group=model`: most requests first, as the API orders it. */
export const usageByModel: UsagePage = {
  ...usageByDay,
  rows: [
    modelRow("openai/gpt-4o", 60, 8_000_000),
    modelRow("anthropic/claude-haiku", 40, 3_000_000),
    modelRow("chat-fast", 20, 0),
    modelRow("ollama/llama3", 15, 1_340_000),
    modelRow("openai/gpt-4o-mini", 10, 5_000),
    modelRow("openai/o3", 5, 0),
  ],
};

/** `GET /api/usage?group=key`; a call without a key is `(none)`, a deleted key `(deleted)`. */
export const usageByKey: UsagePage = {
  ...usageByDay,
  rows: [
    usageRow("1", keys.active.name, { requests: 100, cost_micros: 9_000_000 }),
    usageRow("", "(none)", { requests: 30, cost_micros: 3_000_000 }),
    usageRow("9", "(deleted)", { requests: 20, cost_micros: 340_000 }),
  ],
};

export function usageOf(group: string): UsagePage {
  if (group === "model") return usageByModel;
  if (group === "key") return usageByKey;
  return usageByDay;
}

/** `GET /api/limits` for an admin: one of each scope; a number that is not set is `null`. */
export const limits = {
  gateway: {
    id: 1,
    scope: "gateway",
    scope_id: null,
    label: "gateway",
    requests_per_minute: 6000,
    tokens_per_minute: null,
    concurrent: 200,
  },
  team: {
    id: 2,
    scope: "team",
    scope_id: teams.platform.id,
    label: `team '${teams.platform.name}'`,
    requests_per_minute: 600,
    tokens_per_minute: 1_000_000,
    concurrent: null,
  },
  user: {
    id: 3,
    scope: "user",
    scope_id: users.arjun.id,
    label: `user '${users.arjun.email}'`,
    requests_per_minute: null,
    tokens_per_minute: 50_000,
    concurrent: 4,
  },
  key: {
    id: 4,
    scope: "key",
    scope_id: keys.active.id,
    label: `key '${keys.active.name}'`,
    requests_per_minute: 60,
    tokens_per_minute: null,
    concurrent: null,
  },
} satisfies Record<string, Limit>;

export const limitList: Limit[] = Object.values(limits);

/** What `GET /api/limits` gives a member of Platform: the limits that apply to them. */
export const limitsForMember: Limit[] = [limits.gateway, limits.team];

/** `GET /api/budgets`: `period_start` is the UTC date the period began on. */
export const budgets = {
  gateway: {
    id: 1,
    scope: "gateway",
    scope_id: null,
    label: "gateway",
    amount_micros: 100_000_000,
    period: "monthly",
    action: "block",
    period_start: "2026-09-01",
    spent_micros: 12_500_000,
  },
  team: {
    id: 2,
    scope: "team",
    scope_id: teams.platform.id,
    label: `team '${teams.platform.name}'`,
    amount_micros: 10_000_000,
    period: "weekly",
    action: "alert",
    period_start: "2026-09-28",
    spent_micros: 12_000_000,
  },
  key: {
    id: 3,
    scope: "key",
    scope_id: keys.active.id,
    label: `key '${keys.active.name}'`,
    amount_micros: 2_500_000,
    period: "daily",
    action: "block",
    period_start: "2026-09-30",
    spent_micros: 0,
  },
} satisfies Record<string, Budget>;

export const budgetList: Budget[] = Object.values(budgets);

/**
 * What a member of Platform gets of the budgets: the gateway's and their
 * team's without the spend, their own key's with it.
 */
export const budgetsForMember: Budget[] = [
  { ...budgets.gateway, spent_micros: null },
  { ...budgets.team, spent_micros: null },
  budgets.key,
];

export const settings: Settings = {
  log_retention_days: 30,
  session_hours: 12,
  trusted_proxies: ["10.0.0.0/8"],
  login_limits: { window_minutes: 15, max_per_email: 5, max_per_address: 20 },
};

type ImportReport = components["schemas"]["ImportReport"];

/** What an import reports: what it would do, nothing to do, and a file with errors. */
export const importReports = {
  /** A dry run of a file with something new, something changed and a provider without a credential. */
  changes: {
    created: [
      { kind: "provider", name: "extra", changes: [] },
      { kind: "team", name: "Design", changes: [] },
    ],
    updated: [{ kind: "model", name: "openai/gpt-4o", changes: ["enabled", "grants"] }],
    unchanged: 5,
    warnings: [
      {
        at: "providers[0]",
        message: "provider 'extra' is created with no credential; set one before it can be called",
      },
    ],
    errors: [],
  },
  /** The file says what the gateway has. */
  nothing: { created: [], updated: [], unchanged: 7, warnings: [], errors: [] },
  /** Nothing is written: no created, no updated. */
  invalid: {
    created: [],
    updated: [],
    unchanged: 0,
    warnings: [],
    errors: [
      { at: "models[0].grants.teams[1]", message: "team 'No such team' does not exist" },
      { at: "settings.session_hours", message: "must be from 1 to 720" },
    ],
  },
} satisfies Record<string, ImportReport>;

/** A configuration file: what `GET /api/config/export` answers, in a small gateway. */
export const configFile = {
  format: "ultrafast-config",
  version: 1,
  providers: [
    { name: "openai", kind: "openai", base_url: "https://api.openai.com/v1", api_version: null },
  ],
  models: [
    {
      provider: "openai",
      name: "gpt-4o-mini",
      enabled: true,
      input_price_micros: 150_000,
      output_price_micros: 600_000,
      grants: { everyone: true, teams: [], users: [] },
    },
  ],
  teams: [{ name: "Platform" }],
  routes: [],
  limits: [],
  budgets: [],
  settings: { log_retention_days: 30, session_hours: 12 },
} satisfies components["schemas"]["ConfigFile"];

/** What the playground's call streams by default: two deltas, the finish and the usage. */
export const playgroundChunks: readonly string[] = [
  'data: {"id":"chatcmpl-1","object":"chat.completion.chunk","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":"Hel"},"finish_reason":null}]}\n\n',
  'data: {"id":"chatcmpl-1","object":"chat.completion.chunk","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":"lo"},"finish_reason":null}]}\n\n',
  'data: {"id":"chatcmpl-1","object":"chat.completion.chunk","model":"gpt-4o-mini","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}\n\n',
  "data: [DONE]\n\n",
];
