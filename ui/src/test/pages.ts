// What the tests of the pages share: the end of a session on a page, the
// scans for a secret, and the counting of calls.
import type { QueryClient, QueryKey } from "@tanstack/react-query";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { expect, onTestFinished, vi } from "vitest";
import { api, type Method, type PathFor } from "@/api/client";
import { createQueryClient } from "@/api/queries";
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

/**
 * Records what is written to the console from now on, until the test ends.
 * Returns what was written so far, as one text: a secret must not be in it.
 * It is still written, so that a warning stays visible, unless `quiet`.
 */
export function listenToConsole(quiet = false): () => string {
  const lines: string[] = [];
  for (const method of ["log", "info", "warn", "error", "debug"] as const) {
    const write = console[method].bind(console);
    const spy = vi.spyOn(console, method).mockImplementation((...args: unknown[]) => {
      lines.push(args.map((arg) => (typeof arg === "string" ? arg : JSON.stringify(arg))).join(" "));
      if (!quiet) write(...args);
    });
    onTestFinished(() => {
      spy.mockRestore();
    });
  }
  return () => lines.join("\n");
}

/**
 * A client for `renderWithApp` whose data stays fresh for a minute. A test
 * that leaves a list, changes something and comes back needs it to say
 * anything about the invalidation: with the default, data is stale at once,
 * the list that is mounted again asks again whatever the mutation did, and
 * the test passes without any invalidation. With this client only what a
 * mutation marked as stale is asked for again.
 */
export function clientThatKeepsDataFresh(): QueryClient {
  const client = createQueryClient({ retry: false });
  const defaults = client.getDefaultOptions();
  client.setDefaultOptions({ ...defaults, queries: { ...defaults.queries, staleTime: 60_000 } });
  return client;
}

/** The elements that the selector finds in `scope`, which may be one itself. */
export function inside(scope: Element, selector: string): Element[] {
  return [...(scope.matches(selector) ? [scope] : []), ...scope.querySelectorAll(selector)];
}

/**
 * Watches what comes into the document from now on, until the test ends, also
 * what is there only for a moment. `find` names what it finds of what the
 * test looks for: it is given every element that was added, also one that is
 * gone again by now, and the document as it is after every change.
 */
export function watchTheDocument(find: (scope: Element) => string[]): { seen: () => string[] } {
  const seen = new Set<string>();
  function look(node: Node): void {
    if (!(node instanceof Element)) return;
    for (const name of find(node)) seen.add(name);
  }
  function read(records: MutationRecord[]): void {
    for (const record of records) record.addedNodes.forEach(look);
    look(document.body);
  }
  const observer = new MutationObserver(read);
  observer.observe(document.body, { childList: true, subtree: true, characterData: true });
  onTestFinished(() => {
    observer.disconnect();
  });
  return {
    seen: () => {
      read(observer.takeRecords());
      return [...seen].sort();
    },
  };
}

/**
 * Watches for what must not show on the way from the page of a thing that is
 * gone to its list, also when it shows only for a moment: the skeleton of the
 * page, which `loading` names, and the heading of "not found". Gives the
 * names of what it saw.
 */
export function watchTheWayFrom(loading: string): { seen: () => string[] } {
  return watchTheDocument((scope) => {
    const found: string[] = [];
    if (inside(scope, `[role="status"][aria-label="${loading}"]`).length > 0) found.push(loading);
    if (inside(scope, "h1, h2").some((heading) => heading.textContent === NOT_FOUND)) {
      found.push(NOT_FOUND);
    }
    return found;
  });
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

/**
 * The document has exactly one `main` landmark: a screen that is shown inside
 * the shell brings none of its own.
 */
export function expectOneMain(): void {
  expect(document.querySelectorAll("main, [role=main]")).toHaveLength(1);
  expect(screen.getAllByRole("main")).toHaveLength(1);
}

/**
 * Waits a fixed time, so that what is on its way can arrive. An assertion
 * that something did NOT happen, made after it, proves only that it did not
 * happen within that time: it catches what follows an answer of MSW at once,
 * which is what the pages do, and not what a timer or a slow answer would
 * bring later. Where there is something to wait for, use `waitFor`.
 */
export async function settle(milliseconds = 30): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, milliseconds));
  });
}

/** The lists of the API, which take no parameter. */
export type ListPath = "/api/users" | "/api/teams" | "/api/keys" | "/api/providers" | "/api/tokens";

/**
 * A call that the gateway answers with 401, as it does when the session
 * ended. `path` is a list the page under test does not ask for itself, so
 * that the answers of the page stay what the test set.
 */
export async function aCallFindsTheSessionEnded(path: ListPath): Promise<void> {
  override("get", path, unauthenticated);
  await act(async () => {
    await api.get(path).catch(() => undefined);
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

/**
 * jsdom has no layout: the list of a select asks whether it holds the
 * pointer, and scrolls to its chosen option. For `beforeAll`, with
 * `installPointerCapture`.
 */
export function installSelect(): void {
  installPointerCapture();
  for (const [name, answer] of [
    ["hasPointerCapture", false],
    ["scrollIntoView", undefined],
  ] as const) {
    if (Reflect.has(Element.prototype, name)) continue;
    Object.defineProperty(Element.prototype, name, {
      configurable: true,
      writable: true,
      value: () => answer,
    });
  }
}

async function listOf(select: HTMLElement): Promise<HTMLElement> {
  await userEvent.click(select);
  return screen.findByRole("listbox");
}

async function listClosed(): Promise<void> {
  await waitFor(() => {
    expect(screen.queryByRole("listbox")).toBeNull();
  });
}

/** Opens the select and chooses the option of this name. */
export async function choose(select: HTMLElement, option: string | RegExp): Promise<void> {
  const list = await listOf(select);
  await userEvent.click(within(list).getByRole("option", { name: option }));
  await listClosed();
}

/** The options the select offers, as their texts. It is closed again. */
export async function optionsOf(select: HTMLElement): Promise<string[]> {
  const list = await listOf(select);
  const options = within(list)
    .getAllByRole("option")
    .map((option) => option.textContent);
  await userEvent.keyboard("{Escape}");
  await listClosed();
  return options;
}

/** What describes the control: its error and its hint. */
export function descriptionOf(control: HTMLElement): string {
  return (control.getAttribute("aria-describedby") ?? "")
    .split(" ")
    .filter((id) => id !== "")
    .map((id) => document.getElementById(id)?.textContent ?? `nothing has the id ${id}`)
    .join(" ");
}
