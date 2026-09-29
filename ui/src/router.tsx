import {
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  useRouter,
  useRouterState,
  type RouterHistory,
} from "@tanstack/react-router";
import { useEffect, useMemo } from "react";
import { can, safePath } from "@/auth/guards";
import { useSession, useSessionControl, useSignOut } from "@/auth/session";
import { FormError } from "@/components/AuthForm";
import { PageHeader } from "@/components/PageHeader";
import { Shell } from "@/components/Shell";
import type { ShellUser } from "@/components/shell-context";
import { Button } from "@/components/ui/button";
import { AcceptInvite } from "@/pages/AcceptInvite";
import { NotAvailable, PageProblem } from "@/pages/NotAvailable";
import { NotFound } from "@/pages/NotFound";
import { Setup } from "@/pages/Setup";
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

/**
 * Nothing of the app shows before it is known whether the gateway is set up
 * and who is signed in. While it is not set up, every address is `/setup`.
 */
function Root() {
  const session = useSession();
  const { needsSetup, problem, retry } = useSessionControl();
  const pathname = useRouterState({ select: (state) => state.location.pathname });

  if (problem !== null) {
    return (
      <main className="flex min-h-svh flex-col items-center justify-center gap-4 bg-background p-4">
        <div className="w-full max-w-sm">
          <FormError>{problem.message}</FormError>
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
  if (needsSetup && pathname !== "/setup") return <GoTo path="/setup" />;
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
      me === null ? null : { name: me.user.name, role: me.user.role, teams: me.teams },
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

/** A page of the shell that a later task replaces with the real one. */
function page<const TPath extends string>(path: TPath, title: string) {
  return createRoute({
    getParentRoute: () => shellRoute,
    path,
    staticData: { title },
    component: () => <PageHeader title={title} />,
  });
}

function AuditPage() {
  const session = useSession();
  if (session.status !== "signedIn" || !can(session.me, { type: "viewAudit" })) {
    return <NotAvailable />;
  }
  return <PageHeader title="Audit log" />;
}

const auditRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/audit",
  staticData: { title: "Audit log" },
  component: AuditPage,
});

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sign-in",
  // Only a text is taken; whether it is followed is decided after the sign-in.
  validateSearch: (search): { next?: string } =>
    typeof search.next === "string" ? { next: search.next } : {},
  component: function SignInRoute() {
    const session = useSession();
    const { next } = signInRoute.useSearch();
    if (session.status === "signedIn") return <GoTo path={safePath(next) ?? "/"} />;
    return <SignIn />;
  },
});

const setupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/setup",
  component: function SetupRoute() {
    const session = useSession();
    if (session.status === "signedIn") return <GoTo path="/" />;
    return <Setup />;
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
    page("/", "Overview"),
    page("/providers", "Providers"),
    page("/keys", "Virtual keys"),
    page("/users", "Users"),
    page("/teams", "Teams"),
    auditRoute,
    page("/account", "Account"),
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
