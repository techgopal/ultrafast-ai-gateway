// A gateway that remembers whether somebody is signed in, for the tests of
// the session: sign-in starts a session, sign-out ends it, and `me` and
// `setup` answer accordingly.
import * as fixtures from "./fixtures";
import { apiError, noContent, ok, override } from "./handlers";

export const PASSWORD = "correct-horse-battery-staple";

export interface Gateway {
  signedIn: boolean;
  needsSetup: boolean;
  /** The CSRF token of the next session. */
  csrfToken: string;
  /** The bodies of the sign-in calls. */
  logins: unknown[];
  logouts: number;
  meCalls: number;
}

export function startGateway(
  start: { signedIn?: boolean; needsSetup?: boolean; me?: fixtures.Me } = {},
): Gateway {
  const me = start.me ?? fixtures.me.maya;
  const gateway: Gateway = {
    signedIn: start.signedIn ?? false,
    needsSetup: start.needsSetup ?? false,
    csrfToken: me.csrf_token ?? fixtures.csrfToken,
    logins: [],
    logouts: 0,
    meCalls: 0,
  };
  override("get", "/api/setup", () =>
    ok("get", "/api/setup", 200, { needs_setup: gateway.needsSetup }),
  );
  override("get", "/api/auth/me", () => {
    gateway.meCalls += 1;
    return gateway.signedIn
      ? ok("get", "/api/auth/me", 200, { ...me, csrf_token: gateway.csrfToken })
      : apiError(401, "unauthorized", "Sign in to continue.");
  });
  override("post", "/api/auth/login", async ({ request }) => {
    const body: unknown = await request.json();
    gateway.logins.push(body);
    const given: unknown =
      typeof body === "object" && body !== null ? Reflect.get(body, "password") : null;
    if (given !== PASSWORD) {
      return apiError(401, "invalid_credentials", "The credentials are not valid.");
    }
    gateway.signedIn = true;
    return ok("post", "/api/auth/login", 200, { user: me.user, csrf_token: gateway.csrfToken });
  });
  override("post", "/api/auth/logout", () => {
    gateway.logouts += 1;
    gateway.signedIn = false;
    return noContent();
  });
  return gateway;
}

/** A gate: what waits for it goes on when it is opened. */
export function gate(): { opened: Promise<void>; open: () => void } {
  let open: () => void = () => undefined;
  const opened = new Promise<void>((resolve) => {
    open = resolve;
  });
  return { opened, open };
}
