// The query keys and the hooks the pages use: one hook for each operation of
// the admin API, one for the members of several teams, and one for the audit
// log as the pages it is read in.
//
// Two rules hold for everything here. A query key holds ids and page numbers,
// never a secret. And a mutation is dropped from the mutation cache as soon as
// nothing observes it (`gcTime: 0`), because its variables can hold a password
// or a provider API key and its answer can hold a secret that is shown once.
// While a component observes a mutation, the hook gives it the variables and
// the answer; a form calls `reset()` when its request settled, a dialog when
// it closes. No mutation has a `mutationKey`, and nothing reads the state of
// a mutation from elsewhere.
import {
  QueryCache,
  QueryClient,
  queryOptions,
  useInfiniteQuery,
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
  type Query,
  type QueryKey,
} from "@tanstack/react-query";
import { api, type BodyOf, type QueryOf } from "./client";
import { ApiError, NetworkError, type SessionOverError } from "./errors";

declare module "@tanstack/react-query" {
  interface Register {
    defaultError: ApiError | NetworkError | SessionOverError;
  }
}

export type AuditPageRequest = QueryOf<"/api/audit", "get">;

/** What the key of a detail has after its area. */
const DETAIL = "detail";

/**
 * The key of one thing of an area, which is read by its id: a team, a user,
 * a key. Every such key is built here, and `isDetailKey` knows it by what is
 * put into it here. So the rule for what the gateway hides
 * (`forgetWhatIsHidden`) holds for a detail that is added later, without a
 * list of names anywhere.
 */
const detailOf =
  <Area extends string>(area: Area) =>
  (id: number) =>
    [area, DETAIL, id] as const;

/** Whether the key is the key of a detail: one that `detailOf` built. */
export function isDetailKey(key: QueryKey): boolean {
  return key[1] === DETAIL;
}

export const queryKeys = {
  setup: () => ["setup"] as const,
  me: () => ["me"] as const,
  users: {
    all: () => ["users"] as const,
    list: () => ["users", "list"] as const,
    detail: detailOf("users"),
  },
  teams: {
    all: () => ["teams"] as const,
    list: () => ["teams", "list"] as const,
    detail: detailOf("teams"),
  },
  keys: {
    all: () => ["keys"] as const,
    list: () => ["keys", "list"] as const,
    detail: detailOf("keys"),
  },
  providers: {
    all: () => ["providers"] as const,
    list: () => ["providers", "list"] as const,
  },
  tokens: {
    all: () => ["tokens"] as const,
    list: () => ["tokens", "list"] as const,
  },
  audit: {
    all: () => ["audit"] as const,
    list: (page: AuditPageRequest = {}) =>
      ["audit", "list", { limit: page.limit ?? null, before: page.before ?? null }] as const,
    /** The log as it is read page by page: one entry of the cache for all its pages. */
    pages: () => ["audit", "pages"] as const,
  },
};

/** How often a query is tried again after a network error. */
const NETWORK_RETRIES = 2;

/** Network errors are tried again; an answer of the gateway is final. */
function retryQuery(failures: number, error: unknown): boolean {
  return error instanceof NetworkError && failures < NETWORK_RETRIES;
}

export interface QueryClientSettings {
  /** `false` for no retries at all. Default: the rule of the app. */
  retry?: false;
  /** Milliseconds between the tries. Default: that of the library. */
  retryDelay?: number;
  /** How long an unobserved mutation is kept. Default 0. The tests of the cache scan set it. */
  mutationGcTime?: number;
}

function isNotFound(error: unknown): boolean {
  return error instanceof ApiError && error.status === 404;
}

/**
 * The queries that a mutation said are gone while a page still showed them:
 * see `dropWhatIsGone`. It holds the query, not its key: a query that is
 * made later for the same key, by a return to the page, is not among them.
 */
const goneButShown = new WeakSet<object>();

/**
 * A detail that answers 404 when it is asked for again is gone, or hidden
 * from the caller. What was loaded of it before is dropped: who shows it has
 * the 404 and no data then, and says "not found"; a failure of another kind
 * that follows cannot bring it back to the screen; and nothing of it is left
 * in the cache.
 *
 * Only for details. A 404 of `setup`, of `me` or of a list does not mean
 * this, and the app goes on with what it has of them. Every other failure of
 * a detail keeps what is shown.
 *
 * And not for what a mutation said is gone while its page still shows it.
 * That page is on its way to another one. A read of it in that time, by the
 * window getting the focus, by the network coming back or one that was on
 * its way already, answers 404: dropping the data then would show "not
 * found" for a moment before the page that follows. The query is dropped
 * when the page has gone.
 */
function forgetWhatIsHidden(error: unknown, query: Query<unknown, unknown>): void {
  if (!isNotFound(error) || !isDetailKey(query.queryKey)) return;
  if (query.state.data === undefined || goneButShown.has(query)) return;
  query.setState({ data: undefined, dataUpdatedAt: 0 });
}

/** A query client with the rules of the app. The app has one; each test makes its own. */
export function createQueryClient(settings: QueryClientSettings = {}): QueryClient {
  return new QueryClient({
    queryCache: new QueryCache({ onError: forgetWhatIsHidden }),
    defaultOptions: {
      queries: {
        retry: settings.retry ?? retryQuery,
        ...(settings.retryDelay === undefined ? {} : { retryDelay: settings.retryDelay }),
      },
      mutations: { retry: false, gcTime: settings.mutationGcTime ?? 0 },
    },
  });
}

// ---------------------------------------------------------------- queries

export const setupStatusOptions = () =>
  queryOptions({
    queryKey: queryKeys.setup(),
    queryFn: ({ signal }) => api.get("/api/setup", { signal }),
  });

export const meOptions = () =>
  queryOptions({
    queryKey: queryKeys.me(),
    queryFn: ({ signal }) => api.get("/api/auth/me", { signal }),
  });

export const usersOptions = () =>
  queryOptions({
    queryKey: queryKeys.users.list(),
    queryFn: ({ signal }) => api.get("/api/users", { signal }),
  });

export const userOptions = (id: number) =>
  queryOptions({
    queryKey: queryKeys.users.detail(id),
    queryFn: ({ signal }) => api.get("/api/users/{id}", { params: { id }, signal }),
  });

export const teamsOptions = () =>
  queryOptions({
    queryKey: queryKeys.teams.list(),
    queryFn: ({ signal }) => api.get("/api/teams", { signal }),
  });

export const teamOptions = (id: number) =>
  queryOptions({
    queryKey: queryKeys.teams.detail(id),
    queryFn: ({ signal }) => api.get("/api/teams/{id}", { params: { id }, signal }),
  });

export const keysOptions = () =>
  queryOptions({
    queryKey: queryKeys.keys.list(),
    queryFn: ({ signal }) => api.get("/api/keys", { signal }),
  });

export const keyOptions = (id: number) =>
  queryOptions({
    queryKey: queryKeys.keys.detail(id),
    queryFn: ({ signal }) => api.get("/api/keys/{id}", { params: { id }, signal }),
  });

export const providersOptions = () =>
  queryOptions({
    queryKey: queryKeys.providers.list(),
    queryFn: ({ signal }) => api.get("/api/providers", { signal }),
  });

export const tokensOptions = () =>
  queryOptions({
    queryKey: queryKeys.tokens.list(),
    queryFn: ({ signal }) => api.get("/api/tokens", { signal }),
  });

export const auditLogOptions = (page: AuditPageRequest = {}) =>
  queryOptions({
    queryKey: queryKeys.audit.list(page),
    queryFn: ({ signal }) => api.get("/api/audit", { query: page, signal }),
  });

export const useSetupStatus = () => useQuery(setupStatusOptions());
export const useMe = () => useQuery(meOptions());
export const useUsers = () => useQuery(usersOptions());
export const useUser = (id: number) => useQuery(userOptions(id));
export const useTeams = () => useQuery(teamsOptions());
export const useTeam = (id: number) => useQuery(teamOptions(id));
/**
 * The teams with their members: one call for each team, in the order of the
 * ids. The API has no operation that gives the teams of a user; who is in
 * which team is read from the teams.
 */
export const useTeamDetails = (ids: readonly number[]) =>
  useQueries({ queries: ids.map((id) => teamOptions(id)) });
export const useKeys = () => useQuery(keysOptions());
export const useKey = (id: number) => useQuery(keyOptions(id));
export const useProviders = () => useQuery(providersOptions());
export const useTokens = () => useQuery(tokensOptions());
export const useAuditLog = (page: AuditPageRequest = {}) => useQuery(auditLogOptions(page));

/** How many entries a page of the audit log has. A page with fewer is the last. */
export const AUDIT_PAGE_SIZE = 50;

/**
 * The audit log, newest first, read page by page. The gateway takes the id
 * of the last entry of the page before as `before` and answers with the
 * entries below it: an entry is in one page only, whatever was added to the
 * log meanwhile. When the log is read again, each page is asked for from
 * where the page before it ends now, so no entry shows twice or out of order.
 */
export const useAuditPages = () =>
  useInfiniteQuery({
    queryKey: queryKeys.audit.pages(),
    queryFn: ({ pageParam, signal }) =>
      api.get("/api/audit", {
        query: { limit: AUDIT_PAGE_SIZE, ...(pageParam === null ? {} : { before: pageParam }) },
        signal,
      }),
    initialPageParam: null as number | null,
    getNextPageParam: (last) =>
      last.entries.length < AUDIT_PAGE_SIZE ? undefined : last.entries.at(-1)?.id,
  });

// -------------------------------------------------------------- mutations

/**
 * Drops what no longer exists, or is the caller's to see no more. It is not
 * asked for again, and it is kept only as long as something shows it.
 *
 * What nothing shows is removed at once. What a page still shows is left as
 * it is, and removed when the last that shows it has gone: removed under the
 * page, it would be asked for again by the page's next render, and the page
 * would show its skeleton and then "not found" on its way to the list. Until
 * then a 404 of it changes nothing either (`goneButShown`).
 *
 * So `gone` is for a page that leaves. Who says `gone` for what a page shows
 * must take the user away from that page at once: as long as the page stays,
 * it goes on showing what no longer exists, and nothing corrects it. For a
 * page that stays, say `stale`.
 *
 * Returns what is left for now, which must not be asked for again either.
 */
function dropWhatIsGone(cache: QueryCache, gone: readonly QueryKey[]): ReadonlySet<Query> {
  const shown = new Set<Query>();
  for (const queryKey of gone) {
    for (const query of cache.findAll({ queryKey })) {
      if (query.getObserversCount() === 0) {
        cache.remove(query);
        continue;
      }
      shown.add(query);
      goneButShown.add(query);
      const stop = cache.subscribe((event) => {
        if (event.query !== query) return;
        if (event.type === "removed") {
          // Somebody else dropped it: the end of the session clears the caches.
          stop();
        } else if (event.type === "observerRemoved" && query.getObserversCount() === 0) {
          stop();
          cache.remove(query);
        }
      });
    }
  }
  return shown;
}

/**
 * A mutation that, after it succeeded, marks what it affects as stale, so
 * that what is on the screen is fetched again. It does not wait for that.
 * `gone` is what no longer exists: it is dropped, not fetched again (see
 * `dropWhatIsGone` for when).
 *
 * A mutation that failed changed nothing, and marks nothing as stale, but
 * for what its failure puts in doubt (`doubts`): that is fetched again.
 */
function useApiMutation<TVariables, TData>(
  mutationFn: (variables: TVariables) => Promise<TData>,
  affects: (variables: TVariables) => { stale: readonly QueryKey[]; gone?: readonly QueryKey[] },
  doubts: (error: unknown, variables: TVariables) => readonly QueryKey[] = () => [],
) {
  const client = useQueryClient();
  return useMutation({
    mutationFn,
    retry: false,
    gcTime: 0,
    onSuccess: (_data, variables) => {
      const { stale, gone = [] } = affects(variables);
      const shown = dropWhatIsGone(client.getQueryCache(), gone);
      for (const queryKey of stale) {
        // A key of `stale` can be the area of what is gone: `["teams"]` has the team.
        void client.invalidateQueries({ queryKey, predicate: (query) => !shown.has(query) });
      }
    },
    onError: (error, variables) => {
      for (const queryKey of doubts(error, variables)) void client.invalidateQueries({ queryKey });
    },
  });
}

const audit = queryKeys.audit.all();

// auth

export const useSetup = () =>
  useApiMutation(
    (body: BodyOf<"/api/setup", "post">) => api.post("/api/setup", { body }),
    () => ({ stale: [queryKeys.setup()] }),
  );

// `useMe` and `useLogin` are the plain calls. What a sign-in and the answer
// of `me` mean for the CSRF token and the caches is decided in one place,
// `auth/session.tsx`. Signing out has no hook here: it is `useSignOut` there,
// which also forgets the session.

export const useLogin = () =>
  useApiMutation(
    (body: BodyOf<"/api/auth/login", "post">) => api.post("/api/auth/login", { body }),
    () => ({ stale: [] }),
  );

export const useAcceptInvite = () =>
  useApiMutation(
    (body: BodyOf<"/api/auth/accept-invite", "post">) =>
      api.post("/api/auth/accept-invite", { body }),
    () => ({ stale: [] }),
  );

export const useChangePassword = () =>
  useApiMutation(
    (body: BodyOf<"/api/auth/password", "post">) => api.post("/api/auth/password", { body }),
    () => ({ stale: [audit] }),
  );

// users

/** The answer holds the invite link, which is shown once. */
export const useInviteUser = () =>
  useApiMutation(
    (body: BodyOf<"/api/users", "post">) => api.post("/api/users", { body }),
    () => ({ stale: [queryKeys.users.all(), audit] }),
  );

/** The answer holds the invite link, which is shown once. */
export const useReinviteUser = () =>
  useApiMutation(
    ({ id }: { id: number }) => api.post("/api/users/{id}/invite", { params: { id } }),
    () => ({ stale: [queryKeys.users.all(), audit] }),
  );

// A user's name, role and status show in teams (members), in keys (owner,
// `suspended`) and, for the caller, in `me`.
const aUserChanged = [
  queryKeys.users.all(),
  queryKeys.teams.all(),
  queryKeys.keys.all(),
  queryKeys.me(),
  audit,
];

export const useUpdateUser = () =>
  useApiMutation(
    ({ id, body }: { id: number; body: BodyOf<"/api/users/{id}", "patch"> }) =>
      api.patch("/api/users/{id}", { params: { id }, body }),
    () => ({ stale: aUserChanged }),
  );

export const useDeleteUser = () =>
  useApiMutation(
    ({ id }: { id: number }) => api.delete("/api/users/{id}", { params: { id } }),
    ({ id }) => ({ stale: aUserChanged, gone: [queryKeys.users.detail(id)] }),
  );

// teams

// A team's name shows in keys, and the caller's teams are in `me`.
const aTeamChanged = [queryKeys.teams.all(), queryKeys.keys.all(), queryKeys.me(), audit];
const membersChanged = [queryKeys.teams.all(), queryKeys.me(), audit];

export const useCreateTeam = () =>
  useApiMutation(
    (body: BodyOf<"/api/teams", "post">) => api.post("/api/teams", { body }),
    () => ({ stale: [queryKeys.teams.all(), audit] }),
  );

export const useRenameTeam = () =>
  useApiMutation(
    ({ id, body }: { id: number; body: BodyOf<"/api/teams/{id}", "patch"> }) =>
      api.patch("/api/teams/{id}", { params: { id }, body }),
    () => ({ stale: aTeamChanged }),
  );

export const useDeleteTeam = () =>
  useApiMutation(
    ({ id }: { id: number }) => api.delete("/api/teams/{id}", { params: { id } }),
    ({ id }) => ({ stale: aTeamChanged, gone: [queryKeys.teams.detail(id)] }),
  );

/** Adds the user to the team, or changes their role in it. */
export const usePutTeamMember = () =>
  useApiMutation(
    ({
      id,
      userId,
      body,
    }: {
      id: number;
      userId: number;
      body: BodyOf<"/api/teams/{id}/members/{user_id}", "put">;
    }) =>
      api.put("/api/teams/{id}/members/{user_id}", { params: { id, user_id: userId }, body }),
    () => ({ stale: membersChanged }),
    // The gateway answers 404 for a team it does not show before it looks at
    // the user. The team is asked for again: its page then shows which it was.
    (error, { id }) => (isNotFound(error) ? [queryKeys.teams.detail(id)] : []),
  );

export const useRemoveTeamMember = () =>
  useApiMutation(
    ({ id, userId }: { id: number; userId: number; leaving?: boolean }) =>
      api.delete("/api/teams/{id}/members/{user_id}", { params: { id, user_id: userId } }),
    // `leaving`: the caller removed themselves and sees the team no more.
    // Asked for again, it would answer 404: to the caller it is gone.
    ({ id, leaving = false }) => ({
      stale: membersChanged,
      gone: leaving ? [queryKeys.teams.detail(id)] : [],
    }),
  );

// keys

/** The answer holds the virtual key, which is shown once. */
export const useCreateKey = () =>
  useApiMutation(
    (body: BodyOf<"/api/keys", "post">) => api.post("/api/keys", { body }),
    () => ({ stale: [queryKeys.keys.all(), audit] }),
  );

export const useRevokeKey = () =>
  useApiMutation(
    ({ id }: { id: number }) => api.delete("/api/keys/{id}", { params: { id } }),
    () => ({ stale: [queryKeys.keys.all(), audit] }),
    // The key is not the caller's to see any more: the list shows what is not so.
    (error) => (isNotFound(error) ? [queryKeys.keys.all()] : []),
  );

// providers

export const useCreateProvider = () =>
  useApiMutation(
    (body: BodyOf<"/api/providers", "post">) => api.post("/api/providers", { body }),
    () => ({ stale: [queryKeys.providers.all(), audit] }),
  );

// The provider was deleted meanwhile: the list still shows it.
const providerIsGone = (error: unknown) => (isNotFound(error) ? [queryKeys.providers.all()] : []);

export const useUpdateProvider = () =>
  useApiMutation(
    ({ id, body }: { id: number; body: BodyOf<"/api/providers/{id}", "patch"> }) =>
      api.patch("/api/providers/{id}", { params: { id }, body }),
    () => ({ stale: [queryKeys.providers.all(), audit] }),
    providerIsGone,
  );

export const useDeleteProvider = () =>
  useApiMutation(
    ({ id }: { id: number }) => api.delete("/api/providers/{id}", { params: { id } }),
    () => ({ stale: [queryKeys.providers.all(), audit] }),
    providerIsGone,
  );

// tokens

/** The answer holds the access token, which is shown once. */
export const useCreateToken = () =>
  useApiMutation(
    (body: BodyOf<"/api/tokens", "post">) => api.post("/api/tokens", { body }),
    () => ({ stale: [queryKeys.tokens.all(), audit] }),
  );

export const useRevokeToken = () =>
  useApiMutation(
    ({ id }: { id: number }) => api.delete("/api/tokens/{id}", { params: { id } }),
    () => ({ stale: [queryKeys.tokens.all(), audit] }),
  );
