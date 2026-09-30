import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, test } from "vitest";
import { queryKeys } from "@/api/queries";
import { errors } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, ok, override, refuse } from "@/test/handlers";
import {
  counted,
  descriptionOf,
  expectNotAvailable,
  expectOneMain,
  expectSessionEndsOnPage,
  forbid,
  forgetToasts,
  settle,
  shown,
  theBrowserIsOffline,
  theWindowGetsTheFocus,
  toasts,
} from "@/test/pages";
import { renderWithApp, type AppRenderResult } from "@/test/render";

const PAGE = 50;
const LOAD_OLDER = "Load older";
const REFRESH = "Refresh";
const LOADED_ONLY = "Filtering the loaded entries only. Load older to look further.";

afterEach(forgetToasts);

type Options = { user?: fixtures.Me; width?: number };

function page(options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/audit", ...options });
}

/**
 * A gateway with this log. It answers as `GET /api/audit` does: the entries
 * with an id below `before`, newest first, at most `limit` of them, 50 when
 * no limit is given. `asked` is the query string of every call.
 */
function logIs(all: readonly fixtures.AuditEntry[]) {
  const state = { entries: [...all], asked: [] as string[] };
  override("get", "/api/audit", ({ request }) => {
    const url = new URL(request.url);
    state.asked.push(url.search);
    const limit = Number(url.searchParams.get("limit") ?? PAGE);
    const before = url.searchParams.get("before");
    const entries = state.entries
      .filter((entry) => before === null || entry.id < Number(before))
      .slice(0, limit);
    return ok("get", "/api/audit", 200, { entries });
  });
  return state;
}

async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: "Audit log" });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

/**
 * The rows of the table, each as the texts of its cells. The table is found
 * by its role and its name; its rows and cells by their elements, not by
 * their roles. A query by role asks of every row and cell it finds whether it
 * is hidden, up through everything around it, and this is called at every
 * look of a `waitFor`, for a table of a hundred rows: on a busy machine a
 * test then ran out of its time.
 */
function rows(): string[][] {
  const found = screen.queryByRole("table", { name: "Audit log" });
  if (found === null) return [];
  return [...found.querySelectorAll("tbody tr")].map((row) =>
    [...row.querySelectorAll("td")].map((cell) => cell.textContent),
  );
}

/** The summaries that are listed, in their order. */
function summaries(): string[] {
  return rows().map((cells) => cells[3] ?? "");
}

/** The times that are listed, in their order, as the API gave them. */
function times(): string[] {
  return [...document.querySelectorAll("table time")].map((time) =>
    (time.getAttribute("title") ?? "").replace(" UTC", ""),
  );
}

function loadOlder(): HTMLElement | null {
  return screen.queryByRole("button", { name: LOAD_OLDER });
}

function filter(): HTMLElement {
  return screen.getByRole("searchbox", { name: "Filter" });
}

function refresh(): HTMLElement | null {
  return screen.queryByRole("button", { name: REFRESH });
}

/** Presses "Load older" and waits for this many rows. */
async function older(count: number): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: LOAD_OLDER }));
  await waitFor(() => {
    expect(rows()).toHaveLength(count);
  });
}

/** Counts how often the app asks who is signed in: it does when it reads again what is stale. */
function whoIsSignedIn() {
  return counted("get", "/api/auth/me", () => ok("get", "/api/auth/me", 200, fixtures.me.maya));
}

describe("the audit log", () => {
  test("audit lists newest first", async () => {
    const log = logIs(fixtures.auditEntries);
    await page();
    const entries = await table();
    expect(screen.getByRole("heading", { level: 1, name: "Audit log" })).toBeInTheDocument();
    expect(within(entries).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Time",
      "Actor",
      "Action",
      "Summary",
    ]);
    // As the API gives them: newest first.
    expect(rows().map(([, actor, action, summary]) => [actor, action, summary])).toEqual(
      fixtures.auditEntries.map((entry) => [entry.actor_email, entry.action, entry.summary]),
    );
    expect(times()).toEqual(fixtures.auditEntries.map((entry) => entry.at));
    expect(times()).toEqual([...times()].sort().reverse());
    // The time is a time, with the exact value of the API beside what is shown.
    const [first] = fixtures.auditEntries;
    if (first === undefined) throw new Error("no entry");
    const time = within(entries).getAllByRole("row")[1]?.querySelector("time");
    expect(time).toHaveAttribute("datetime", `${first.at.replace(" ", "T")}Z`);
    expect(time?.textContent).toMatch(/2026/);
    // The action is shown as the API returns it, in mono.
    for (const entry of fixtures.auditEntries) {
      const action = within(entries).getByText(entry.action);
      expect(action.className.split(/\s+/)).toContain("font-mono");
    }
    // One call: 50 entries, and nothing before.
    expect(log.asked).toEqual([`?limit=${PAGE}`]);
    // No column sorts: the order is the order of the log.
    expect(within(entries).queryByRole("button")).toBeNull();
    expectOneMain();
  });

  test("a summary is shown as the text it is, never as markup", async () => {
    const markup = '<img src=x onerror="alert(1)"> <b>bold</b> &amp; <script>alert(2)</script>';
    const [first] = fixtures.auditEntries;
    if (first === undefined) throw new Error("no entry");
    logIs([{ ...first, summary: markup, action: "<i>user.invite</i>" }]);
    await page();
    const entries = await table();
    expect(within(entries).getByText(markup)).toBeInTheDocument();
    expect(within(entries).getByText("<i>user.invite</i>")).toBeInTheDocument();
    expect(entries.querySelector("img, b, i, script")).toBeNull();
  });

  test("an action the console does not know is shown as it is", async () => {
    const [first] = fixtures.auditEntries;
    if (first === undefined) throw new Error("no entry");
    logIs([{ ...first, action: "budget.exceed", target_type: "budget", target_id: null }]);
    await page();
    const entries = await table();
    expect(within(entries).getByText("budget.exceed").className.split(/\s+/)).toContain("font-mono");
  });

  test("load older uses before", async () => {
    // Three pages: 50, 50 and 20.
    const all = fixtures.auditEntriesFrom(120, 120);
    const log = logIs(all);
    await page();
    await table();
    expect(rows()).toHaveLength(50);
    expect(times()).toEqual(all.slice(0, 50).map((entry) => entry.at));
    expect(log.asked).toEqual([`?limit=${PAGE}`]);

    const lastOfFirst = all[49]?.id;
    expect(lastOfFirst).toBe(71);
    await userEvent.click(screen.getByRole("button", { name: LOAD_OLDER }));
    await waitFor(() => {
      expect(rows()).toHaveLength(100);
    });
    // The second request has `before` equal to the last id of the first page.
    expect(log.asked).toEqual([`?limit=${PAGE}`, `?limit=${PAGE}&before=${lastOfFirst}`]);
    expect(times()).toEqual(all.slice(0, 100).map((entry) => entry.at));
    // A full page: there may be more.
    expect(loadOlder()).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: LOAD_OLDER }));
    await waitFor(() => {
      expect(rows()).toHaveLength(120);
    });
    expect(log.asked[2]).toBe(`?limit=${PAGE}&before=21`);
    expect(log.asked).toHaveLength(3);
    expect(times()).toEqual(all.map((entry) => entry.at));
    // The page had fewer than 50 rows: there is nothing older.
    expect(loadOlder()).toBeNull();
    expect(toasts()).toEqual([]);
  });

  test("a first page with fewer than 50 rows has no button", async () => {
    logIs(fixtures.auditEntries);
    await page();
    await table();
    expect(rows()).toHaveLength(5);
    expect(loadOlder()).toBeNull();
  });

  test("a last page of exactly 50 rows: the button goes when the next page is empty", async () => {
    const log = logIs(fixtures.auditEntriesFrom(50, 50));
    await page();
    await table();
    await userEvent.click(screen.getByRole("button", { name: LOAD_OLDER }));
    await waitFor(() => {
      expect(loadOlder()).toBeNull();
    });
    expect(log.asked).toEqual([`?limit=${PAGE}`, `?limit=${PAGE}&before=1`]);
    expect(rows()).toHaveLength(50);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("while older entries are loaded the button is disabled and says so; one request", async () => {
    const all = fixtures.auditEntriesFrom(80, 80);
    const door = gate();
    const asked: string[] = [];
    override("get", "/api/audit", async ({ request }) => {
      const url = new URL(request.url);
      asked.push(url.search);
      const before = url.searchParams.get("before");
      if (before !== null) await door.opened;
      const entries = all.filter((entry) => before === null || entry.id < Number(before));
      return ok("get", "/api/audit", 200, { entries: entries.slice(0, PAGE) });
    });
    await page();
    await table();
    await userEvent.click(screen.getByRole("button", { name: LOAD_OLDER }));
    const running = await screen.findByRole("button", { name: "Loading older entries" });
    expect(running).toBeDisabled();
    await userEvent.click(running);
    // What is loaded stays while the older entries are on their way.
    expect(rows()).toHaveLength(50);
    expect(screen.getByRole("table", { name: "Audit log" })).toHaveAttribute("aria-busy", "false");
    await settle();
    expect(asked).toHaveLength(2);
    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(rows()).toHaveLength(80);
    });
    expect(asked).toHaveLength(2);
    expect(loadOlder()).toBeNull();
  });

  test("older entries that cannot be loaded: what is loaded stays, and Retry asks for the same", async () => {
    const all = fixtures.auditEntriesFrom(80, 80);
    const asked: string[] = [];
    let failing = true;
    const door = gate();
    override("get", "/api/audit", async ({ request }) => {
      const url = new URL(request.url);
      asked.push(url.search);
      const before = url.searchParams.get("before");
      if (before !== null && failing) return refuse(errors.internal_error);
      if (before !== null) await door.opened;
      const entries = all.filter((entry) => before === null || entry.id < Number(before));
      return ok("get", "/api/audit", 200, { entries: entries.slice(0, PAGE) });
    });
    await page();
    await table();
    await userEvent.click(screen.getByRole("button", { name: LOAD_OLDER }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(rows()).toHaveLength(50);
    expect(toasts()).toEqual([]);
    expect(asked).toEqual([`?limit=${PAGE}`, `?limit=${PAGE}&before=31`]);

    failing = false;
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    // While it is asked for again, the page says that, and no longer what went wrong.
    expect(await screen.findByRole("button", { name: "Loading older entries" })).toBeDisabled();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(rows()).toHaveLength(50);
    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(rows()).toHaveLength(80);
    });
    expect(asked).toEqual([
      `?limit=${PAGE}`,
      `?limit=${PAGE}&before=31`,
      `?limit=${PAGE}&before=31`,
    ]);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(times()).toEqual(all.map((entry) => entry.at));
  });

  test("reading the log again neither repeats nor reorders rows, also when it grew meanwhile", async () => {
    const all = fixtures.auditEntriesFrom(120, 120);
    const log = logIs(all);
    const app = await page();
    await table();
    await userEvent.click(screen.getByRole("button", { name: LOAD_OLDER }));
    await waitFor(() => {
      expect(rows()).toHaveLength(100);
    });

    // Three things happened since: the log has three entries more.
    const grown = fixtures.auditEntriesFrom(123, 123);
    log.entries = grown;
    log.asked.length = 0;
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: queryKeys.audit.all() });
    });
    await waitFor(() => {
      expect(times()[0]).toBe(grown[0]?.at);
    });
    // Each page is asked for from where the page before it ended, as it is now.
    expect(log.asked).toEqual([`?limit=${PAGE}`, `?limit=${PAGE}&before=74`]);
    expect(times()).toEqual(grown.slice(0, 100).map((entry) => entry.at));
    expect(new Set(times()).size).toBe(100);
    expect(times()).toEqual([...times()].sort().reverse());
    expect(loadOlder()).toBeInTheDocument();
  });

  test("the window gets the focus with three pages loaded: the log is not read again", async () => {
    const me = whoIsSignedIn();
    const log = logIs(fixtures.auditEntriesFrom(120, 120));
    await page();
    await table();
    await older(100);
    await older(120);
    expect(log.asked).toHaveLength(3);
    const asked = me.calls;

    theWindowGetsTheFocus();
    // The focus was heard: what is read again at a focus was.
    await waitFor(() => {
      expect(me.calls).toBe(asked + 1);
    });
    await settle();
    expect(log.asked).toHaveLength(3);
    expect(rows()).toHaveLength(120);
  });

  test("the network comes back with three pages loaded: the log is not read again", async () => {
    const me = whoIsSignedIn();
    const log = logIs(fixtures.auditEntriesFrom(120, 120));
    await page();
    await table();
    await older(100);
    await older(120);
    const asked = me.calls;

    theBrowserIsOffline().back();
    await waitFor(() => {
      expect(me.calls).toBe(asked + 1);
    });
    await settle();
    expect(log.asked).toHaveLength(3);
    expect(rows()).toHaveLength(120);
  });

  test("Refresh starts again from the newest page: one request, the older pages are dropped, the filter stays", async () => {
    const log = logIs(fixtures.auditEntriesFrom(120, 120));
    await page();
    await table();
    await older(100);
    await older(120);
    await userEvent.type(filter(), "auth.login");
    expect(rows()).toHaveLength(120);

    // Three things happened since: the log has three entries more.
    const grown = fixtures.auditEntriesFrom(123, 123);
    log.entries = grown;
    log.asked.length = 0;
    await userEvent.click(screen.getByRole("button", { name: REFRESH }));
    await waitFor(() => {
      expect(times()[0]).toBe(grown[0]?.at);
    });
    await table();
    // One request, for the newest page: nothing of what was loaded is asked for again.
    expect(log.asked).toEqual([`?limit=${PAGE}`]);
    expect(rows()).toHaveLength(50);
    expect(times()).toEqual(grown.slice(0, 50).map((entry) => entry.at));
    expect(loadOlder()).toBeInTheDocument();
    expect(filter()).toHaveValue("auth.login");
    expect(toasts()).toEqual([]);
    await settle();
    expect(log.asked).toHaveLength(1);

    // Older entries are loaded from where the newest page ends now.
    await older(100);
    expect(log.asked).toEqual([`?limit=${PAGE}`, `?limit=${PAGE}&before=74`]);
    expect(times()).toEqual(grown.slice(0, 100).map((entry) => entry.at));
  });

  test("while the log is read again the table says so, and Refresh stays where it is", async () => {
    const door = gate();
    let reads = 0;
    override("get", "/api/audit", async () => {
      reads += 1;
      if (reads > 1) await door.opened;
      return ok("get", "/api/audit", 200, { entries: fixtures.auditEntries });
    });
    await page();
    await table();
    const button = screen.getByRole("button", { name: REFRESH });
    await userEvent.click(button);
    await waitFor(() => {
      expect(screen.getByRole("table", { name: "Audit log" })).toHaveAttribute("aria-busy", "true");
    });
    // It is the same button, and it keeps the focus: it is not disabled.
    expect(refresh()).toBe(button);
    expect(button).toBeEnabled();
    expect(button).toHaveFocus();
    expect(filter()).toBeInTheDocument();
    expect(loadOlder()).toBeNull();
    act(() => {
      door.open();
    });
    await table();
    expect(rows()).toHaveLength(5);
    expect(reads).toBe(2);
  });

  test("a refresh that fails shows the error with Retry, and Retry reads the newest page", async () => {
    const log = logIs(fixtures.auditEntriesFrom(80, 80));
    await page();
    await table();
    await older(80);
    override("get", "/api/audit", () => refuse(errors.internal_error));
    await userEvent.click(screen.getByRole("button", { name: REFRESH }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(screen.queryByRole("table")).toBeNull();
    // Retry is the way on: there is no second button that does the same.
    expect(refresh()).toBeNull();
    expect(toasts()).toEqual([]);

    const again = logIs(fixtures.auditEntriesFrom(80, 80));
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await table();
    expect(again.asked).toEqual([`?limit=${PAGE}`]);
    expect(rows()).toHaveLength(50);
    expect(refresh()).toBeInTheDocument();
    expect(log.asked).toHaveLength(2);
  });

  test("Refresh is there for an empty log too, and reads it again", async () => {
    const log = logIs([]);
    await page();
    expect(await screen.findByRole("heading", { name: "No audit entries" })).toBeInTheDocument();
    log.entries = [...fixtures.auditEntries];
    await userEvent.click(screen.getByRole("button", { name: REFRESH }));
    await table();
    expect(rows()).toHaveLength(5);
    expect(log.asked).toEqual([`?limit=${PAGE}`, `?limit=${PAGE}`]);
  });

  test("Refresh is not there when the list call answers 403", async () => {
    forbid("/api/audit");
    await page();
    await expectNotAvailable();
    expect(refresh()).toBeNull();
  });

  test("Refresh is not there for who may not see the log", async () => {
    await page({ user: fixtures.me.lena });
    await expectNotAvailable();
    expect(refresh()).toBeNull();
  });

  test("at width 390 Refresh is high enough to touch", async () => {
    logIs(fixtures.auditEntries);
    await page({ width: 390 });
    await screen.findByRole("list", { name: "Audit log" });
    expect(screen.getByRole("button", { name: REFRESH }).className.split(/\s+/)).toContain(
      "min-h-11",
    );
  });

  test("audit filter", async () => {
    const log = logIs(fixtures.auditEntries);
    await page();
    await table();
    const [invite, revoke, team, update, setup] = fixtures.auditEntries.map((e) => e.summary);
    expect(summaries()).toEqual([invite, revoke, team, update, setup]);

    // The action.
    await userEvent.type(filter(), "key.revoke");
    expect(summaries()).toEqual([revoke]);
    // The actor.
    await userEvent.clear(filter());
    await userEvent.type(filter(), "LENA@");
    expect(summaries()).toEqual([revoke]);
    // The summary, without regard to case.
    await userEvent.clear(filter());
    await userEvent.type(filter(), "growth");
    expect(summaries()).toEqual([team]);
    // Several rows, in the order of the log.
    await userEvent.clear(filter());
    await userEvent.type(filter(), "user.");
    expect(summaries()).toEqual([invite, update]);

    // Nothing matches: the page says so, and the filter stays.
    await userEvent.clear(filter());
    await userEvent.type(filter(), "no such thing");
    expect(await screen.findByRole("heading", { name: "No entries match" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    expect(filter()).toHaveValue("no such thing");

    await userEvent.clear(filter());
    expect(summaries()).toEqual([invite, revoke, team, update, setup]);
    // The filter narrows what is loaded: the gateway was asked once.
    await settle();
    expect(log.asked).toEqual([`?limit=${PAGE}`]);
  });

  test("the filter narrows the loaded rows only, and takes in the older ones when they are loaded", async () => {
    // The oldest entry is the only one of its kind, and on the second page.
    const [setup] = fixtures.auditEntries.slice(-1);
    if (setup === undefined) throw new Error("no entry");
    const all = [...fixtures.auditEntriesFrom(61, 60), setup];
    const log = logIs(all);
    await page();
    await table();
    await userEvent.type(filter(), "setup.create_admin");
    expect(await screen.findByRole("heading", { name: "No entries match" })).toBeInTheDocument();
    // It says where it looked, and older entries can still be loaded.
    expect(screen.getByText(LOADED_ONLY)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: LOAD_OLDER }));
    await waitFor(() => {
      expect(summaries()).toEqual([setup.summary]);
    });
    expect(log.asked).toEqual([`?limit=${PAGE}`, `?limit=${PAGE}&before=12`]);
    expect(loadOlder()).toBeNull();
  });

  test("while older entries may exist the filter says that it looks at the loaded ones only, also when there are matches", async () => {
    logIs(fixtures.auditEntriesFrom(120, 120));
    await page();
    await table();
    // Without a filter there is nothing to say.
    expect(screen.queryByText(LOADED_ONLY)).toBeNull();
    expect(descriptionOf(filter())).toBe("");

    await userEvent.type(filter(), "auth.login");
    // Every loaded entry matches, and the notice is there all the same.
    expect(rows()).toHaveLength(50);
    const notice = screen.getByText(LOADED_ONLY);
    expect(notice).toHaveAttribute("role", "status");
    // It is under the filter, and describes it.
    expect(filter().compareDocumentPosition(notice) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(
      notice.compareDocumentPosition(screen.getByRole("table", { name: "Audit log" })) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(descriptionOf(filter())).toBe(LOADED_ONLY);
    expect(screen.queryByRole("alert")).toBeNull();

    // A second full page: older entries may still exist.
    await older(100);
    expect(screen.getByText(LOADED_ONLY)).toBeInTheDocument();
    // The last page: the end was reached, everything is looked at.
    await older(120);
    expect(loadOlder()).toBeNull();
    expect(screen.queryByText(LOADED_ONLY)).toBeNull();
    expect(descriptionOf(filter())).toBe("");
    expect(filter()).toHaveValue("auth.login");
  });

  test("the notice of the filter goes when the filter is emptied", async () => {
    logIs(fixtures.auditEntriesFrom(120, 120));
    await page();
    await table();
    await userEvent.type(filter(), "a");
    expect(screen.getByText(LOADED_ONLY)).toBeInTheDocument();
    await userEvent.clear(filter());
    expect(screen.queryByText(LOADED_ONLY)).toBeNull();
    // Blanks are no filter: every entry is shown.
    await userEvent.type(filter(), "  ");
    expect(rows()).toHaveLength(50);
    expect(screen.queryByText(LOADED_ONLY)).toBeNull();
  });

  test("a log that is loaded to its end has no notice at its filter", async () => {
    logIs(fixtures.auditEntries);
    await page();
    await table();
    await userEvent.type(filter(), "user.");
    expect(rows()).toHaveLength(2);
    expect(screen.queryByText(LOADED_ONLY)).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
    // Nothing matches: the filter is what to change.
    await userEvent.clear(filter());
    await userEvent.type(filter(), "no such thing");
    expect(await screen.findByRole("heading", { name: "No entries match" })).toBeInTheDocument();
    expect(screen.getByText("Change the filter to see more entries.")).toBeInTheDocument();
    expect(screen.queryByText(LOADED_ONLY)).toBeNull();
  });

  test("nothing matches while older entries may exist: the notice says where it looked, once", async () => {
    logIs(fixtures.auditEntriesFrom(120, 120));
    await page();
    await table();
    await userEvent.type(filter(), "setup.create_admin");
    expect(await screen.findByRole("heading", { name: "No entries match" })).toBeInTheDocument();
    expect(screen.getAllByText(LOADED_ONLY)).toHaveLength(1);
    expect(descriptionOf(filter())).toBe(LOADED_ONLY);
    expect(loadOlder()).toBeInTheDocument();
    // The words it had for this before are gone.
    expect(screen.queryByText(/Only the entries that are loaded/)).toBeNull();
  });

  test("loading shows skeleton rows", async () => {
    const door = gate();
    override("get", "/api/audit", async () => {
      await door.opened;
      return ok("get", "/api/audit", 200, { entries: fixtures.auditEntries });
    });
    await page();
    const entries = screen.getByRole("table", { name: "Audit log" });
    expect(entries).toHaveAttribute("aria-busy", "true");
    expect(entries.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expect(loadOlder()).toBeNull();
    act(() => {
      door.open();
    });
    await table();
    expect(entries.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0);
  });

  test("an empty log says so", async () => {
    logIs([]);
    await page();
    expect(await screen.findByRole("heading", { name: "No audit entries" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    // There is nothing to filter, and nothing older.
    expect(screen.queryByRole("searchbox")).toBeNull();
    expect(loadOlder()).toBeNull();
  });

  test("a failed list call shows the error with Retry, and Retry asks again", async () => {
    const failing = counted("get", "/api/audit", () => refuse(errors.internal_error));
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByRole("searchbox")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(failing.calls).toBe(1);

    const log = logIs(fixtures.auditEntries);
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await table();
    expect(log.asked).toEqual([`?limit=${PAGE}`]);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(rows()).toHaveLength(5);
  });

  test("a gateway that cannot be reached is an error with Retry", async () => {
    override("get", "/api/audit", networkFailure);
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("at width 390 the rows are cards with their labels", async () => {
    logIs(fixtures.auditEntriesFrom(60, 60));
    await page({ width: 390 });
    const cards = await screen.findByRole("list", { name: "Audit log" });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    expect(screen.queryByRole("table")).toBeNull();
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(50);
    const [first] = items;
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Time",
      "Actor",
      "Action",
      "Summary",
    ]);
    expect(first).toHaveTextContent("maya@example.test signed in");
    expect(within(first).getByText("auth.login").className.split(/\s+/)).toContain("font-mono");
    // The controls are high enough to touch, and the filter is as wide as the page.
    for (const control of [filter(), screen.getByRole("button", { name: LOAD_OLDER })]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
    expect(filter().className.split(/\s+/)).toContain("w-full");
  });

  test("the list call answers 403: not available", async () => {
    forbid("/api/audit");
    await page();
    await expectNotAvailable();
    expectOneMain();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByRole("searchbox")).toBeNull();
  });

  test.each([
    ["a lead", fixtures.me.arjun],
    ["a member", fixtures.me.lena],
    ["somebody in no team", fixtures.me.priya],
  ])("for %s the audit log is not available, and is not asked for", async (_, user) => {
    const asked = counted("get", "/api/audit", () => refuse(errors.forbidden));
    await page({ user });
    await expectNotAvailable();
    expectOneMain();
    expect(screen.queryByRole("heading", { name: "Audit log" })).toBeNull();
    await settle();
    expect(asked.calls).toBe(0);
  });

  test("the session ends while the log is open", async () => {
    startGateway({ signedIn: true });
    const app = await page();
    await table();
    await expectSessionEndsOnPage(app, {
      path: "/api/audit",
      queryKey: queryKeys.audit.pages(),
      at: "/audit",
    });
    const [first] = fixtures.auditEntries;
    expect(shown()).not.toContain(first?.summary ?? "no entry");
  });
});
