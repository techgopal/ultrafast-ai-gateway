// The query keys and the hooks the pages use: one hook for each operation of
// the admin API that a page calls, one for the members of several teams, and
// one for the audit log as the pages it is read in. `me` is read by the
// session (`auth/session.tsx`) with `meOptions`, and by no page.
//
// Two rules hold for everything here. A query key holds names and ids, never
// a secret. And a mutation is dropped from the mutation cache as soon as
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
  useQuery,
  useQueryClient,
  type Query,
  type QueryKey,
} from "@tanstack/react-query";
import { api, type BodyOf } from "./client";
import { ApiError, NetworkError, type SessionOverError } from "./errors";

declare module "@tanstack/react-query" {
  interface Register {
    defaultError: ApiError | NetworkError | SessionOverError;
  }
}

/** What the key of a detail has after its area. */
const DETAIL = "detail";

/**
 * The key of one thing of an area, which is read by its id: a team, a user.
 * Every such key is built here, and `isDetailKey` knows it by what is
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
  },
  providers: {
    all: () => ["providers"] as const,
    list: () => ["providers", "list"] as const,
  },
  models: {
    all: () => ["models"] as const,
    list: () => ["models", "list"] as const,
  },
  routes: {
    all: () => ["routes"] as const,
    list: () => ["routes", "list"] as const,
    detail: detailOf("routes"),
  },
  routingHealth: () => ["routing", "health"] as const,
  tokens: {
    all: () => ["tokens"] as const,
    list: () => ["tokens", "list"] as const,
  },
  audit: {
    all: () => ["audit"] as const,
    /** The log as it is read page by page: one entry of the cache for all its pages. */
    pages: () => ["audit", "pages"] as const,
  },
  logs: {
    all: () => ["logs"] as const,
    /** The calls as they are read page by page, for one filter and one run. */
    pages: (filter: LogsFilter, run: number) => ["logs", "pages", filter, run] as const,
    detail: detailOf("logs"),
  },
  usage: {
    all: () => ["usage"] as const,
    sums: (group: UsageGroup) => ["usage", group] as const,
  },
};

/** What narrows the list of calls. A part that is not there leaves nothing out. */
export interface LogsFilter {
  from?: string;
  to?: string;
  key_id?: number;
  user_id?: number;
  team_id?: number;
  model?: string;
  errors?: boolean;
}

export type UsageGroup = "day" | "model" | "key" | "user" | "team";

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
        // A query is asked whatever the browser says about the network, as a
        // mutation is sent. The gateway can be on the same machine, where the
        // browser's "offline" says nothing about it; held back, the app would
        // say "Loading" until the browser changed its mind. Asked at once, an
        // unreachable gateway is the error that every page shows, with Retry.
        networkMode: "always",
        // What is shown is still read again when the browser says the
        // network is back. (With "always" the library would no longer do it.)
        refetchOnReconnect: true,
      },
      mutations: {
        retry: false,
        gcTime: settings.mutationGcTime ?? 0,
        // A mutation is sent whatever the browser says about the network.
        // By default the library holds it back while the browser is offline
        // and sends it when the network is back: the dialog that waits for it
        // would say "Saving" and could not be left until then. Sent at once,
        // it fails at once, and its dialog says that the gateway cannot be
        // reached.
        networkMode: "always",
      },
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

export const providersOptions = () =>
  queryOptions({
    queryKey: queryKeys.providers.list(),
    queryFn: ({ signal }) => api.get("/api/providers", { signal }),
  });

export const modelsOptions = () =>
  queryOptions({
    queryKey: queryKeys.models.list(),
    queryFn: ({ signal }) => api.get("/api/models", { signal }),
  });

export const routesOptions = () =>
  queryOptions({
    queryKey: queryKeys.routes.list(),
    queryFn: ({ signal }) => api.get("/api/routes", { signal }),
  });

export const routeOptions = (id: number) =>
  queryOptions({
    queryKey: queryKeys.routes.detail(id),
    queryFn: ({ signal }) => api.get("/api/routes/{id}", { params: { id }, signal }),
  });

export const routingHealthOptions = () =>
  queryOptions({
    queryKey: queryKeys.routingHealth(),
    queryFn: ({ signal }) => api.get("/api/routing/health", { signal }),
  });

export const tokensOptions = () =>
  queryOptions({
    queryKey: queryKeys.tokens.list(),
    queryFn: ({ signal }) => api.get("/api/tokens", { signal }),
  });

export const useSetupStatus = () => useQuery(setupStatusOptions());
export const useUsers = () => useQuery(usersOptions());
export const useUser = (id: number) => useQuery(userOptions(id));
export const useTeams = () => useQuery(teamsOptions());
export const useTeam = (id: number) => useQuery(teamOptions(id));
export const useKeys = () => useQuery(keysOptions());
export const useProviders = () => useQuery(providersOptions());
/** An admin gets every model with its grants; anybody else the models they may call. */
export const useModels = (enabled = true) => useQuery({ ...modelsOptions(), enabled });
/** An admin gets every route in full; anybody else the routes they may use, with their models only. */
export const useRoutes = (enabled = true) => useQuery({ ...routesOptions(), enabled });
export const useRoute = (id: number) => useQuery(routeOptions(id));
/** Admin only. What the gateway saw of the targets in real traffic since it started. */
export const useRoutingHealth = () => useQuery(routingHealthOptions());
export const useTokens = () => useQuery(tokensOptions());

/** How many entries a page of the audit log has. A page with fewer is the last. */
export const AUDIT_PAGE_SIZE = 50;

/**
 * The audit log, newest first, read page by page. The gateway takes the id
 * of the last entry of the page before as `before` and answers with the
 * entries below it: an entry is in one page only, whatever was added to the
 * log meanwhile. When the log is read again, each page is asked for from
 * where the page before it ends now, so no entry shows twice or out of order.
 *
 * Reading it again is one request for every page that is loaded, one after
 * the other. So it is not read again when the window gets the focus or the
 * network comes back, as the other lists are: the user starts it again
 * (`useAuditFromTheStart`), which is one request. Nor is it kept when the
 * page that shows it is left (`gcTime: 0`): opened again, the log starts
 * from its newest page, as when it was first opened.
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
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    gcTime: 0,
  });

/**
 * Starts the audit log again: the pages that were loaded are dropped, and
 * the newest page is read as it is now. One request, however many pages
 * there were. Until the answer is there the log is on its way, as when the
 * page was opened.
 */
export function useAuditFromTheStart(): () => void {
  const client = useQueryClient();
  return () => {
    void client.resetQueries({ queryKey: queryKeys.audit.pages(), exact: true });
  };
}

/** How many calls a page of the logs has. A page with fewer is the last. */
export const LOGS_PAGE_SIZE = 50;

/**
 * The calls, newest first, read page by page with `before` as the audit log
 * is. `run` is part of the key: Refresh starts a new run, which is one request
 * for the newest page. Like the audit log it is not read again by the focus
 * or the network, and not kept when its page is left.
 */
export const useLogsPages = (filter: LogsFilter, run: number) =>
  useInfiniteQuery({
    queryKey: queryKeys.logs.pages(filter, run),
    queryFn: ({ pageParam, signal }) =>
      api.get("/api/logs", {
        query: { limit: LOGS_PAGE_SIZE, ...filter, ...(pageParam === null ? {} : { before: pageParam }) },
        signal,
      }),
    initialPageParam: null as number | null,
    getNextPageParam: (last) =>
      last.logs.length < LOGS_PAGE_SIZE ? undefined : last.logs.at(-1)?.id,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    gcTime: 0,
  });

/** One call with the targets it tried. */
export const useLog = (id: number) =>
  useQuery({
    queryKey: queryKeys.logs.detail(id),
    queryFn: ({ signal }) => api.get("/api/logs/{id}", { params: { id }, signal }),
  });

/** Sums of the last 30 days (the API's default range), by day, model, key, user or team. */
export const useUsage = (group: UsageGroup) =>
  useQuery({
    queryKey: queryKeys.usage.sums(group),
    queryFn: ({ signal }) => api.get("/api/usage", { query: { group }, signal }),
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

// `meOptions` and `useLogin` are the plain calls. What a sign-in and the answer
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

// The gateway ends the other sessions of the caller and revokes all their
// access tokens. The session the call was made in goes on as it is.
export const useChangePassword = () =>
  useApiMutation(
    (body: BodyOf<"/api/auth/password", "post">) => api.post("/api/auth/password", { body }),
    () => ({ stale: [queryKeys.tokens.all(), audit] }),
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
// (A route shows the teams that may use it.)
const aTeamChanged = [
  queryKeys.teams.all(),
  queryKeys.keys.all(),
  queryKeys.me(),
  queryKeys.routes.all(),
  audit,
];
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

/** Adds the active user with this email to the team, as a member. The answer is the member. */
export const useAddTeamMember = () =>
  useApiMutation(
    ({ id, body }: { id: number; body: BodyOf<"/api/teams/{id}/members", "post"> }) =>
      api.post("/api/teams/{id}/members", { params: { id }, body }),
    () => ({ stale: membersChanged }),
    // The gateway answers 404 for a team it does not show, and also for an
    // email that is no active user's: only the first puts the team in doubt.
    (error, { id }) =>
      isNotFound(error) && !(error instanceof ApiError && error.code === "user_not_found")
        ? [queryKeys.teams.detail(id)]
        : [],
  );

/** Changes the role of a member of the team. */
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
    // The models of the provider go with it, and so do their targets in routes.
    () => ({ stale: [queryKeys.providers.all(), queryKeys.routes.all(), audit] }),
    providerIsGone,
  );

// models

// The model was deleted meanwhile: the list still shows it.
const modelIsGone = (error: unknown) => (isNotFound(error) ? [queryKeys.models.all()] : []);

/**
 * Reads the models the provider lists and adds the new ones, disabled. The
 * answer says which were added.
 */
export const useSyncProvider = () =>
  useApiMutation(
    ({ id }: { id: number }) => api.post("/api/providers/{id}/sync", { params: { id } }),
    () => ({ stale: [queryKeys.models.all(), audit] }),
    (error) => (isNotFound(error) ? [queryKeys.providers.all()] : []),
  );

export const useCreateModel = () =>
  useApiMutation(
    (body: BodyOf<"/api/models", "post">) => api.post("/api/models", { body }),
    () => ({ stale: [queryKeys.models.all(), audit] }),
  );

export const useUpdateModel = () =>
  useApiMutation(
    ({ id, body }: { id: number; body: BodyOf<"/api/models/{id}", "patch"> }) =>
      api.patch("/api/models/{id}", { params: { id }, body }),
    // A route is broken when none of its targets is enabled.
    () => ({ stale: [queryKeys.models.all(), queryKeys.routes.all(), audit] }),
    modelIsGone,
  );

/** Replaces the grants of the model: who may call it. */
export const usePutModelGrants = () =>
  useApiMutation(
    ({ id, body }: { id: number; body: BodyOf<"/api/models/{id}/grants", "put"> }) =>
      api.put("/api/models/{id}/grants", { params: { id }, body }),
    () => ({ stale: [queryKeys.models.all(), audit] }),
    modelIsGone,
  );

export const useDeleteModel = () =>
  useApiMutation(
    ({ id }: { id: number }) => api.delete("/api/models/{id}", { params: { id } }),
    // A deleted model leaves its targets with it.
    () => ({ stale: [queryKeys.models.all(), queryKeys.routes.all(), audit] }),
    modelIsGone,
  );

// routes

// The route was deleted meanwhile: the list still shows it.
const routeIsGone = (error: unknown) => (isNotFound(error) ? [queryKeys.routes.all()] : []);

export const useCreateRoute = () =>
  useApiMutation(
    (body: BodyOf<"/api/routes", "post">) => api.post("/api/routes", { body }),
    () => ({ stale: [queryKeys.routes.all(), audit] }),
  );

/** Replaces the route: the body is the whole of it. */
export const useUpdateRoute = () =>
  useApiMutation(
    ({ id, body }: { id: number; body: BodyOf<"/api/routes/{id}", "put"> }) =>
      api.put("/api/routes/{id}", { params: { id }, body }),
    () => ({ stale: [queryKeys.routes.all(), audit] }),
    routeIsGone,
  );

export const useDeleteRoute = () =>
  useApiMutation(
    ({ id }: { id: number }) => api.delete("/api/routes/{id}", { params: { id } }),
    ({ id }) => ({ stale: [queryKeys.routes.all(), audit], gone: [queryKeys.routes.detail(id)] }),
    routeIsGone,
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
