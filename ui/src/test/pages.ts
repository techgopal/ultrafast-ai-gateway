// What the tests of the pages share: the end of a session on a page, the
// scans for a secret, the counting of calls, and what a dialog with a form
// does while its request runs.
import { onlineManager, type QueryClient, type QueryKey } from "@tanstack/react-query";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { createMemoryHistory, type RouterHistory } from "@tanstack/react-router";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { expect, onTestFinished, vi } from "vitest";
import { api, type Method, type PathFor } from "@/api/client";
import { createQueryClient } from "@/api/queries";
import { errors, type GatewayError } from "./errors";
import { gate } from "./gateway";
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

/**
 * A history, for `renderWithApp`, whose way from the page at `route` to the
 * next is held: what the app pushes is pushed when `open` is called. So a
 * test can let happen, and look at, what falls between the success of a
 * change and the page that follows it. `pushes` counts what the app pushed.
 */
export function aWayThatIsHeld(route: string): {
  history: RouterHistory;
  pushes: () => number;
  open: () => void;
} {
  const door = gate();
  const history = createMemoryHistory({ initialEntries: [route] });
  const push = history.push.bind(history);
  const pushed = vi.spyOn(history, "push").mockImplementation((...to) => {
    void door.opened.then(() => {
      push(...to);
    });
  });
  return {
    history,
    pushes: () => pushed.mock.calls.length,
    open: () => {
      act(() => {
        door.open();
      });
    },
  };
}

/**
 * The clock says this time from now on, until the test ends: `fixtures.now`
 * for the time of the fixtures. Only what reads the clock is changed; timers
 * run as they do. A test whose result depends on the time (a status worked
 * out from an expiry, a day counted from today) calls it, so that it passes
 * on every day and not only until one.
 */
export function theClockIs(at: string): void {
  vi.setSystemTime(new Date(at));
  onTestFinished(() => {
    vi.useRealTimers();
  });
}

/**
 * The window gets the focus, as when the user comes back to the tab: the
 * query library then reads again what is shown and stale. Call it inside a
 * test, never in an `afterEach`: there it would start reads while the app is
 * taken down.
 */
export function theWindowGetsTheFocus(): void {
  act(() => {
    window.dispatchEvent(new Event("visibilitychange"));
  });
}

/**
 * The browser says that there is no network, until the test ends or `back`
 * is called. The query library then holds back what it is told to hold back
 * while offline; what the app sends all the same fails as the test's gateway
 * makes it fail.
 */
export function theBrowserIsOffline(): { back: () => void } {
  const back = () => {
    act(() => {
      onlineManager.setOnline(true);
    });
  };
  act(() => {
    onlineManager.setOnline(false);
  });
  onTestFinished(back);
  return { back };
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

/** An operation whose answer is held: see `held`. */
export interface Held extends Counted {
  /** Lets the answer of every call through, those made so far and those that follow. */
  answer: () => void;
}

/**
 * Counts the calls of the operation and holds their answer, a refusal of the
 * gateway, until `answer` is called: the request of a form runs for as long
 * as the test wants to look at its dialog.
 */
export function held<M extends Method>(
  method: M,
  path: PathFor<M>,
  refusal: GatewayError = errors.forbidden,
): Held {
  const door = gate();
  const count = counted(method, path, async () => {
    await door.opened;
    return refuse(refusal);
  });
  return Object.assign(count, {
    answer: () => {
      act(() => {
        door.open();
      });
    },
  });
}

/** What covers the page behind a dialog: a click on it is a click beside the dialog. */
export function besideTheDialog(): Element {
  const overlay = document.querySelector('[data-slot="dialog-overlay"]');
  if (overlay === null) throw new Error("no overlay");
  return overlay;
}

/**
 * Sends the form of the dialog twice in one tick, as a double press does
 * before anything on the screen could be disabled: nothing of the first
 * submit has been rendered when the second one comes.
 */
export function sendTwiceAtOnce(dialog: HTMLElement): void {
  const form = within(dialog).getByRole("form");
  act(() => {
    fireEvent.submit(form);
    fireEvent.submit(form);
  });
}

/**
 * While the request of the dialog runs: Escape, a click beside the dialog,
 * Cancel and a second submit do nothing, and there is no X. `field` is a
 * field of the form.
 */
export async function expectTheDialogStays(dialog: HTMLElement, field: HTMLElement): Promise<void> {
  const open = () => screen.queryByRole("dialog");

  await userEvent.keyboard("{Escape}");
  expect(open()).toBe(dialog);
  await userEvent.click(besideTheDialog());
  expect(open()).toBe(dialog);
  // There is no button to leave by.
  expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
  expect(within(dialog).queryByRole("button", { name: "Close" })).toBeNull();
  await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(open()).toBe(dialog);
  // Nor is the form sent a second time: not by Enter, pressed twice, not by a
  // submit of the form itself.
  await userEvent.type(field, "{Enter}{Enter}");
  fireEvent.submit(within(dialog).getByRole("form"));
  await settle();
  expect(open()).toBe(dialog);
}

/**
 * After its request failed the dialog can be left again: Cancel can be
 * pressed, and the X is back. The X is pressed, and the dialog is gone.
 */
export async function expectTheDialogCanBeLeft(dialog: HTMLElement): Promise<void> {
  await waitFor(() => {
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
  });
  await userEvent.click(within(dialog).getByRole("button", { name: "Close" }));
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

/**
 * What every dialog with a form does, from the submit to the refusal. The
 * form of `dialog` is filled and can be sent; `request` is its operation,
 * held (`held`). The form is sent twice in one tick. While the request runs
 * the submit button says `running` and is disabled, and the dialog stays
 * (`expectTheDialogStays`). One request was made in all. When the gateway
 * refuses it, the dialog says so and can be left (`expectTheDialogCanBeLeft`).
 */
export async function expectOneRequestWhileTheDialogStays(
  dialog: HTMLElement,
  field: HTMLElement,
  running: string,
  request: Held,
  refusal: GatewayError = errors.forbidden,
): Promise<void> {
  sendTwiceAtOnce(dialog);
  expect(await within(dialog).findByRole("button", { name: running })).toBeDisabled();
  await expectTheDialogStays(dialog, field);
  expect(request.calls).toBe(1);

  request.answer();
  expect(await within(dialog).findByText(refusal.body.error.message)).toBeInTheDocument();
  await settle();
  expect(request.calls).toBe(1);
  await expectTheDialogCanBeLeft(dialog);
  expect(request.calls).toBe(1);
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
 * address of the page. The overview, at `/`, is where a sign-in leads anyway:
 * the app is then at the sign-in page without `next`.
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
  const signIn = page.at === "/" ? "/sign-in" : `/sign-in?next=${encodeURIComponent(page.at)}`;
  await waitFor(() => {
    expect(href(app)).toBe(signIn);
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

/**
 * In `scope`, every `<label for>` is the label of a control that a label can
 * name, and every radio group is named once, by the text that `aria-labelledby`
 * points at: not by a label, which cannot name a group, and not by a second
 * name of its own.
 */
export function expectLabelsNameControls(scope: HTMLElement): void {
  const labels = [...scope.querySelectorAll("label[for]")];
  expect(labels.length).toBeGreaterThan(0);
  for (const label of labels) {
    const target = document.getElementById(label.getAttribute("for") ?? "");
    expect([label.textContent, target?.matches("input, select, textarea, button")]).toEqual([
      label.textContent,
      true,
    ]);
  }
  for (const group of within(scope).queryAllByRole("radiogroup")) {
    expect(group).not.toHaveAttribute("aria-label");
    const name = document.getElementById(group.getAttribute("aria-labelledby") ?? "");
    expect(name).not.toBeNull();
    expect(name?.tagName).not.toBe("LABEL");
    expect(group).toHaveAccessibleName(name?.textContent ?? "no name");
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
