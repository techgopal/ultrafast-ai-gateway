import {
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  type RouterHistory,
} from "@tanstack/react-router";
import { PageHeader } from "@/components/PageHeader";
import { Shell } from "@/components/Shell";
import { useShell } from "@/components/shell-context";
import { NotFound } from "@/pages/NotFound";

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
  interface StaticDataRouteOption {
    /** The page title, shown in the top bar on narrow screens. */
    title?: string;
  }
}

const rootRoute = createRootRoute({
  component: Outlet,
  notFoundComponent: NotFound,
});

function ShellLayout() {
  const { user, signOut } = useShell();
  return <Shell user={user} onSignOut={signOut} />;
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

const routeTree = rootRoute.addChildren([
  shellRoute.addChildren([
    page("/", "Overview"),
    page("/providers", "Providers"),
    page("/keys", "Virtual keys"),
    page("/users", "Users"),
    page("/teams", "Teams"),
    page("/audit", "Audit log"),
    page("/account", "Account"),
  ]),
]);

export function createAppRouter(options: { history?: RouterHistory } = {}) {
  return createRouter({
    routeTree,
    ...(options.history === undefined ? {} : { history: options.history }),
  });
}
