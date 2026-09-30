import { QueryClientProvider, type QueryClient, type QueryKey } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, test, vi } from "vitest";
import * as fixtures from "@/test/fixtures";
import { errors } from "@/test/errors";
import { networkFailure, noContent, ok, override, refuse } from "@/test/handlers";
import { api, onUnauthenticated } from "./client";
import { ApiError, NetworkError } from "./errors";
import * as q from "./queries";

function wrapperOf(client: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

/** A client with the app's rules, and no wait between the tries. */
const appClient = () => q.createQueryClient({ retryDelay: 0 });

/** Counts the calls of a GET, which answers as before. */
function counted(path: "/api/teams" | "/api/keys" | "/api/users" | "/api/audit" | "/api/auth/me") {
  const count = { calls: 0 };
  const answers = {
    "/api/teams": () => ok("get", "/api/teams", 200, { teams: fixtures.teamList }),
    "/api/keys": () => ok("get", "/api/keys", 200, { keys: fixtures.keyList }),
    "/api/users": () => ok("get", "/api/users", 200, { users: fixtures.userList }),
    "/api/audit": () => ok("get", "/api/audit", 200, { entries: fixtures.auditEntries }),
    "/api/auth/me": () => ok("get", "/api/auth/me", 200, fixtures.me.maya),
  };
  override("get", path, () => {
    count.calls += 1;
    return answers[path]();
  });
  return count;
}

/** Everything the two caches hold, as one text. */
function cached(client: QueryClient): string {
  const queries = client
    .getQueryCache()
    .getAll()
    .map((query) => ({ key: query.queryKey, state: query.state }));
  const mutations = client
    .getMutationCache()
    .getAll()
    .map((mutation) => ({ key: mutation.options.mutationKey, state: mutation.state }));
  return JSON.stringify({ queries, mutations });
}

describe("queries", () => {
  test("lists and details answer from the API", async () => {
    const wrapper = wrapperOf(appClient());
    const { result } = renderHook(
      () => ({
        setup: q.useSetupStatus(),
        me: q.useMe(),
        users: q.useUsers(),
        user: q.useUser(fixtures.users.arjun.id),
        teams: q.useTeams(),
        team: q.useTeam(fixtures.teams.platform.id),
        keys: q.useKeys(),
        key: q.useKey(fixtures.keys.expired.id),
        providers: q.useProviders(),
        tokens: q.useTokens(),
        audit: q.useAuditLog(),
      }),
      { wrapper },
    );
    await waitFor(() => {
      expect(Object.values(result.current).every((query) => query.isSuccess)).toBe(true);
    });
    expect(result.current.setup.data).toEqual({ needs_setup: false });
    expect(result.current.me.data).toEqual(fixtures.me.maya);
    expect(result.current.users.data).toEqual({ users: fixtures.userList });
    expect(result.current.user.data).toEqual(fixtures.users.arjun);
    expect(result.current.teams.data).toEqual({ teams: fixtures.teamList });
    expect(result.current.team.data).toEqual(fixtures.teamDetails.platform);
    expect(result.current.keys.data).toEqual({ keys: fixtures.keyList });
    expect(result.current.key.data).toEqual(fixtures.keys.expired);
    expect(result.current.providers.data).toEqual({ providers: fixtures.providerList });
    expect(result.current.tokens.data).toEqual({ tokens: fixtures.tokenList });
    expect(result.current.audit.data).toEqual({ entries: fixtures.auditEntries });
  });

  test("the audit log asks for the page", async () => {
    let search = "";
    override("get", "/api/audit", ({ request }) => {
      search = new URL(request.url).search;
      return ok("get", "/api/audit", 200, { entries: [] });
    });
    const { result } = renderHook(() => q.useAuditLog({ limit: 20, before: 3 }), {
      wrapper: wrapperOf(appClient()),
    });
    await waitFor(() => {
      expect(result.current.isSuccess).toBe(true);
    });
    expect(search).toBe("?limit=20&before=3");
  });

  test("the teams with their members are one call for each team", async () => {
    const asked: string[] = [];
    override("get", "/api/teams/{id}", ({ params }) => {
      asked.push(params.id ?? "");
      const detail = fixtures.teamDetailList.find((one) => String(one.team.id) === params.id);
      return detail === undefined
        ? refuse(errors.not_found)
        : ok("get", "/api/teams/{id}", 200, detail);
    });
    const client = appClient();
    const ids = [fixtures.teams.platform.id, fixtures.teams.research.id];
    const { result, rerender } = renderHook(({ of }: { of: number[] }) => q.useTeamDetails(of), {
      wrapper: wrapperOf(client),
      initialProps: { of: ids },
    });
    await waitFor(() => {
      expect(result.current.every((query) => query.isSuccess)).toBe(true);
    });
    expect(result.current.map((query) => query.data)).toEqual([
      fixtures.teamDetails.platform,
      fixtures.teamDetails.research,
    ]);
    expect(asked.sort()).toEqual(ids.map(String));
    // They are the queries of the pages of the teams: a change of a team reaches them.
    expect(client.getQueryData(q.queryKeys.teams.detail(ids[0] ?? 0))).toEqual(
      fixtures.teamDetails.platform,
    );
    // No team, no call.
    rerender({ of: [] });
    expect(result.current).toEqual([]);
    expect(asked).toHaveLength(2);
  });

  test("a 404 is an ApiError of the query", async () => {
    const { result } = renderHook(() => q.useTeam(999), { wrapper: wrapperOf(appClient()) });
    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });
    const { error } = result.current;
    expect(error).toBeInstanceOf(ApiError);
    expect(error instanceof ApiError ? error.status : null).toBe(404);
  });
});

describe("retries", () => {
  test("api errors are not retried", async () => {
    let calls = 0;
    override("get", "/api/teams", () => {
      calls += 1;
      return refuse(errors.internal_error);
    });
    const { result } = renderHook(() => q.useTeams(), { wrapper: wrapperOf(appClient()) });
    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });
    expect(result.current.error).toBeInstanceOf(ApiError);
    expect(calls).toBe(1);
  });

  test("network errors are retried twice", async () => {
    let calls = 0;
    override("get", "/api/teams", () => {
      calls += 1;
      return networkFailure();
    });
    const { result } = renderHook(() => q.useTeams(), { wrapper: wrapperOf(appClient()) });
    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });
    expect(result.current.error).toBeInstanceOf(NetworkError);
    expect(calls).toBe(3);
  });

  test("a query recovers when the network does", async () => {
    let calls = 0;
    override("get", "/api/teams", () => {
      calls += 1;
      return calls < 3
        ? networkFailure()
        : ok("get", "/api/teams", 200, { teams: fixtures.teamList });
    });
    const { result } = renderHook(() => q.useTeams(), { wrapper: wrapperOf(appClient()) });
    await waitFor(() => {
      expect(result.current.isSuccess).toBe(true);
    });
    expect(calls).toBe(3);
  });

  test("mutations never retry", async () => {
    let calls = 0;
    override("post", "/api/teams", () => {
      calls += 1;
      return networkFailure();
    });
    const { result } = renderHook(() => q.useCreateTeam(), { wrapper: wrapperOf(appClient()) });
    await act(async () => {
      await expect(result.current.mutateAsync({ name: "Growth" })).rejects.toBeInstanceOf(
        NetworkError,
      );
    });
    expect(calls).toBe(1);
  });
});

describe("what the gateway hides is dropped", () => {
  const details = [
    [
      "a team",
      "/api/teams/{id}",
      () => q.useTeam(fixtures.teams.platform.id),
      q.queryKeys.teams.detail(fixtures.teams.platform.id),
    ],
    [
      "a user",
      "/api/users/{id}",
      () => q.useUser(fixtures.users.lena.id),
      q.queryKeys.users.detail(fixtures.users.lena.id),
    ],
    [
      "a key",
      "/api/keys/{id}",
      () => q.useKey(fixtures.keys.expired.id),
      q.queryKeys.keys.detail(fixtures.keys.expired.id),
    ],
  ] as const;

  const others = [
    ["setup", "/api/setup", () => q.useSetupStatus(), q.queryKeys.setup()],
    ["the caller", "/api/auth/me", () => q.useMe(), q.queryKeys.me()],
    ["the list of users", "/api/users", () => q.useUsers(), q.queryKeys.users.list()],
    ["the list of teams", "/api/teams", () => q.useTeams(), q.queryKeys.teams.list()],
    ["the list of keys", "/api/keys", () => q.useKeys(), q.queryKeys.keys.list()],
    ["the list of providers", "/api/providers", () => q.useProviders(), q.queryKeys.providers.list()],
    ["the list of tokens", "/api/tokens", () => q.useTokens(), q.queryKeys.tokens.list()],
    ["the audit log", "/api/audit", () => q.useAuditLog(), q.queryKeys.audit.list()],
  ] as const;

  /** Asks for it again, and waits for the answer. */
  async function askAgain(client: QueryClient, queryKey: QueryKey): Promise<void> {
    await act(async () => {
      await client.invalidateQueries({ queryKey, exact: true });
    });
  }

  test.each(details)(
    "%s that answers 404 when asked for again is kept no longer",
    async (_, path, useOne, queryKey) => {
      const client = appClient();
      const { result } = renderHook(() => useOne(), { wrapper: wrapperOf(client) });
      await waitFor(() => {
        expect(result.current.isSuccess).toBe(true);
      });
      expect(client.getQueryData(queryKey)).toBeDefined();

      override("get", path, () => refuse(errors.not_found));
      await askAgain(client, queryKey);
      await waitFor(() => {
        expect(result.current.isError).toBe(true);
      });
      // Who shows it has nothing to show but the answer.
      expect(result.current.data).toBeUndefined();
      expect(result.current.error).toMatchObject({ status: 404 });
      // And the cache holds nothing of it.
      expect(client.getQueryData(queryKey)).toBeUndefined();
      expect(client.getQueryState(queryKey)).toMatchObject({ status: "error", dataUpdatedAt: 0 });
    },
  );

  test.each(details)(
    "%s is kept when asking again fails with something else",
    async (_, path, useOne, queryKey) => {
      const client = appClient();
      const { result } = renderHook(() => useOne(), { wrapper: wrapperOf(client) });
      await waitFor(() => {
        expect(result.current.isSuccess).toBe(true);
      });
      const loaded: unknown = result.current.data;
      expect(loaded).toBeDefined();

      for (const [answer, kind] of [
        [() => refuse(errors.internal_error), ApiError],
        [() => refuse(errors.forbidden), ApiError],
        [networkFailure, NetworkError],
      ] as const) {
        override("get", path, answer);
        await askAgain(client, queryKey);
        await waitFor(() => {
          expect(result.current.error).toBeInstanceOf(kind);
        });
        expect(result.current.data).toEqual(loaded);
        expect(client.getQueryData(queryKey)).toEqual(loaded);
      }
    },
  );

  // The rule is for what is one thing with an id. The app goes on with who is
  // signed in and with its lists, whatever a 404 of them would mean.
  test.each(others)("a 404 of %s drops nothing", async (_, path, useOne, queryKey) => {
    const client = appClient();
    const { result } = renderHook(() => useOne(), { wrapper: wrapperOf(client) });
    await waitFor(() => {
      expect(result.current.isSuccess).toBe(true);
    });
    const loaded: unknown = result.current.data;
    expect(loaded).toBeDefined();

    override("get", path, () => refuse(errors.not_found));
    await askAgain(client, queryKey);
    await waitFor(() => {
      expect(result.current.error).toMatchObject({ status: 404 });
    });
    expect(result.current.data).toEqual(loaded);
    expect(client.getQueryData(queryKey)).toEqual(loaded);
  });

  test("a key is one of a detail by how it is built, not by its name", () => {
    // Every area that has a `detail`, also one that is added later.
    const built = Object.entries(q.queryKeys).flatMap(([area, keys]) =>
      "detail" in keys ? [[area, keys.detail(7)] as const] : [],
    );
    expect(built.map(([area]) => area)).toEqual(expect.arrayContaining(["users", "teams", "keys"]));
    for (const [area, key] of built) expect([area, q.isDetailKey(key)]).toEqual([area, true]);

    for (const key of [
      q.queryKeys.setup(),
      q.queryKeys.me(),
      q.queryKeys.users.all(),
      q.queryKeys.users.list(),
      q.queryKeys.teams.all(),
      q.queryKeys.teams.list(),
      q.queryKeys.keys.all(),
      q.queryKeys.keys.list(),
      q.queryKeys.providers.all(),
      q.queryKeys.providers.list(),
      q.queryKeys.tokens.all(),
      q.queryKeys.tokens.list(),
      q.queryKeys.audit.all(),
      q.queryKeys.audit.list({ limit: 10, before: 5 }),
    ]) {
      expect([key, q.isDetailKey(key)]).toEqual([key, false]);
    }
  });
});

describe("mutations invalidate", () => {
  test("after create team the teams list refetches", async () => {
    const teams = counted("/api/teams");
    const audit = counted("/api/audit");
    const users = counted("/api/users");
    const { result } = renderHook(
      () => ({
        teams: q.useTeams(),
        audit: q.useAuditLog(),
        users: q.useUsers(),
        create: q.useCreateTeam(),
      }),
      { wrapper: wrapperOf(appClient()) },
    );
    await waitFor(() => {
      expect(result.current.teams.isSuccess && result.current.audit.isSuccess).toBe(true);
    });
    expect(teams.calls).toBe(1);

    let created: unknown;
    await act(async () => {
      created = await result.current.create.mutateAsync({ name: "Growth" });
    });
    expect(created).toEqual(fixtures.teams.growth);
    await waitFor(() => {
      expect(teams.calls).toBe(2);
    });
    await waitFor(() => {
      expect(audit.calls).toBe(2);
    });
    // What the mutation does not affect is left alone.
    expect(users.calls).toBe(1);
  });

  test("a failed mutation invalidates nothing", async () => {
    const teams = counted("/api/teams");
    override("post", "/api/teams", () => refuse(errors.team_exists));
    const { result } = renderHook(() => ({ teams: q.useTeams(), create: q.useCreateTeam() }), {
      wrapper: wrapperOf(appClient()),
    });
    await waitFor(() => {
      expect(result.current.teams.isSuccess).toBe(true);
    });
    await act(async () => {
      await expect(result.current.create.mutateAsync({ name: "Platform" })).rejects.toBeInstanceOf(
        ApiError,
      );
    });
    expect(teams.calls).toBe(1);
  });

  test("a 404 of a change of a member asks for the team again; another refusal asks for nothing", async () => {
    let detail = 0;
    override("get", "/api/teams/{id}", () => {
      detail += 1;
      return ok("get", "/api/teams/{id}", 200, fixtures.teamDetails.platform);
    });
    const teams = counted("/api/teams");
    const me = counted("/api/auth/me");
    const id = fixtures.teams.platform.id;
    const { result } = renderHook(
      () => ({ team: q.useTeam(id), teams: q.useTeams(), me: q.useMe(), put: q.usePutTeamMember() }),
      { wrapper: wrapperOf(appClient()) },
    );
    await waitFor(() => {
      expect(
        result.current.team.isSuccess && result.current.teams.isSuccess && result.current.me.isSuccess,
      ).toBe(true);
    });
    const add = () =>
      result.current.put.mutateAsync({ id, userId: 999, body: { role: "member" } });

    override("put", "/api/teams/{id}/members/{user_id}", () => refuse(errors.forbidden));
    await act(async () => {
      await expect(add()).rejects.toMatchObject({ status: 403 });
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    expect([detail, teams.calls, me.calls]).toEqual([1, 1, 1]);

    // The gateway answers 404 for a team it does not show as for a user it does not know.
    override("put", "/api/teams/{id}/members/{user_id}", () => refuse(errors.not_found));
    await act(async () => {
      await expect(add()).rejects.toMatchObject({ status: 404 });
    });
    await waitFor(() => {
      expect(detail).toBe(2);
    });
    // Only the team is in doubt.
    expect([teams.calls, me.calls]).toEqual([1, 1]);
  });

  test.each([
    [
      "revoking a key",
      "/api/keys",
      (answer: () => Response) => {
        override("delete", "/api/keys/{id}", answer);
      },
      () => q.useRevokeKey(),
      { id: 999 },
    ],
    [
      "changing a provider",
      "/api/providers",
      (answer: () => Response) => {
        override("patch", "/api/providers/{id}", answer);
      },
      () => q.useUpdateProvider(),
      { id: 999, body: { api_key: null } },
    ],
    [
      "deleting a provider",
      "/api/providers",
      (answer: () => Response) => {
        override("delete", "/api/providers/{id}", answer);
      },
      () => q.useDeleteProvider(),
      { id: 999 },
    ],
  ] as const)(
    "a 404 of %s asks for the list again; another refusal asks for nothing",
    async (_, list, answerWith, useChange, variables) => {
      let lists = 0;
      override("get", list, () => {
        lists += 1;
        return list === "/api/keys"
          ? ok("get", "/api/keys", 200, { keys: fixtures.keyList })
          : ok("get", "/api/providers", 200, { providers: fixtures.providerList });
      });
      const audit = counted("/api/audit");
      const { result } = renderHook(
        () => ({
          list: list === "/api/keys" ? q.useKeys() : q.useProviders(),
          audit: q.useAuditLog(),
          change: useChange(),
        }),
        { wrapper: wrapperOf(appClient()) },
      );
      await waitFor(() => {
        expect(result.current.list.isSuccess && result.current.audit.isSuccess).toBe(true);
      });
      const change = () => result.current.change.mutateAsync(variables as never);

      answerWith(() => refuse(errors.forbidden));
      await act(async () => {
        await expect(change()).rejects.toMatchObject({ status: 403 });
        await new Promise((resolve) => setTimeout(resolve, 30));
      });
      expect([lists, audit.calls]).toEqual([1, 1]);

      // It is gone, or hidden from the caller: what the list shows is in doubt.
      answerWith(() => refuse(errors.not_found));
      await act(async () => {
        await expect(change()).rejects.toMatchObject({ status: 404 });
      });
      await waitFor(() => {
        expect(lists).toBe(2);
      });
      // Nothing was changed: nothing was recorded.
      expect(audit.calls).toBe(1);
    },
  );

  test("a change of a user refetches users, keys, teams and the caller", async () => {
    const users = counted("/api/users");
    const keys = counted("/api/keys");
    const teams = counted("/api/teams");
    const me = counted("/api/auth/me");
    let detail = 0;
    override("get", "/api/users/{id}", () => {
      detail += 1;
      return ok("get", "/api/users/{id}", 200, fixtures.users.dana);
    });
    const { result } = renderHook(
      () => ({
        users: q.useUsers(),
        user: q.useUser(fixtures.users.dana.id),
        keys: q.useKeys(),
        teams: q.useTeams(),
        me: q.useMe(),
        update: q.useUpdateUser(),
      }),
      { wrapper: wrapperOf(appClient()) },
    );
    await waitFor(() => {
      expect(
        [result.current.users, result.current.user, result.current.keys, result.current.teams]
          .concat([])
          .every((query) => query.isSuccess) && result.current.me.isSuccess,
      ).toBe(true);
    });
    await act(async () => {
      await result.current.update.mutateAsync({
        id: fixtures.users.dana.id,
        body: { status: "active" },
      });
    });
    await waitFor(() => {
      expect([users.calls, detail, keys.calls, teams.calls, me.calls]).toEqual([2, 2, 2, 2, 2]);
    });
  });

  test("a deleted team is not asked for again", async () => {
    let detail = 0;
    override("get", "/api/teams/{id}", () => {
      detail += 1;
      return ok("get", "/api/teams/{id}", 200, fixtures.teamDetails.growth);
    });
    const teams = counted("/api/teams");
    const client = appClient();
    const { result } = renderHook(
      () => ({ teams: q.useTeams(), remove: q.useDeleteTeam() }),
      { wrapper: wrapperOf(client) },
    );
    await client.query(q.teamOptions(fixtures.teams.growth.id));
    await waitFor(() => {
      expect(result.current.teams.isSuccess).toBe(true);
    });
    await act(async () => {
      await result.current.remove.mutateAsync({ id: fixtures.teams.growth.id });
    });
    await waitFor(() => {
      expect(teams.calls).toBe(2);
    });
    expect(detail).toBe(1);
    expect(client.getQueryData(q.queryKeys.teams.detail(fixtures.teams.growth.id))).toBeUndefined();
  });

  test("a team the caller left is not asked for again, and is kept while its page shows it; the team of another removal is asked for again", async () => {
    let detail = 0;
    override("get", "/api/teams/{id}", () => {
      detail += 1;
      return ok("get", "/api/teams/{id}", 200, fixtures.teamDetails.platform);
    });
    const teams = counted("/api/teams");
    const me = counted("/api/auth/me");
    const client = appClient();
    const id = fixtures.teams.platform.id;
    // The team is shown, as on its page.
    const { result } = renderHook(
      () => ({
        team: q.useTeam(id),
        teams: q.useTeams(),
        me: q.useMe(),
        remove: q.useRemoveTeamMember(),
      }),
      { wrapper: wrapperOf(client) },
    );
    await waitFor(() => {
      expect(
        result.current.team.isSuccess && result.current.teams.isSuccess && result.current.me.isSuccess,
      ).toBe(true);
    });
    await act(async () => {
      await result.current.remove.mutateAsync({ id, userId: fixtures.users.lena.id });
    });
    await waitFor(() => {
      expect([detail, teams.calls, me.calls]).toEqual([2, 2, 2]);
    });

    await act(async () => {
      await result.current.remove.mutateAsync({
        id,
        userId: fixtures.users.arjun.id,
        leaving: true,
      });
    });
    await waitFor(() => {
      expect([teams.calls, me.calls]).toEqual([3, 3]);
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    // Asked for again, the team would answer 404 to the page that still shows
    // it; dropped, the page would ask for it. The page drops it when it has gone.
    expect(detail).toBe(2);
    expect(client.getQueryData(q.queryKeys.teams.detail(id))).toEqual(fixtures.teamDetails.platform);
    expect(client.getQueryState(q.queryKeys.teams.detail(id))?.isInvalidated).toBe(false);
    expect(result.current.team.data).toEqual(fixtures.teamDetails.platform);
  });

  // The session (`auth/session.tsx`) owns the CSRF token and the caches; its
  // tests say what a sign-in, a sign-out and `me` do to them.
  test("the hooks of the session are plain calls", async () => {
    const client = appClient();
    const { result } = renderHook(
      () => ({ me: q.useMe(), login: q.useLogin() }),
      { wrapper: wrapperOf(client) },
    );
    await waitFor(() => {
      expect(result.current.me.isSuccess).toBe(true);
    });
    await client.query(q.usersOptions());
    const sent: (string | null)[] = [];
    override("post", "/api/teams", ({ request }) => {
      sent.push(request.headers.get("x-csrf-token"));
      return noContent();
    });

    await api.post("/api/teams", { body: { name: "x" } });
    await act(async () => {
      await result.current.login.mutateAsync({ email: "maya@example.test", password: "p" });
    });
    await api.post("/api/teams", { body: { name: "x" } });
    expect(sent).toEqual([null, null]);

    expect(client.getQueryData(q.queryKeys.users.list())).toBeDefined();
    expect(client.getQueryData(q.queryKeys.me())).toBeDefined();
  });

  test("a wrong current password does not sign out", async () => {
    const handler = vi.fn();
    const unsubscribe = onUnauthenticated(handler);
    override("post", "/api/auth/password", () => refuse(errors.invalid_credentials));
    const { result } = renderHook(() => q.useChangePassword(), {
      wrapper: wrapperOf(appClient()),
    });
    await act(async () => {
      await expect(
        result.current.mutateAsync({ current_password: "wrong", new_password: "new-password" }),
      ).rejects.toBeInstanceOf(ApiError);
    });
    expect(handler).not.toHaveBeenCalled();
    unsubscribe();
  });
});

describe("every mutation calls its operation", () => {
  const cases: [string, string, () => { mutateAsync: (v: never) => Promise<unknown> }, unknown][] =
    [
      ["useSetup", "POST /api/setup", q.useSetup, { email: "a@example.test", name: "A", password: "p" }],
      ["useLogin", "POST /api/auth/login", q.useLogin, { email: "a@example.test", password: "p" }],
      ["useAcceptInvite", "POST /api/auth/accept-invite", q.useAcceptInvite, { token: "t", password: "p" }],
      ["useChangePassword", "POST /api/auth/password", q.useChangePassword, { current_password: "a", new_password: "b" }],
      ["useInviteUser", "POST /api/users", q.useInviteUser, { email: "a@example.test", name: "A", role: "member" }],
      ["useUpdateUser", "PATCH /api/users/3", q.useUpdateUser, { id: 3, body: { name: "L" } }],
      ["useDeleteUser", "DELETE /api/users/3", q.useDeleteUser, { id: 3 }],
      ["useReinviteUser", "POST /api/users/6/invite", q.useReinviteUser, { id: 6 }],
      ["useCreateTeam", "POST /api/teams", q.useCreateTeam, { name: "Growth" }],
      ["useRenameTeam", "PATCH /api/teams/2", q.useRenameTeam, { id: 2, body: { name: "R" } }],
      ["useDeleteTeam", "DELETE /api/teams/3", q.useDeleteTeam, { id: 3 }],
      ["usePutTeamMember", "PUT /api/teams/1/members/5", q.usePutTeamMember, { id: 1, userId: 5, body: { role: "member" } }],
      ["useRemoveTeamMember", "DELETE /api/teams/1/members/3", q.useRemoveTeamMember, { id: 1, userId: 3 }],
      ["useCreateKey", "POST /api/keys", q.useCreateKey, { name: "k" }],
      ["useRevokeKey", "DELETE /api/keys/1", q.useRevokeKey, { id: 1 }],
      ["useCreateProvider", "POST /api/providers", q.useCreateProvider, { name: "p", kind: "openai", base_url: "u" }],
      ["useUpdateProvider", "PATCH /api/providers/2", q.useUpdateProvider, { id: 2, body: { api_key: null } }],
      ["useDeleteProvider", "DELETE /api/providers/2", q.useDeleteProvider, { id: 2 }],
      ["useCreateToken", "POST /api/tokens", q.useCreateToken, { name: "t" }],
      ["useRevokeToken", "DELETE /api/tokens/1", q.useRevokeToken, { id: 1 }],
    ];

  // Signing out has no hook here: it goes through `useSignOut` of the session only.
  test("there are 20 of them, and 12 queries", () => {
    expect(cases).toHaveLength(20);
    const hooks = Object.keys(q).filter((name) => /^use[A-Z]/.test(name));
    expect(hooks).toHaveLength(32);
    expect(hooks).toContain("useTeamDetails");
    expect(hooks).not.toContain("useLogout");
    expect(hooks).toEqual(expect.arrayContaining(cases.map(([name]) => name)));
  });

  test.each(cases)("%s calls %s", async (_, operation, useHook, variables) => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const { result } = renderHook(() => useHook(), { wrapper: wrapperOf(appClient()) });
    await act(async () => {
      await result.current.mutateAsync(variables as never);
    });
    const [url, init] = fetchSpy.mock.calls[0] ?? [];
    expect(`${String(init?.method)} ${typeof url === "string" ? url : "not a text"}`).toBe(operation);
    fetchSpy.mockRestore();
  });
});

describe("secrets stay out of the caches", () => {
  const password = "a-password-that-is-secret";
  const apiKey = "sk-provider-api-key-that-is-secret";
  const cases: [string, () => { mutateAsync: (v: never) => Promise<unknown> }, unknown, string[]][] =
    [
      ["useCreateKey", q.useCreateKey, { name: "k" }, [fixtures.newKeySecret]],
      ["useCreateToken", q.useCreateToken, { name: "t" }, [fixtures.newTokenSecret]],
      ["useInviteUser", q.useInviteUser, { email: "s@example.test", name: "S", role: "member" }, [fixtures.newInviteLink]],
      ["useReinviteUser", q.useReinviteUser, { id: 6 }, [fixtures.newInviteLink]],
      ["useLogin", q.useLogin, { email: "m@example.test", password }, [password]],
      ["useSetup", q.useSetup, { email: "m@example.test", name: "M", password }, [password]],
      ["useAcceptInvite", q.useAcceptInvite, { token: "invite-token-that-is-secret", password }, [password, "invite-token-that-is-secret"]],
      ["useChangePassword", q.useChangePassword, { current_password: password, new_password: `${password}-new` }, [password]],
      ["useCreateProvider", q.useCreateProvider, { name: "p", kind: "openai", base_url: "u", api_key: apiKey }, [apiKey]],
      ["useUpdateProvider", q.useUpdateProvider, { id: 1, body: { api_key: apiKey } }, [apiKey]],
    ];

  test.each(cases)("%s", async (_, useHook, variables, secrets) => {
    const client = appClient();
    const { result, unmount } = renderHook(
      () => ({
        mutation: useHook(),
        keys: q.useKeys(),
        tokens: q.useTokens(),
        users: q.useUsers(),
        providers: q.useProviders(),
      }),
      { wrapper: wrapperOf(client) },
    );
    let answer: unknown;
    await act(async () => {
      answer = await result.current.mutation.mutateAsync(variables as never);
    });
    // The caller of the mutation is the one who gets the secret.
    if (secrets[0] === fixtures.newKeySecret) {
      expect(answer).toEqual({ key: fixtures.keys.active, secret: fixtures.newKeySecret });
    }
    await waitFor(() => {
      expect(client.isFetching()).toBe(0);
    });
    for (const secret of secrets) {
      expect(JSON.stringify(client.getQueryCache().getAll().map((c) => [c.queryKey, c.state]))).not.toContain(secret);
    }

    unmount();
    await waitFor(() => {
      expect(client.getMutationCache().getAll()).toHaveLength(0);
    });
    for (const secret of secrets) expect(cached(client)).not.toContain(secret);
  });

  test("the scan of the caches sees a secret that is there", async () => {
    const client = q.createQueryClient({ retryDelay: 0, mutationGcTime: 60_000 });
    client.setQueryData(["probe"], { secret: "probe-secret" });
    expect(cached(client)).toContain("probe-secret");
    await client
      .getMutationCache()
      .build(client, { mutationFn: () => Promise.resolve("mutation-secret") })
      .execute(undefined);
    expect(cached(client)).toContain("mutation-secret");
  });

  test("query keys hold ids and page numbers only", () => {
    const keys = [
      q.queryKeys.setup(),
      q.queryKeys.me(),
      q.queryKeys.users.all(),
      q.queryKeys.users.list(),
      q.queryKeys.users.detail(1),
      q.queryKeys.teams.list(),
      q.queryKeys.teams.detail(1),
      q.queryKeys.keys.list(),
      q.queryKeys.keys.detail(1),
      q.queryKeys.providers.list(),
      q.queryKeys.tokens.list(),
      q.queryKeys.audit.list({ limit: 10, before: 5 }),
    ];
    expect(new Set(keys.map((key) => JSON.stringify(key))).size).toBe(keys.length);
    expect(q.queryKeys.users.detail(1).slice(0, 1)).toEqual(q.queryKeys.users.all());
    expect(q.queryKeys.users.list().slice(0, 1)).toEqual(q.queryKeys.users.all());
  });
});

describe("the render helper", () => {
  test("every render has a query client of its own, which does not retry", async () => {
    const { renderWithApp } = await import("@/test/render");
    let calls = 0;
    override("get", "/api/teams", () => {
      calls += 1;
      return networkFailure();
    });
    function Teams() {
      const teams = q.useTeams();
      return <p>{teams.isError ? "failed" : teams.isSuccess ? "loaded" : "loading"}</p>;
    }
    const first = await renderWithApp(<Teams />);
    await first.findByText("failed");
    expect(calls).toBe(1);
    first.unmount();

    const second = await renderWithApp(<Teams />);
    expect(second.queryClient).not.toBe(first.queryClient);
    // At most what this render asks for: the page's teams, and the session's `setup` and `me`.
    expect(second.queryClient.getQueryCache().getAll().length).toBeLessThanOrEqual(3);
    await second.findByText("failed");
    expect(calls).toBe(2);
  });
});
