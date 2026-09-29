// The session: who is signed in, and what happens when that begins and ends.
//
// This file is the only one that sets or clears the CSRF token, that clears
// the caches, and that listens to `onUnauthenticated`. The hooks in
// `api/queries.ts` are plain calls.
//
// The rule for "a session ends once": the client tells of a 401 once, and
// again only after a token was set, which `begin` and the answer of `me` do
// here. An answer that arrives after the session ended finds its query gone
// from the cache and its observers unmounted, so it sets no token, starts no
// session and shows nobody.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createContext,
  Fragment,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { api, onUnauthenticated, setCsrfToken } from "@/api/client";
import { ApiError } from "@/api/errors";
import { meOptions, queryKeys, setupStatusOptions } from "@/api/queries";
import type { Me } from "./guards";

export type { Me } from "./guards";

export type Session =
  | { status: "loading" }
  | { status: "signedOut" }
  | { status: "signedIn"; me: Me };

/** How the last session ended: the gateway ended it, or the user signed out. */
export type Ending = "expired" | "left";

export const SESSION_ENDED_NOTICE = "Your session ended. Sign in again.";

export interface SessionControl {
  /** True while the gateway has no user yet. */
  needsSetup: boolean;
  /** Why the app could not learn who is signed in; `null` when it could. */
  problem: Error | null;
  /** Asks again after a problem. */
  retry: () => void;
  /** What the sign-in page tells the user. */
  notice: string | null;
  /** `null` when no session ended since the app loaded or since the last sign-in. */
  ending: Ending | null;
  /** A sign-in succeeded: the token of the new session. */
  begin: (csrfToken: string) => void;
  /** The session is over. Forgets the token and everything that was loaded. */
  end: (how: Ending) => void;
  /** Sets what the sign-in page tells the user. */
  announce: (notice: string) => void;
  /** The first admin exists now. */
  markSetUp: () => void;
  /**
   * The token of the invite link that was opened, while the invite is being
   * accepted; `null` at every other time. It is held here, in memory, because
   * a sign-out mounts the pages anew. The router takes it from the address
   * and drops it as soon as the address is another page.
   */
  invite: string | null;
  holdInvite: (token: string) => void;
  dropInvite: () => void;
}

interface SessionContextValue extends SessionControl {
  session: Session;
}

const SessionContext = createContext<SessionContextValue | null>(null);

const LOADING: Session = { status: "loading" };
const SIGNED_OUT: Session = { status: "signedOut" };

function isUnauthorized(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}

type Actions = Pick<
  SessionControl,
  | "notice"
  | "ending"
  | "begin"
  | "end"
  | "announce"
  | "markSetUp"
  | "invite"
  | "holdInvite"
  | "dropInvite"
>;

/** The session while it is not known to have ended: asks the gateway. */
function LiveSession({ actions, children }: { actions: Actions; children: ReactNode }) {
  const client = useQueryClient();
  const setup = useQuery(setupStatusOptions());
  const me = useQuery(meOptions());
  const { end } = actions;

  // The token of the session comes from `me`: after a reload, and whenever it
  // is asked again. Only what the cache holds now counts.
  useEffect(() => {
    const key = meOptions().queryKey;
    const take = () => {
      const token = client.getQueryData(key)?.csrf_token;
      if (typeof token === "string") setCsrfToken(token);
    };
    take();
    return client.getQueryCache().subscribe((event) => {
      if (event.type !== "updated" || event.action.type !== "success") return;
      if (client.getQueryCache().find({ queryKey: key, exact: true }) !== event.query) return;
      take();
    });
  }, [client]);

  // `me` is asked again now and then. A 401 then says that the session ended.
  const expired = me.data !== undefined && isUnauthorized(me.error);
  useEffect(() => {
    if (expired) end("expired");
  }, [expired, end]);

  const { refetch: askSetup } = setup;
  const { refetch: askMe } = me;
  const retry = useCallback(() => {
    void askSetup();
    void askMe();
  }, [askSetup, askMe]);

  const value = useMemo((): SessionContextValue => {
    const signedOut = isUnauthorized(me.error);
    let problem: Error | null = null;
    if (setup.data === undefined && setup.error !== null) problem = setup.error;
    else if (me.data === undefined && me.error !== null && !signedOut) problem = me.error;

    let session: Session = LOADING;
    if (setup.data !== undefined && problem === null) {
      if (signedOut) session = SIGNED_OUT;
      else if (me.data !== undefined) session = { status: "signedIn", me: me.data };
    }
    return {
      ...actions,
      session,
      needsSetup: setup.data?.needs_setup === true,
      problem,
      retry,
    };
  }, [actions, setup.data, setup.error, me.data, me.error, retry]);

  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}

const nothing = () => undefined;

export function SessionProvider({ children }: { children: ReactNode }) {
  const client = useQueryClient();
  const [ending, setEnding] = useState<Ending | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [invite, setInvite] = useState<string | null>(null);
  const dropInvite = useCallback(() => {
    setInvite(null);
  }, []);

  const end = useCallback(
    (how: Ending) => {
      setCsrfToken(null);
      setEnding(how);
      setNotice(how === "expired" ? SESSION_ENDED_NOTICE : null);
      // Both caches. Requests on their way are cancelled with their queries.
      client.clear();
    },
    [client],
  );

  const begin = useCallback(
    (csrfToken: string) => {
      setCsrfToken(csrfToken);
      setEnding(null);
      setNotice(null);
      // Nobody was signed in a moment ago; now `me` has an answer.
      void client.resetQueries({ queryKey: queryKeys.me() });
    },
    [client],
  );

  const markSetUp = useCallback(() => {
    client.setQueryData(setupStatusOptions().queryKey, { needs_setup: false });
  }, [client]);

  useEffect(
    () =>
      onUnauthenticated(() => {
        end("expired");
      }),
    [end],
  );

  const actions = useMemo(
    (): Actions => ({
      notice,
      ending,
      begin,
      end,
      announce: setNotice,
      markSetUp,
      invite,
      holdInvite: setInvite,
      dropInvite,
    }),
    [notice, ending, begin, end, markSetUp, invite, dropInvite],
  );
  const ended = useMemo(
    (): SessionContextValue => ({
      ...actions,
      session: SIGNED_OUT,
      needsSetup: false,
      problem: null,
      retry: nothing,
    }),
    [actions],
  );

  // What is below is mounted anew when a session ends: every dialog closes,
  // and what a page kept in its state is gone.
  if (ending !== null) {
    return (
      <SessionContext.Provider value={ended}>
        <Fragment key="ended">{children}</Fragment>
      </SessionContext.Provider>
    );
  }
  return (
    <LiveSession key="live" actions={actions}>
      {children}
    </LiveSession>
  );
}

function useSessionContext(): SessionContextValue {
  const value = useContext(SessionContext);
  if (value === null) throw new Error("There is no SessionProvider above.");
  return value;
}

export function useSession(): Session {
  return useSessionContext().session;
}

/** For the guards of the routes and the sign-in, setup and invite pages. */
export function useSessionControl(): SessionControl {
  return useSessionContext();
}

/**
 * Signs out: tells the gateway, then forgets the session. The session is
 * forgotten also when the gateway could not be told or refused.
 */
export function useSignOut(): () => Promise<void> {
  const { end } = useSessionContext();
  return useCallback(async () => {
    try {
      await api.post("/api/auth/logout");
    } catch {
      // The console forgets the session whatever became of the call.
    } finally {
      end("left");
    }
  }, [end]);
}
