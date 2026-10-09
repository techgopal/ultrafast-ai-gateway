import {
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  redirect,
  useRouter,
  useRouterState,
  type RouterHistory,
} from "@tanstack/react-router";
import { useEffect, useMemo } from "react";
import { messageOfError, SOMETHING_WENT_WRONG } from "@/api/errors";
import { can, safePath } from "@/auth/guards";
import { useSession, useSessionControl, useSignOut } from "@/auth/session";
import { FormError } from "@/components/AuthForm";
import { Shell } from "@/components/Shell";
import type { ShellUser } from "@/components/shell-context";
import { Button } from "@/components/ui/button";
import { AcceptInvite } from "@/pages/AcceptInvite";
import { Account } from "@/pages/Account";
import { Alerts } from "@/pages/Alerts";
import { Guardrails } from "@/pages/Guardrails";
import { GuardrailsEdit } from "@/pages/GuardrailsEdit";
import { Keys } from "@/pages/Keys";
import { Limits } from "@/pages/Limits";
import { Logs } from "@/pages/Logs";
import { LogsDetail } from "@/pages/LogsDetail";
import { PageProblem } from "@/pages/NotAvailable";
import { NotFound } from "@/pages/NotFound";
import { Overview } from "@/pages/Overview";
import { Playground } from "@/pages/Playground";
import { Prompts } from "@/pages/Prompts";
import { PromptsEdit } from "@/pages/PromptsEdit";
import { Models } from "@/pages/Models";
import { Routes } from "@/pages/Routes";
import { RoutesEdit } from "@/pages/RoutesEdit";
import { Providers } from "@/pages/Providers";
import { Settings } from "@/pages/Settings";
import { Setup } from "@/pages/Setup";
import { TeamDetail } from "@/pages/TeamDetail";
import { Teams } from "@/pages/Teams";
import { UserDetail } from "@/pages/UserDetail";
import { Users } from "@/pages/Users";
import { SignIn } from "@/pages/SignIn";

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
  interface StaticDataRouteOption {
    /** The page title, shown in the top bar on narrow screens. */
    title?: string;
  }
}

/**
 * Goes to a path of the app in place of the current entry. A path that came
 * from the address was checked with `safePath` before.
 */
function GoTo({ path }: { path: string }) {
  const router = useRouter();
  useEffect(() => {
    router.history.replace(path);
  }, [router, path]);
  return null;
}

/**
 * Sends a visitor who is not signed in to the sign-in page. They come back to
 * where they were, unless they signed out themselves.
 */
function ToSignIn({ comeBack }: { comeBack: boolean }) {
  const router = useRouter();
  useEffect(() => {
    // Read once, here: while the router is on its way, this page is still
    // mounted and the address is already that of the sign-in page.
    const { pathname, href } = router.state.location;
    if (pathname === "/sign-in") return;
    const next = comeBack && href !== "/" ? safePath(href) : null;
    router.history.replace(
      next === null ? "/sign-in" : `/sign-in?next=${encodeURIComponent(next)}`,
    );
  }, [router, comeBack]);
  return null;
}

const INVITE_PAGE = "/accept-invite";

/** The fragment of an invite link: `token=<token>`. */
const FRAGMENT_TOKEN = /^token=(.*)$/;

/**
 * Takes the token of an invite link out of the address and gives it to the
 * session to hold. The gateway puts it in the fragment (`#token=`), which a
 * browser never sends to a server; a link of `?token=`, as links sent
 * before were made, is read as well. This runs above the gate, so the token
 * is gone from the address also while the app loads or cannot reach the
 * gateway. The token is dropped as soon as the address is that of another
 * page.
 */
function useInviteToken() {
  const router = useRouter();
  const { holdInvite, dropInvite } = useSessionControl();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const searchStr = useRouterState({ select: (state) => state.location.searchStr });
  const hash = useRouterState({ select: (state) => state.location.hash });
  useEffect(() => {
    if (pathname !== INVITE_PAGE) {
      dropInvite();
      return;
    }
    const search = new URLSearchParams(searchStr);
    const inSearch = search.get("token");
    const inFragment = FRAGMENT_TOKEN.exec(hash)?.[1] ?? null;
    if (inSearch === null && inFragment === null) return;
    let token = inFragment ?? inSearch ?? "";
    try {
      token = inFragment === null ? token : decodeURIComponent(inFragment);
    } catch {
      token = "";
    }
    if (token !== "") holdInvite(token);
    search.delete("token");
    const rest = search.toString();
    const fragment = inFragment === null && hash !== "" ? `#${hash}` : "";
    // Replaces the entry of the history: with a browser, `history.replaceState`.
    router.history.replace(INVITE_PAGE + (rest === "" ? "" : `?${rest}`) + fragment);
  }, [router, pathname, searchStr, hash, holdInvite, dropInvite]);
}

/**
 * Nothing of the app shows before it is known whether the gateway is set up
 * and who is signed in. While it is not set up, every address is `/setup`.
 */
function Root() {
  const session = useSession();
  const { needsSetup, problem, retry } = useSessionControl();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  useInviteToken();

  if (problem !== null) {
    return (
      <main className="flex min-h-svh flex-col items-center justify-center gap-4 bg-background p-4">
        <h1 className="text-2xl font-semibold">The console could not load</h1>
        <div className="w-full max-w-sm">
          <FormError>{messageOfError(problem) ?? SOMETHING_WENT_WRONG}</FormError>
        </div>
        <Button type="button" className="min-h-11" onClick={retry}>
          Try again
        </Button>
      </main>
    );
  }
  if (session.status === "loading") {
    return (
      <main className="flex min-h-svh items-center justify-center bg-background p-4">
        <p role="status" aria-label="Loading" className="text-sm text-muted-foreground">
          Loading
        </p>
      </main>
    );
  }
  // The address changes at once, the matches of the router follow later. The
  // setup page is rendered here, not through them: a stale match of the shell
  // would send the visitor to the sign-in page, and from there back here.
  if (needsSetup) return pathname === "/setup" ? <Setup /> : <GoTo path="/setup" />;
  return <Outlet />;
}

const rootRoute = createRootRoute({
  component: Root,
  notFoundComponent: NotFound,
});

/** Everything in the shell needs a signed-in user. */
function ShellLayout() {
  const session = useSession();
  const { ending } = useSessionControl();
  const signOut = useSignOut();
  const me = session.status === "signedIn" ? session.me : null;
  const user = useMemo(
    (): ShellUser | null =>
      me === null
        ? null
        : {
            name: me.user.name,
            role: me.user.role,
            teams: me.teams,
            maySetSettings: can(me, { type: "manageSettings" }),
            mayManageAlerts: can(me, { type: "manageAlerts" }),
            mayManageGuardrails: can(me, { type: "manageGuardrails" }),
          },
    [me],
  );

  if (user === null) return <ToSignIn comeBack={ending !== "left"} />;
  return (
    <Shell
      user={user}
      onSignOut={() => {
        void signOut();
      }}
    />
  );
}

const shellRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "shell",
  component: ShellLayout,
});

const overviewRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/",
  staticData: { title: "Overview" },
  component: Overview,
});

// The audit log is a view of the settings page now: the old address leads there.
const auditRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/audit",
  beforeLoad: () => {
    // eslint-disable-next-line @typescript-eslint/only-throw-error -- how the router redirects
    throw redirect({ to: "/settings", hash: "audit", replace: true });
  },
});

const logsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/logs",
  staticData: { title: "Logs" },
  component: Logs,
});

const logRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/logs/$id",
  staticData: { title: "Call" },
  component: function LogRoute() {
    const { id } = logRoute.useParams();
    return <LogsDetail key={id} id={id} />;
  },
});

const alertsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/alerts",
  staticData: { title: "Alerts" },
  component: Alerts,
});

const playgroundRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/playground",
  staticData: { title: "Playground" },
  // "Open in Playground" on a prompt template names it and a version. Only a text and
  // a number are taken; whether they name a template is decided by the page.
  validateSearch: (search): { prompt?: string; version?: number } => ({
    ...(typeof search.prompt === "string" && search.prompt !== ""
      ? { prompt: search.prompt }
      : typeof search.prompt === "number"
        ? { prompt: String(search.prompt) }
        : {}),
    ...(typeof search.version === "number" && Number.isSafeInteger(search.version) && search.version > 0
      ? { version: search.version }
      : {}),
  }),
  component: function PlaygroundRoute() {
    const { prompt, version } = playgroundRoute.useSearch();
    return <Playground {...(prompt === undefined ? {} : { prompt: { name: prompt, version: version ?? null } })} />;
  },
});

const limitsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/limits",
  staticData: { title: "Budgets and limits" },
  component: Limits,
});

const guardrailsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/guardrails",
  staticData: { title: "Guardrails" },
  component: Guardrails,
});

const guardrailNewRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/guardrails/new",
  staticData: { title: "New guardrail" },
  component: function GuardrailNewRoute() {
    return <GuardrailsEdit id={null} />;
  },
});

const guardrailRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/guardrails/$id",
  staticData: { title: "Guardrail" },
  component: function GuardrailRoute() {
    const { id } = guardrailRoute.useParams();
    // Another guardrail is another page: its form and its state do not carry over.
    return <GuardrailsEdit key={id} id={id} />;
  },
});

const promptsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/prompts",
  staticData: { title: "Prompts" },
  component: Prompts,
});

const promptNewRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/prompts/new",
  staticData: { title: "New template" },
  component: function PromptNewRoute() {
    return <PromptsEdit id={null} />;
  },
});

const promptRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/prompts/$id",
  staticData: { title: "Prompt template" },
  component: function PromptRoute() {
    const { id } = promptRoute.useParams();
    // Another template is another page: its form and its state do not carry over.
    return <PromptsEdit key={id} id={id} />;
  },
});

const settingsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/settings",
  staticData: { title: "Settings" },
  component: Settings,
});

const accountRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/account",
  staticData: { title: "Account" },
  component: Account,
});

const providersRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/providers",
  staticData: { title: "Providers" },
  component: Providers,
});

const modelsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/models",
  staticData: { title: "Models" },
  component: Models,
});

const routesRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/routes",
  staticData: { title: "Routing" },
  component: Routes,
});

const routeNewRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/routes/new",
  staticData: { title: "New route" },
  component: function RouteNewRoute() {
    return <RoutesEdit id={null} />;
  },
});

const routeRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/routes/$id",
  staticData: { title: "Route" },
  component: function RouteRoute() {
    const { id } = routeRoute.useParams();
    // Another route is another page: its form and its state do not carry over.
    return <RoutesEdit key={id} id={id} />;
  },
});

const keysRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/keys",
  staticData: { title: "Virtual keys" },
  component: Keys,
});

const usersRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/users",
  staticData: { title: "Users" },
  component: Users,
});

const userRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/users/$id",
  staticData: { title: "User" },
  component: function UserRoute() {
    const { id } = userRoute.useParams();
    // Another user is another page: its dialogs and its state do not carry over.
    return <UserDetail key={id} id={id} />;
  },
});

const teamsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/teams",
  staticData: { title: "Teams" },
  component: Teams,
});

const teamRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/teams/$id",
  staticData: { title: "Team" },
  component: function TeamRoute() {
    const { id } = teamRoute.useParams();
    // Another team is another page: its dialogs and its state do not carry over.
    return <TeamDetail key={id} id={id} />;
  },
});

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sign-in",
  // Only a text is taken; whether it is followed is decided after the sign-in.
  // `sso_error` is the code the gateway sends back after a failed single sign-on.
  validateSearch: (search): { next?: string; sso_error?: string } => ({
    ...(typeof search.next === "string" ? { next: search.next } : {}),
    ...(typeof search.sso_error === "string" ? { sso_error: search.sso_error } : {}),
  }),
  component: function SignInRoute() {
    const session = useSession();
    const { next, sso_error: ssoError } = signInRoute.useSearch();
    if (session.status === "signedIn") return <GoTo path={safePath(next) ?? "/"} />;
    return <SignIn next={next} ssoError={ssoError} />;
  },
});

const setupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/setup",
  component: function SetupRoute() {
    const session = useSession();
    // While setup is needed the root renders the setup page itself, so this
    // is reached only when setup is done: there is nothing to do here.
    return <GoTo path={session.status === "signedIn" ? "/" : "/sign-in"} />;
  },
});

const acceptInviteRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/accept-invite",
  component: AcceptInvite,
});

const routeTree = rootRoute.addChildren([
  signInRoute,
  setupRoute,
  acceptInviteRoute,
  shellRoute.addChildren([
    overviewRoute,
    logsRoute,
    logRoute,
    alertsRoute,
    playgroundRoute,
    providersRoute,
    modelsRoute,
    routesRoute,
    routeNewRoute,
    routeRoute,
    keysRoute,
    usersRoute,
    userRoute,
    teamsRoute,
    teamRoute,
    auditRoute,
    limitsRoute,
    guardrailsRoute,
    guardrailNewRoute,
    guardrailRoute,
    promptsRoute,
    promptNewRoute,
    promptRoute,
    settingsRoute,
    accountRoute,
  ]),
]);

export function createAppRouter(options: { history?: RouterHistory } = {}) {
  return createRouter({
    routeTree,
    // A page whose data call is refused shows "not available", not an error.
    defaultErrorComponent: ({ error }) => <PageProblem error={error} />,
    ...(options.history === undefined ? {} : { history: options.history }),
  });
}
