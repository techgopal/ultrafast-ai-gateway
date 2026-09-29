import type { QueryClient } from "@tanstack/react-query";
import { render, screen, waitFor, type RenderResult } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
  type AnyRouter,
  type RouterHistory,
} from "@tanstack/react-router";
import type { ReactElement } from "react";
import { act } from "react";
import { expect } from "vitest";
import { createQueryClient } from "@/api/queries";
import { AppProviders } from "@/providers";
import { createAppRouter } from "@/router";
import { THEME_STORAGE_KEY, type Theme } from "@/theme/theme";
import { setDevice } from "./device";
import * as fixtures from "./fixtures";
import { apiError, ok, override } from "./handlers";

/** What `/api/auth/me` answers for an admin. */
export const adminUser: fixtures.Me = {
  ...fixtures.me.maya,
  user: { ...fixtures.users.maya, name: "Ada Admin" },
};

/** What `/api/auth/me` answers for a member. */
export const memberUser: fixtures.Me = {
  ...fixtures.me.tomas,
  user: { ...fixtures.users.tomas, name: "Mel Member" },
};

export interface RenderOptions {
  /** The path the app opens at. Default `/`. */
  route?: string;
  /**
   * Who `/api/auth/me` answers with; `null` for nobody (a 401). Default: what
   * the handlers say, which is the admin of the fixtures unless the test
   * changed it.
   */
  user?: fixtures.Me | null;
  /** The saved theme choice. Default: none saved, so the device decides. */
  theme?: Theme;
  /** The screen width in pixels. Below 768 the sidebar is a drawer. Default 1280. */
  width?: number;
  /** What the device prefers. Default `light`. */
  device?: "light" | "dark";
  /** Default: a new client for this render, which does not retry. */
  queryClient?: QueryClient;
  /** Default: a history in memory that starts at `route`. */
  history?: RouterHistory;
}

export interface AppRenderResult extends RenderResult {
  router: AnyRouter;
  queryClient: QueryClient;
}

export const unauthorized = () => apiError(401, "unauthorized", "Sign in to continue.");

/**
 * Renders inside the app's providers and a router with memory history.
 * With `ui`, that element is the whole page at any path. With `null`, the real
 * route tree of the app is rendered at `route`, and the render waits until the
 * app knows whether somebody is signed in.
 */
export async function renderWithApp(
  ui: ReactElement | null,
  options: RenderOptions = {},
): Promise<AppRenderResult> {
  setDevice({
    width: options.width ?? 1280,
    prefersDark: options.device === "dark",
  });
  if (options.theme !== undefined) {
    window.localStorage.setItem(THEME_STORAGE_KEY, options.theme);
  }
  const { user } = options;
  if (user === null) override("get", "/api/auth/me", unauthorized);
  if (user !== null && user !== undefined) {
    override("get", "/api/auth/me", () => ok("get", "/api/auth/me", 200, user));
  }

  const history =
    options.history ?? createMemoryHistory({ initialEntries: [options.route ?? "/"] });
  const router: AnyRouter =
    ui === null
      ? createAppRouter({ history })
      : createRouter({
          history,
          routeTree: createRootRoute({
            component: () => ui,
            notFoundComponent: () => ui,
          }),
        });

  // A client of its own, so that no test sees what another one fetched.
  const queryClient = options.queryClient ?? createQueryClient({ retry: false });

  let result: RenderResult | undefined;
  await act(async () => {
    result = render(
      <AppProviders queryClient={queryClient}>
        <RouterProvider router={router} />
      </AppProviders>,
    );
    await router.load();
  });
  if (result === undefined) throw new Error("render did not run");
  await waitFor(() => {
    expect(screen.queryByRole("status", { name: "Loading" })).toBeNull();
  });
  return Object.assign(result, { router, queryClient });
}
