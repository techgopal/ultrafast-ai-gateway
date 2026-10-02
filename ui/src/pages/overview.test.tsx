import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HttpResponse } from "msw";
import { afterEach, describe, expect, onTestFinished, test } from "vitest";
import { queryKeys } from "@/api/queries";
import { errors } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, ok, override, refuse, server } from "@/test/handlers";
import {
  counted,
  expectOneH1,
  expectOneMain,
  expectSessionEndsOnPage,
  forgetToasts,
  href,
  settle,
  shown,
  toasts,
  type Counted,
} from "@/test/pages";
import { renderWithApp, type AppRenderResult } from "@/test/render";

/** The note that stood where the usage is now: it is gone. */
const OLD_NOTE = "Usage, spend and request logs arrive with a later release.";
const NOT_TRACKED =
  "The console cannot tell yet whether a call was made, so this step is never marked done.";
const TILE_NOT_AVAILABLE = "Not available to your account.";
/** What the tiles of users and of teams are called for the lead of a team. */
const LEAD_USERS = "Users in your teams";
const LEAD_TEAMS = "Your teams";

afterEach(forgetToasts);

type Options = { user?: fixtures.Me; width?: number };

function page(options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/", ...options });
}

function main(): HTMLElement {
  return screen.getByRole("main");
}

const USAGE = "Usage, last 30 days";

function usageSection(): HTMLElement {
  return within(main()).getByRole("region", { name: USAGE });
}

/** The page without its usage section: the counts and the steps. */
function withoutUsage(): HTMLElement {
  const copy = main().cloneNode(true) as HTMLElement;
  copy.querySelector(`section[aria-label="${USAGE}"]`)?.remove();
  return copy;
}

function tile(name: string): HTMLElement {
  return within(main()).getByRole("group", { name });
}

function queryTile(name: string): HTMLElement | null {
  return within(main()).queryByRole("group", { name });
}

/** The tile, when its list has been read. */
async function loaded(name: string): Promise<HTMLElement> {
  const found = await within(main()).findByRole("group", { name });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

/** What the tile says: its number, and each line under it. */
function figures(name: string): string[] {
  return [...tile(name).querySelectorAll("p, li")].map((line) => line.textContent.trim());
}

/** The names of the tiles, in their order. */
function tiles(): string[] {
  return within(main())
    .queryAllByRole("group")
    .filter(
      (group) =>
        group.getAttribute("data-slot") === "card" && group.closest(`section[aria-label="${USAGE}"]`) === null,
    )
    .map((group) => within(group).getByRole("heading").textContent);
}

function providersAre(list: readonly fixtures.Provider[]): Counted {
  return counted("get", "/api/providers", () =>
    ok("get", "/api/providers", 200, { providers: [...list] }),
  );
}

function keysAre(list: readonly fixtures.Key[]): Counted {
  return counted("get", "/api/keys", () => ok("get", "/api/keys", 200, { keys: [...list] }));
}

function usersAre(list: readonly fixtures.User[]): Counted {
  return counted("get", "/api/users", () => ok("get", "/api/users", 200, { users: [...list] }));
}

function teamsAre(list: readonly fixtures.Team[]): Counted {
  return counted("get", "/api/teams", () => ok("get", "/api/teams", 200, { teams: [...list] }));
}

/** Every request that is made from now on, until the test ends, as `GET /api/keys`. */
function requests(): string[] {
  const made: string[] = [];
  const listen = ({ request }: { request: Request }) => {
    made.push(`${request.method} ${new URL(request.url).pathname}`);
  };
  server.events.on("request:start", listen);
  onTestFinished(() => {
    server.events.removeListener("request:start", listen);
  });
  return made;
}

function getStarted(): HTMLElement {
  return within(main()).getByRole("region", { name: "Get started" });
}

function queryGetStarted(): HTMLElement | null {
  return within(main()).queryByRole("region", { name: "Get started" });
}

/** The three steps, in their order. */
function steps(): HTMLElement[] {
  return within(within(getStarted()).getByRole("list")).getAllByRole("listitem");
}

function step(index: number): HTMLElement {
  const found = steps()[index];
  if (found === undefined) throw new Error(`no step ${index + 1}`);
  return found;
}

/** The text of the page without its usage section, which is where those words are. */
function pageText(): string {
  const text = withoutUsage().textContent;
  expect(text).not.toContain(OLD_NOTE);
  return text;
}

function expectNoInventedNumbers(): void {
  const text = pageText();
  expect(text).not.toMatch(/\$\s*\d/);
  expect(text).not.toContain("$");
  // The words: "suspended", the status of a key, is another one.
  expect(text).not.toMatch(/\bspend\b/i);
  expect(text).not.toMatch(/\brequests\b/i);
  expect("1 suspended").not.toMatch(/\bspend\b/i);
  expect("no spend yet").toMatch(/\bspend\b/i);
  // No charts.
  expect(withoutUsage().querySelector("canvas, svg[role=img], figure")).toBeNull();
}

describe("the overview", () => {
  test("overview counts", async () => {
    await page();
    expect(screen.getByRole("heading", { level: 1, name: "Overview" })).toBeInTheDocument();
    for (const name of ["Providers", "Virtual keys", "Users", "Teams"]) await loaded(name);
    expect(tiles()).toEqual(["Providers", "Virtual keys", "Users", "Teams"]);

    // Counted from the fixtures.
    const withCredential = fixtures.providerList.filter((one) => one.has_credential).length;
    expect([fixtures.providerList.length, withCredential]).toEqual([2, 1]);
    expect(figures("Providers")).toEqual(["2", "1 with a credential"]);

    const keys = (status: string) => fixtures.keyList.filter((k) => k.status === status).length;
    expect(fixtures.keyList).toHaveLength(5);
    expect(figures("Virtual keys")).toEqual([
      "5",
      `${keys("active")} active`,
      `${keys("suspended")} suspended`,
      `${keys("expired")} expired`,
      `${keys("revoked")} revoked`,
    ]);
    expect(figures("Virtual keys")).toEqual(["5", "2 active", "1 suspended", "1 expired", "1 revoked"]);

    const users = (status: string) => fixtures.userList.filter((u) => u.status === status).length;
    expect(figures("Users")).toEqual([
      String(fixtures.userList.length),
      `${users("active")} active`,
      `${users("invited")} invited`,
      `${users("disabled")} disabled`,
    ]);
    expect(figures("Users")).toEqual(["7", "5 active", "1 invited", "1 disabled"]);

    expect(figures("Teams")).toEqual([String(fixtures.teamList.length)]);
    expect(figures("Teams")).toEqual(["3"]);

    // Each status is said by its pill.
    for (const status of ["active", "suspended", "expired", "revoked"]) {
      expect(within(tile("Virtual keys")).getByText(status)).toHaveAttribute("data-slot", "badge");
    }
    // Each tile links to its page.
    for (const [name, to] of [
      ["Providers", "/providers"],
      ["Virtual keys", "/keys"],
      ["Users", "/users"],
      ["Teams", "/teams"],
    ] as const) {
      expect(within(tile(name)).getByRole("link", { name })).toHaveAttribute("href", to);
      expect(within(tile(name)).getAllByRole("link")).toHaveLength(1);
    }
    expect(within(main()).queryByText(OLD_NOTE)).toBeNull();
    // There is a provider and a key: nothing to get started with.
    expect(queryGetStarted()).toBeNull();
    expect(toasts()).toEqual([]);
    expectOneMain();
    expectOneH1();
  });

  test("the link of a tile leads to its page", async () => {
    const app = await page();
    await loaded("Virtual keys");
    await userEvent.click(within(tile("Virtual keys")).getByRole("link", { name: "Virtual keys" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Virtual keys" })).toBeInTheDocument();
    expect(href(app)).toBe("/keys");
  });

  test("the counts are of what the API returns to the viewer: a lead", async () => {
    // What the gateway lists for the lead of Platform: their team's people and keys.
    const { arjun, lena } = fixtures.users;
    const { platform, research } = fixtures.teams;
    const { active, revoked, noOwner } = fixtures.keys;
    const users = usersAre([arjun, lena]);
    const teams = teamsAre([platform, research]);
    keysAre([active, revoked, noOwner]);
    await page({ user: fixtures.me.arjun });
    for (const name of ["Providers", "Virtual keys", LEAD_USERS, LEAD_TEAMS]) await loaded(name);
    expect(tiles()).toEqual(["Providers", "Virtual keys", LEAD_USERS, LEAD_TEAMS]);
    expect(figures("Virtual keys")).toEqual(["3", "2 active", "1 revoked"]);
    expect(figures(LEAD_USERS)).toEqual(["2", "2 active"]);
    expect(figures(LEAD_TEAMS)).toEqual(["2"]);
    expect([users.calls, teams.calls]).toEqual([1, 1]);
  });

  test("for a lead the tiles say whose the users and the teams are", async () => {
    await page({ user: fixtures.me.arjun });
    for (const name of ["Providers", "Virtual keys", LEAD_USERS, LEAD_TEAMS]) await loaded(name);
    // Not "Users" and "Teams": the numbers are not those of the gateway.
    expect(queryTile("Users")).toBeNull();
    expect(queryTile("Teams")).toBeNull();
    expect([LEAD_USERS, LEAD_TEAMS]).toEqual(["Users in your teams", "Your teams"]);
    // The heading of the tile is its link, to the same pages as for an admin.
    for (const [name, to] of [
      [LEAD_USERS, "/users"],
      [LEAD_TEAMS, "/teams"],
    ] as const) {
      expect(within(tile(name)).getByRole("heading", { level: 2, name })).toBeInTheDocument();
      expect(within(tile(name)).getByRole("link", { name })).toHaveAttribute("href", to);
      expect(within(tile(name)).getAllByRole("link")).toHaveLength(1);
    }
    // The lists that everybody sees whole are called as they are for everybody.
    expect(tiles().slice(0, 2)).toEqual(["Providers", "Virtual keys"]);
    expectOneMain();
    expectOneH1();
  });

  test("an admin who leads a team sees all users and teams, and the tiles are called so", async () => {
    const admin = {
      ...fixtures.me.arjun,
      user: { ...fixtures.users.arjun, role: "admin" },
    } satisfies fixtures.Me;
    await page({ user: admin });
    for (const name of ["Providers", "Virtual keys", "Users", "Teams"]) await loaded(name);
    expect(tiles()).toEqual(["Providers", "Virtual keys", "Users", "Teams"]);
    expect(figures("Users")).toEqual(["7", "5 active", "1 invited", "1 disabled"]);
  });

  test("a lead's list that failed is said in the tile that says whose it is", async () => {
    override("get", "/api/users", () => refuse(errors.internal_error));
    await page({ user: fixtures.me.arjun });
    await loaded(LEAD_TEAMS);
    expect(await within(tile(LEAD_USERS)).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(within(tile(LEAD_USERS)).getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("overview has no invented numbers", async () => {
    await page();
    for (const name of ["Providers", "Virtual keys", "Users", "Teams"]) await loaded(name);
    expectNoInventedNumbers();
    // Every number on the page is one of the counts: there is no digit outside the tiles,
    const outside = withoutUsage();
    for (const card of outside.querySelectorAll('[data-slot="card"]')) card.remove();
    expect(outside.textContent).not.toMatch(/\d/);
    // and in the tiles there are the counts, and nothing else.
    const numbers = tiles().flatMap((name) => figures(name).join(" ").match(/\d+/g) ?? []);
    expect(numbers).toEqual(["2", "1", "5", "2", "1", "1", "1", "7", "5", "1", "1", "3"]);
    for (const name of tiles()) {
      const lines = [...tile(name).querySelectorAll("p, li")].map((line) => line.textContent);
      expect(tile(name).textContent).toBe(name + lines.join(""));
    }
  });

  test("the panel to get started has no invented numbers either", async () => {
    providersAre([]);
    keysAre([]);
    await page();
    await loaded("Providers");
    await loaded("Virtual keys");
    expect(getStarted()).toBeInTheDocument();
    expectNoInventedNumbers();
  });

  test("get started appears without providers", async () => {
    // A gateway that was just set up: no provider, no key.
    providersAre([]);
    keysAre([]);
    const app = await page();
    await loaded("Providers");
    await loaded("Virtual keys");
    const panel = getStarted();
    expect(within(panel).getByRole("heading", { level: 2, name: "Get started" })).toBeInTheDocument();
    expect(steps()).toHaveLength(3);
    expect(panel.querySelector("ol")).not.toBeNull();

    // Each step links to its page, and says whether it is done.
    const [provider, key, call] = [step(0), step(1), step(2)];
    expect(within(provider).getByRole("link", { name: "Add a provider" })).toHaveAttribute(
      "href",
      "/providers",
    );
    expect(within(provider).getByText("To do")).toHaveAttribute("data-slot", "badge");
    expect(within(provider).queryByText("Done")).toBeNull();
    expect(within(key).getByRole("link", { name: "Create a virtual key" })).toHaveAttribute(
      "href",
      "/keys",
    );
    expect(within(key).getByText("To do")).toBeInTheDocument();

    // The third shows a call against this gateway, with placeholders.
    expect(within(call).getByText("Make a first call")).toBeInTheDocument();
    const example = within(call).getByRole("group", { name: "Example call" });
    expect(example.querySelector("pre code")).not.toBeNull();
    expect(example).toHaveTextContent(`curl ${window.location.origin}/v1/chat/completions`);
    expect(example).toHaveTextContent("Authorization: Bearer <key>");
    expect(example).toHaveTextContent('"model": "<provider>/<model>"');
    expect(example.textContent).not.toMatch(/uf-(sk|at)-/);
    // Whether a call was made cannot be known: the step says so, and is never marked.
    expect(within(call).getByText(NOT_TRACKED)).toBeInTheDocument();
    expect(within(call).queryByText("Done")).toBeNull();
    expect(within(call).queryByText("To do")).toBeNull();
    expect(figures("Providers")).toEqual(["0"]);
    expectOneMain();
    expectOneH1();

    // A provider exists now.
    providersAre([fixtures.providers.withCredential]);
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: queryKeys.providers.all() });
    });
    await waitFor(() => {
      expect(within(step(0)).getByText("Done")).toHaveAttribute("data-slot", "badge");
    });
    expect(within(step(0)).queryByText("To do")).toBeNull();
    expect(within(step(0)).getByRole("link", { name: "Add a provider" })).toBeInTheDocument();
    expect(within(step(1)).getByText("To do")).toBeInTheDocument();
    expect(within(step(2)).queryByText("Done")).toBeNull();
    expect(figures("Providers")).toEqual(["1", "1 with a credential"]);

    // And a key: the two steps the console can see are done, and the panel goes.
    keysAre([fixtures.keys.active]);
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: queryKeys.keys.all() });
    });
    await waitFor(() => {
      expect(queryGetStarted()).toBeNull();
    });
  });

  test("without a provider the panel shows, also when there are keys: that step is done", async () => {
    providersAre([]);
    await page();
    await loaded("Providers");
    await loaded("Virtual keys");
    expect(within(step(0)).getByText("To do")).toBeInTheDocument();
    expect(within(step(1)).getByText("Done")).toBeInTheDocument();
    expect(within(step(2)).getByText(NOT_TRACKED)).toBeInTheDocument();
  });

  test("the panel waits for what it depends on, and says nothing of what it does not know", async () => {
    const door = gate();
    override("get", "/api/providers", async () => {
      await door.opened;
      return ok("get", "/api/providers", 200, { providers: [] });
    });
    keysAre([]);
    await page();
    await loaded("Virtual keys");
    // Whether there is a provider is not known yet.
    expect(queryGetStarted()).toBeNull();
    act(() => {
      door.open();
    });
    await loaded("Providers");
    expect(getStarted()).toBeInTheDocument();
  });

  test("a list of providers that failed shows no panel", async () => {
    override("get", "/api/providers", () => refuse(errors.internal_error));
    keysAre([]);
    await page();
    await loaded("Virtual keys");
    await within(tile("Providers")).findByRole("alert");
    expect(queryGetStarted()).toBeNull();
  });

  test("overview for a member", async () => {
    const made = requests();
    // What would answer 403, or is not the member's to count.
    const users = counted("get", "/api/users", () => refuse(errors.forbidden));
    const teams = counted("get", "/api/teams", () => refuse(errors.forbidden));
    const audit = counted("get", "/api/audit", () => refuse(errors.forbidden));
    await page({ user: fixtures.me.lena });
    await loaded("Providers");
    await loaded("Virtual keys");
    // Keys and providers only.
    expect(tiles()).toEqual(["Providers", "Virtual keys"]);
    expect(queryTile("Users")).toBeNull();
    expect(queryTile("Teams")).toBeNull();
    expect(figures("Providers")).toEqual(["2", "1 with a credential"]);
    expect(within(main()).queryByText(OLD_NOTE)).toBeNull();
    await settle();
    // No call that would return 403, and none beyond the two lists.
    expect([users.calls, teams.calls, audit.calls]).toEqual([0, 0, 0]);
    expect([...made].sort()).toEqual([
      "GET /api/auth/me",
      "GET /api/keys",
      "GET /api/providers",
      "GET /api/setup",
      // The usage by day and by model: one request each, however many tiles read them.
      "GET /api/usage",
      "GET /api/usage",
    ]);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(errors.forbidden.body.error.message)).toBeNull();
    expect(toasts()).toEqual([]);
    expectOneMain();
    expectOneH1();
  });

  test("somebody in no team sees keys and providers only", async () => {
    const made = requests();
    await page({ user: fixtures.me.priya });
    await loaded("Providers");
    await loaded("Virtual keys");
    expect(tiles()).toEqual(["Providers", "Virtual keys"]);
    await settle();
    expect(made).not.toContain("GET /api/users");
    expect(made).not.toContain("GET /api/teams");
  });

  test.each([
    ["an admin", fixtures.me.maya, ["Users", "Teams"]],
    ["a lead", fixtures.me.arjun, [LEAD_USERS, LEAD_TEAMS]],
  ])("%s makes the four list calls and the three usage calls, once each, and no other", async (_, user, theirs) => {
    const made = requests();
    await page({ user });
    for (const name of ["Providers", "Virtual keys", ...theirs]) await loaded(name);
    await settle();
    expect([...made].sort()).toEqual([
      "GET /api/auth/me",
      "GET /api/keys",
      "GET /api/providers",
      "GET /api/setup",
      "GET /api/teams",
      // By day, by model, and by key.
      "GET /api/usage",
      "GET /api/usage",
      "GET /api/usage",
      "GET /api/users",
    ]);
  });

  test("who cannot add a provider is told who can", async () => {
    providersAre([]);
    keysAre([]);
    await page({ user: fixtures.me.lena });
    await loaded("Providers");
    await loaded("Virtual keys");
    expect(within(step(0)).getByText("Only an admin can add a provider.")).toBeInTheDocument();
    expect(within(step(0)).getByRole("link", { name: "Add a provider" })).toHaveAttribute(
      "href",
      "/providers",
    );
  });

  test("an admin is not told that", async () => {
    providersAre([]);
    keysAre([]);
    await page();
    await loaded("Providers");
    await loaded("Virtual keys");
    expect(within(getStarted()).queryByText("Only an admin can add a provider.")).toBeNull();
  });

  test("loading shows a skeleton in each tile", async () => {
    const door = gate();
    for (const path of ["/api/providers", "/api/keys", "/api/users", "/api/teams"] as const) {
      const answer = {
        "/api/providers": () => ok("get", "/api/providers", 200, { providers: fixtures.providerList }),
        "/api/keys": () => ok("get", "/api/keys", 200, { keys: fixtures.keyList }),
        "/api/users": () => ok("get", "/api/users", 200, { users: fixtures.userList }),
        "/api/teams": () => ok("get", "/api/teams", 200, { teams: fixtures.teamList }),
      }[path];
      override("get", path, async () => {
        await door.opened;
        return answer();
      });
    }
    await page();
    expect(tiles()).toEqual(["Providers", "Virtual keys", "Users", "Teams"]);
    for (const name of tiles()) {
      expect(tile(name)).toHaveAttribute("aria-busy", "true");
      expect(tile(name).querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
      expect(figures(name)).toEqual([]);
      // The link is there from the start.
      expect(within(tile(name)).getByRole("link", { name })).toBeInTheDocument();
    }
    expect(queryGetStarted()).toBeNull();
    act(() => {
      door.open();
    });
    for (const name of ["Providers", "Virtual keys", "Users", "Teams"]) {
      const found = await loaded(name);
      expect(found.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0);
    }
  });

  test("a failed list shows its tile in an error state with Retry, and the rest of the page stays", async () => {
    const failing = counted("get", "/api/keys", () => refuse(errors.internal_error));
    await page();
    for (const name of ["Providers", "Users", "Teams"]) await loaded(name);
    const keys = tile("Virtual keys");
    expect(await within(keys).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(failing.calls).toBe(1);
    expect(toasts()).toEqual([]);
    // The other tiles, the link of this one and the note are there.
    expect(figures("Providers")).toEqual(["2", "1 with a credential"]);
    expect(figures("Users")).toEqual(["7", "5 active", "1 invited", "1 disabled"]);
    expect(figures("Teams")).toEqual(["3"]);
    expect(within(keys).getByRole("link", { name: "Virtual keys" })).toHaveAttribute("href", "/keys");
    expect(within(main()).queryByText(OLD_NOTE)).toBeNull();
    expect(screen.getByRole("heading", { level: 1, name: "Overview" })).toBeInTheDocument();
    // Whether there is a key is not known: nothing is said about getting started.
    expect(queryGetStarted()).toBeNull();
    expectOneMain();
    expectOneH1();

    const again = keysAre(fixtures.keyList);
    const others = [providersAre(fixtures.providerList), usersAre(fixtures.userList)];
    await userEvent.click(within(keys).getByRole("button", { name: "Retry" }));
    await waitFor(() => {
      expect(figures("Virtual keys")).toEqual(["5", "2 active", "1 suspended", "1 expired", "1 revoked"]);
    });
    expect(again.calls).toBe(1);
    // Only the list that failed was asked for again.
    expect(others.map((other) => other.calls)).toEqual([0, 0]);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("a gateway that cannot be reached is an error with Retry in each tile", async () => {
    for (const path of ["/api/providers", "/api/keys", "/api/users", "/api/teams"] as const) {
      override("get", path, networkFailure);
    }
    await page();
    await waitFor(() => {
      expect(screen.getAllByRole("alert")).toHaveLength(4);
    });
    for (const name of tiles()) {
      expect(within(tile(name)).getByRole("alert")).toHaveTextContent("Could not reach the gateway.");
      expect(within(tile(name)).getByRole("button", { name: "Retry" })).toBeInTheDocument();
    }
    expect(screen.getByRole("heading", { level: 1, name: "Overview" })).toBeInTheDocument();
    expect(within(main()).queryByText(OLD_NOTE)).toBeNull();
    expectOneMain();
    expectOneH1();
  });

  test("a list call answers 403: its tile says that it is not available, not an error and not a toast", async () => {
    const refused = counted("get", "/api/users", () => refuse(errors.forbidden));
    await page();
    for (const name of ["Providers", "Virtual keys", "Teams"]) await loaded(name);
    const users = tile("Users");
    expect(await within(users).findByText(TILE_NOT_AVAILABLE)).toBeInTheDocument();
    expect(users).toHaveAttribute("aria-busy", "false");
    expect(refused.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(within(users).queryByRole("button", { name: "Retry" })).toBeNull();
    expect(screen.queryByText(errors.forbidden.body.error.message)).toBeNull();
    expect(toasts()).toEqual([]);
    // The rest of the page is as it is for everybody.
    expect(figures("Teams")).toEqual(["3"]);
    expect(screen.getByRole("heading", { level: 1, name: "Overview" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Not available" })).toBeNull();
    expectOneMain();
    expectOneH1();
  });

  test("a status the console does not know is counted under its own text, in a neutral pill", async () => {
    const { active, revoked } = fixtures.keys;
    keysAre([active, { ...revoked, id: 8, status: "frozen" }, { ...revoked, id: 9, status: "frozen" }]);
    override("get", "/api/users", () =>
      // What a later gateway may answer: the description of the API has no such status.
      HttpResponse.json({ users: [fixtures.users.maya, { ...fixtures.users.sam, status: "locked" }] }),
    );
    await page();
    await loaded("Virtual keys");
    await loaded("Users");
    expect(figures("Virtual keys")).toEqual(["3", "1 active", "2 frozen"]);
    expect(figures("Users")).toEqual(["2", "1 active", "1 locked"]);
    for (const [name, status] of [
      ["Virtual keys", "frozen"],
      ["Users", "locked"],
    ] as const) {
      const pill = within(tile(name)).getByText(status);
      expect(pill).toHaveAttribute("data-slot", "badge");
      expect(pill).toHaveAttribute("data-variant", "outline");
    }
  });

  test("lists with nothing in them count zero", async () => {
    providersAre([]);
    keysAre([]);
    teamsAre([]);
    await page();
    for (const name of ["Providers", "Virtual keys", "Users", "Teams"]) await loaded(name);
    expect(figures("Providers")).toEqual(["0"]);
    expect(figures("Virtual keys")).toEqual(["0"]);
    expect(figures("Teams")).toEqual(["0"]);
    // Nothing by status where there is nothing.
    expect(within(tile("Virtual keys")).queryByRole("list")).toBeNull();
  });

  test("at width 390 the tiles stack, the links are high enough to touch and the example wraps", async () => {
    providersAre([]);
    keysAre([]);
    await page({ width: 390 });
    for (const name of ["Providers", "Virtual keys", "Users", "Teams"]) await loaded(name);
    // One column, and more only from a wider screen on.
    const grid = tile("Providers").parentElement;
    const classes = grid?.className.split(/\s+/) ?? [];
    expect(classes).toContain("grid");
    expect(classes).toContain("grid-cols-1");
    expect(classes.filter((name) => /^grid-cols-/.test(name))).toEqual(["grid-cols-1"]);
    expect(classes.some((name) => /^sm:grid-cols-/.test(name))).toBe(true);
    for (const name of tiles()) {
      const link = within(tile(name)).getByRole("link", { name });
      expect(link.className.split(/\s+/)).toContain("min-h-11");
    }
    for (const name of ["Add a provider", "Create a virtual key"]) {
      const link = within(getStarted()).getByRole("link", { name });
      expect(link.className.split(/\s+/)).toContain("min-h-11");
    }
    // The example is as wide as its step at most: long lines wrap.
    const example = within(getStarted()).getByRole("group", { name: "Example call" });
    const pre = example.querySelector("pre");
    expect(pre?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["whitespace-pre-wrap", "wrap-anywhere", "min-w-0"]),
    );
    expectOneMain();
    expectOneH1();
  });

  test("the session ends while the overview is open", async () => {
    startGateway({ signedIn: true });
    const app = await page();
    for (const name of ["Providers", "Virtual keys", "Users", "Teams"]) await loaded(name);
    await expectSessionEndsOnPage(app, {
      path: "/api/providers",
      queryKey: queryKeys.providers.list(),
      at: "/",
    });
    expect(href(app)).toBe("/sign-in");
    expect(shown()).not.toContain(OLD_NOTE);
  });
});

describe("usage on the overview", () => {
  const UNPRICED = "Some models have no price; spend is a lower bound.";

  function usageTile(name: string): HTMLElement {
    return within(usageSection()).getByRole("group", { name });
  }

  async function usageLoaded(name: string): Promise<HTMLElement> {
    const found = await within(await screen.findByRole("region", { name: USAGE })).findByRole(
      "group",
      { name },
    );
    await waitFor(() => {
      expect(found).toHaveAttribute("aria-busy", "false");
    });
    return found;
  }

  function lines(name: string): string[] {
    return [...usageTile(name).querySelectorAll("p")].map((line) => line.textContent.trim());
  }

  function tableRows(name: string): string[][] {
    return [...usageTile(name).querySelectorAll("tbody tr")].map((row) =>
      [...row.querySelectorAll("td")].map((cell) => cell.textContent.trim()),
    );
  }

  test("tiles for the last 30 days, each with a sparkline of its days", async () => {
    await page();
    for (const name of ["Requests", "Errors", "Tokens", "Spend"]) await usageLoaded(name);
    expect(lines("Requests")).toEqual(["150"]);
    expect(lines("Errors")).toEqual(["6", "4% of requests"]);
    expect(lines("Tokens")).toEqual(["15,000 in", "6,000 out"]);
    expect(lines("Spend").slice(0, 1)).toEqual(["$12.34"]);
    // Thirty days; a day with no row is a day with nothing; the name says lowest and highest.
    const names = ["Requests", "Errors", "Tokens", "Spend"].map((name) => {
      const image = within(usageTile(name)).getByRole("img");
      expect(image.querySelector("title")).not.toBeNull();
      return image.getAttribute("aria-label");
    });
    expect(names).toEqual([
      "Requests per day, 30 days, lowest 0, highest 70",
      "Errors per day, 30 days, lowest 0, highest 5",
      "Tokens per day, 30 days, lowest 0, highest 10,000",
      "Spend per day, 30 days, lowest $0.00, highest $6.00",
    ]);
    expect(main().textContent).not.toContain(OLD_NOTE);
    expect(within(usageTile("Requests")).getByRole("link", { name: "Requests" })).toHaveAttribute(
      "href",
      "/logs",
    );
    expectOneMain();
    expectOneH1("Overview");
  });

  test("an unpriced call says that spend is a lower bound", async () => {
    await page();
    await usageLoaded("Spend");
    expect(usageTile("Spend")).toHaveTextContent(UNPRICED);
  });

  test("with every call priced there is no such note", async () => {
    const day = fixtures.usageByDay;
    override("get", "/api/usage", () =>
      ok("get", "/api/usage", 200, {
        ...day,
        total: { ...day.total, unpriced_requests: 0 },
        rows: day.rows.map((row) => ({ ...row, unpriced_requests: 0 })),
      }),
    );
    await page();
    await usageLoaded("Spend");
    expect(main().textContent).not.toContain(UNPRICED);
  });

  test("a gateway with no calls shows zeros, a flat line and no rate", async () => {
    const empty = { ...fixtures.usageByDay, rows: [], total: { ...fixtures.usageByDay.total, requests: 0, errors: 0, input_tokens: 0, output_tokens: 0, cost_micros: 0, unpriced_requests: 0 } };
    override("get", "/api/usage", () => ok("get", "/api/usage", 200, empty));
    await page();
    await usageLoaded("Spend");
    expect(lines("Requests")).toEqual(["0"]);
    expect(lines("Errors")).toEqual(["0", "0% of requests"]);
    expect(lines("Spend")).toEqual(["$0.00"]);
    expect(usageTile("Top models")).toHaveTextContent("No calls yet");
  });

  test("top models: five, most requests first, with their spend", async () => {
    const asked = counted("get", "/api/usage", ({ request }) =>
      ok("get", "/api/usage", 200, fixtures.usageOf(new URL(request.url).searchParams.get("group") ?? "day")),
    );
    await page();
    await usageLoaded("Top models");
    expect(tableRows("Top models")).toEqual([
      ["openai/gpt-4o", "60", "$8.00"],
      ["anthropic/claude-haiku", "40", "$3.00"],
      ["chat-fast", "20", "$0.00"],
      ["ollama/llama3", "15", "$1.34"],
      ["openai/gpt-4o-mini", "10", "<$0.01"],
    ]);
    expect(asked.calls).toBeGreaterThanOrEqual(3);
  });

  test("top keys for an admin and for a lead, not for a member", async () => {
    for (const user of [fixtures.me.maya, fixtures.me.arjun]) {
      const app = await page({ user });
      await usageLoaded("Top keys");
      expect(tableRows("Top keys")).toEqual([
        ["platform-prod", "100", "$9.00"],
        ["(none)", "30", "$3.00"],
        ["(deleted)", "20", "$0.34"],
      ]);
      app.unmount();
    }
    const groups: (string | null)[] = [];
    override("get", "/api/usage", ({ request }) => {
      const group = new URL(request.url).searchParams.get("group");
      groups.push(group);
      return ok("get", "/api/usage", 200, fixtures.usageOf(group ?? "day"));
    });
    await page({ user: fixtures.me.tomas });
    await usageLoaded("Spend");
    await usageLoaded("Top models");
    await settle();
    expect(within(usageSection()).queryByRole("group", { name: "Top keys" })).toBeNull();
    expect(groups.sort()).toEqual(["day", "model"]);
  });

  test("a failed usage call is said in its tiles with Retry; the counts stay", async () => {
    override("get", "/api/usage", () => refuse(errors.internal_error));
    await page();
    await loaded("Providers");
    const tile = await within(await screen.findByRole("region", { name: USAGE })).findByRole("group", { name: "Requests" });
    expect(await within(tile).findByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    expect(within(tile).getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expect(figures("Providers")).toEqual(["2", "1 with a credential"]);
    expectOneMain();
    expectOneH1("Overview");
  });

  test("at width 390 the tiles fit one column", async () => {
    await page({ width: 390 });
    await usageLoaded("Requests");
    expect(usageSection().querySelector(".grid")?.className).toContain("grid-cols-1");
    for (const link of within(usageSection()).getAllByRole("link")) {
      expect(link.className).toContain("min-h-11");
    }
  });
});
