import { render, type RenderResult } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
  type AnyRouter,
} from "@tanstack/react-router";
import type { ReactElement } from "react";
import { act } from "react";
import { AppProviders } from "@/providers";
import { createAppRouter } from "@/router";
import type { ShellUser } from "@/components/shell-context";
import { THEME_STORAGE_KEY, type Theme } from "@/theme/theme";
import { setDevice } from "./device";

export const adminUser: ShellUser = {
  name: "Ada Admin",
  role: "admin",
  teams: [{ name: "Platform" }],
};

export const memberUser: ShellUser = {
  name: "Mel Member",
  role: "member",
  teams: [{ name: "Research" }],
};

export interface RenderOptions {
  /** The path the app opens at. Default `/`. */
  route?: string;
  /** The signed-in user the shell shows. Default: an admin. `null` for nobody. */
  user?: ShellUser | null;
  /** The saved theme choice. Default: none saved, so the device decides. */
  theme?: Theme;
  /** The screen width in pixels. Below 768 the sidebar is a drawer. Default 1280. */
  width?: number;
  /** What the device prefers. Default `light`. */
  device?: "light" | "dark";
  /** Called by the Sign out button. */
  onSignOut?: () => void;
}

export interface AppRenderResult extends RenderResult {
  router: AnyRouter;
}

/**
 * Renders inside the app's providers and a router with memory history.
 * With `ui`, that element is the whole page at any path. With `null`, the real
 * route tree of the app is rendered at `route`.
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

  const history = createMemoryHistory({
    initialEntries: [options.route ?? "/"],
  });
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

  let result: RenderResult | undefined;
  await act(async () => {
    result = render(
      <AppProviders
        user={options.user === undefined ? adminUser : options.user}
        onSignOut={options.onSignOut ?? (() => undefined)}
      >
        <RouterProvider router={router} />
      </AppProviders>,
    );
    await router.load();
  });
  if (result === undefined) throw new Error("render did not run");
  return Object.assign(result, { router });
}
