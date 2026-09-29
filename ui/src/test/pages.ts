// What the tests of the pages share: the end of a session on a page, the
// scans for a secret, and the counting of calls.
import type { QueryClient, QueryKey } from "@tanstack/react-query";
import { act, screen, waitFor } from "@testing-library/react";
import { toast } from "sonner";
import { expect } from "vitest";
import { api, type Method, type PathFor } from "@/api/client";
import { errors } from "./errors";
import { override, refuse, type Call } from "./handlers";
import { unauthenticated, type AppRenderResult } from "./render";

export const SESSION_ENDED = "Your session ended. Sign in again.";
export const NOT_AVAILABLE = "This page is not available to your account.";
export const NOT_FOUND = "Page not found";

/** The address the app is at. */
export function href(app: AppRenderResult): string {
  return app.router.state.location.href;
}

/** The page as text, with what the fields hold. */
export function shown(): string {
  const values = [...document.querySelectorAll("input, textarea")].map((field) =>
    field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement ? field.value : "",
  );
  return document.documentElement.outerHTML + JSON.stringify(values);
}

/** Everything the two caches hold, as one text. */
export function cached(client: QueryClient): string {
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

/** What the browser keeps: both storages, the cookie, the history and the address. */
export function stored(): string {
  return JSON.stringify([
    Object.entries(window.localStorage),
    Object.entries(window.sessionStorage),
    document.cookie,
    window.history.state,
    window.location.href,
  ]);
}

/** The secret is in none of: the document, the caches, the router, the browser. */
export function expectNoSecret(app: AppRenderResult, secret: string): void {
  expect(shown()).not.toContain(secret);
  expect(cached(app.queryClient)).not.toContain(secret);
  expect(JSON.stringify(app.router.state)).not.toContain(secret);
  expect(stored()).not.toContain(secret);
}

export interface Counted {
  calls: number;
  /** The bodies of the calls that had one. */
  bodies: unknown[];
}

/** Answers the operation with `resolver` and counts its calls. */
export function counted<M extends Method>(
  method: M,
  path: PathFor<M>,
  resolver: (call: Call) => Response | Promise<Response>,
): Counted {
  const count: Counted = { calls: 0, bodies: [] };
  override(method, path, async (call) => {
    count.calls += 1;
    if (method !== "get" && method !== "delete") {
      count.bodies.push(await call.request.clone().json().catch(() => undefined));
    }
    return resolver(call);
  });
  return count;
}

/** Lets what is on its way arrive. */
export async function settle(milliseconds = 30): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, milliseconds));
  });
}

/** A call that the gateway answers with 401, as it does when the session ended. */
export async function aCallFindsTheSessionEnded(): Promise<void> {
  override("get", "/api/teams", unauthenticated);
  await act(async () => {
    await api.get("/api/teams").catch(() => undefined);
  });
}

/**
 * Item 8 of the amendment, first case: the page is open, its own query is
 * asked again and answers 401. Asserts that the app is at the sign-in page
 * with `next`, says that the session ended, holds nothing in its caches and
 * asks nothing afterwards.
 *
 * `path` is the operation of the page's query, `queryKey` its key, `at` the
 * address of the page.
 */
export async function expectSessionEndsOnPage(
  app: AppRenderResult,
  page: { path: PathFor<"get">; queryKey: QueryKey; at: string },
): Promise<void> {
  const asked = counted("get", page.path, unauthenticated);
  const me = counted("get", "/api/auth/me", unauthenticated);
  await act(async () => {
    await app.queryClient.invalidateQueries({ queryKey: page.queryKey });
  });
  await waitFor(() => {
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent(page.at)}`);
  });
  expect(asked.calls).toBe(1);
  expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(toasts()).toEqual([]);
  expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
  expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
  await settle();
  expect(asked.calls).toBe(1);
  expect(me.calls).toBe(0);
  expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
}

/** The texts of the toasts that are shown. */
export function toasts(): string[] {
  return [...document.querySelectorAll("[data-sonner-toast]")].map(
    (toast) => toast.querySelector("[data-title]")?.textContent ?? toast.textContent,
  );
}

/**
 * The toast library keeps its toasts beside the app, where the next test
 * would find them. For `afterEach`.
 */
export function forgetToasts(): void {
  toast.dismiss();
}

/**
 * Item 8, second case: answers the operation with 403. The test renders the
 * page afterwards and calls `expectNotAvailable`.
 */
export function forbid(path: PathFor<"get">): Counted {
  return counted("get", path, () => refuse(errors.forbidden));
}

/** The "not available" screen: not an error, and not a toast. */
export async function expectNotAvailable(): Promise<void> {
  expect(await screen.findByText(NOT_AVAILABLE)).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Not available" })).toBeInTheDocument();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByText(errors.forbidden.body.error.message)).toBeNull();
  expect(toasts()).toEqual([]);
}

/** jsdom has no pointer capture, which the toast asks for when it is pressed. */
export function installPointerCapture(): void {
  for (const name of ["setPointerCapture", "releasePointerCapture"]) {
    if (Reflect.has(HTMLElement.prototype, name)) continue;
    Object.defineProperty(HTMLElement.prototype, name, {
      configurable: true,
      writable: true,
      value: () => undefined,
    });
  }
}

/** What describes the control: its error and its hint. */
export function descriptionOf(control: HTMLElement): string {
  return (control.getAttribute("aria-describedby") ?? "")
    .split(" ")
    .filter((id) => id !== "")
    .map((id) => document.getElementById(id)?.textContent ?? `nothing has the id ${id}`)
    .join(" ");
}
