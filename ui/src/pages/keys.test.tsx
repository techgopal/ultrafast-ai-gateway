import type { QueryClient } from "@tanstack/react-query";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, onTestFinished, test, vi } from "vitest";
import { queryKeys } from "@/api/queries";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, noContent, ok, override, refuse } from "@/test/handlers";
import {
  aCallFindsTheSessionEnded,
  cached,
  choose,
  counted,
  descriptionOf,
  expectNoSecret,
  expectNotAvailable,
  expectOneMain,
  expectSessionEndsOnPage,
  forbid,
  forgetToasts,
  href,
  installSelect,
  listenToConsole,
  optionsOf,
  SESSION_ENDED,
  settle,
  shown,
  toasts,
} from "@/test/pages";
import { renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

const { maya, arjun, lena, tomas, priya, sam, dana } = fixtures.users;
const { platform, research } = fixtures.teams;
const { active, suspended, expired, revoked, noOwner } = fixtures.keys;
const SECRET = fixtures.newKeySecret;

const SUSPENDED_HINT = "The owner is not active";
const SHOWN_ONCE = "Copy this key now. It is not shown again.";
const REVOKE = "Apps using this key stop working at once. This cannot be undone.";

// The tests drive dialogs and selects step by step. On a busy machine one can
// take longer than the 5 seconds a test has by default.
vi.setConfig({ testTimeout: 20_000 });

beforeAll(installSelect);
afterEach(forgetToasts);

type Options = { user?: fixtures.Me; width?: number; queryClient?: QueryClient };

function page(options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/keys", ...options });
}

function keysAre(list: readonly fixtures.Key[]) {
  return counted("get", "/api/keys", () => ok("get", "/api/keys", 200, { keys: [...list] }));
}

async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: "Virtual keys" });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

function rowOf(name: string): HTMLElement {
  const row = screen.getByRole("cell", { name }).closest("tr");
  if (row === null) throw new Error(`no row for ${name}`);
  return row;
}

/** The names of the keys that are listed, in their order. */
function listed(): string[] {
  const found = screen.queryByRole("table", { name: "Virtual keys" });
  if (found === null) return [];
  return within(found)
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getAllByRole("cell")[0]?.textContent ?? "");
}

function showRevoked(): HTMLElement {
  return screen.getByRole("checkbox", { name: "Show revoked" });
}

function filter(name: "Team" | "Status"): HTMLElement {
  return screen.getByRole("combobox", { name });
}

/** What the owner is shown as where one is chosen. */
function person(user: { name: string; email: string }): string {
  return `${user.name} (${user.email})`;
}

/** A gateway that keeps the keys and changes them as it is told. */
function keeps(start: readonly fixtures.Key[] = fixtures.keyList) {
  const state = {
    keys: [...start],
    lists: 0,
    created: [] as unknown[],
    revoked: [] as (string | undefined)[],
  };
  override("get", "/api/keys", () => {
    state.lists += 1;
    return ok("get", "/api/keys", 200, { keys: state.keys });
  });
  override("post", "/api/keys", async ({ request }) => {
    const body: unknown = await request.json();
    state.created.push(body);
    const name: unknown = typeof body === "object" && body !== null ? Reflect.get(body, "name") : "";
    const key: fixtures.Key = { ...active, id: 9, name: typeof name === "string" ? name : "" };
    state.keys = [key, ...state.keys];
    return ok("post", "/api/keys", 201, { key, secret: SECRET });
  });
  override("delete", "/api/keys/{id}", ({ params }) => {
    state.revoked.push(params.id);
    state.keys = state.keys.map((key) =>
      String(key.id) === params.id
        ? { ...key, status: "revoked", revoked_at: "2026-09-30 10:00:00" }
        : key,
    );
    return noContent();
  });
  return state;
}

/** Opens the dialog. `chooses`: the viewer chooses the owner, whose choices are waited for. */
async function openCreate(chooses = true): Promise<HTMLElement> {
  await userEvent.click(await screen.findByRole("button", { name: "Create key" }));
  const dialog = await screen.findByRole("dialog", { name: "Create key" });
  if (chooses) await within(dialog).findByRole("combobox", { name: "Owner" });
  return dialog;
}

function field(dialog: HTMLElement, name: "Owner" | "Team"): HTMLElement {
  return within(dialog).getByRole("combobox", { name });
}

async function named(dialog: HTMLElement, name: string): Promise<void> {
  await userEvent.type(within(dialog).getByLabelText("Name"), name);
}

function send(dialog: HTMLElement): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name: "Create key" }));
}

function secretDialog(): Promise<HTMLElement> {
  return screen.findByRole("dialog", { name: "Your new key" });
}

async function closeSecret(): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: "Done" }));
  const question = await screen.findByRole("alertdialog");
  await userEvent.click(within(question).getByRole("button", { name: "Close" }));
  await closed();
}

async function closed(): Promise<void> {
  await waitFor(() => {
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

async function askToRevoke(key: fixtures.Key): Promise<HTMLElement> {
  await userEvent.click(within(rowOf(key.name)).getByRole("button", { name: "Revoke" }));
  return screen.findByRole("alertdialog", { name: `Revoke ${key.name}?` });
}

function confirm(dialog: HTMLElement): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name: "Revoke" }));
}

describe("the list of keys", () => {
  test("keys list and pills", async () => {
    await page();
    const keys = await table();
    expect(screen.getByRole("heading", { level: 1, name: "Virtual keys" })).toBeInTheDocument();
    await userEvent.click(showRevoked());
    expect(within(keys).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Name",
      "Key",
      "Owner",
      "Team",
      "Expires",
      "Status",
      "Actions",
    ]);
    for (const key of fixtures.keyList) {
      const row = rowOf(key.name);
      const display = within(row).getByText(key.display);
      expect(display.className.split(/\s+/)).toContain("font-mono");
      expect(row).toHaveTextContent(key.owner_email ?? "No owner");
      expect(row).toHaveTextContent(key.team_name ?? "No team");
      // The four statuses, each said by its pill.
      const pill = within(row).getByText(key.status);
      expect(pill).toHaveAttribute("data-slot", "badge");
    }
    expect(fixtures.keyList.map((key) => key.status).sort()).toEqual([
      "active",
      "active",
      "expired",
      "revoked",
      "suspended",
    ]);
    // A key with no owner.
    expect(within(rowOf(noOwner.name)).getByText("No owner")).toBeInTheDocument();
    expect(screen.getAllByText("No owner")).toHaveLength(1);
    expect(within(rowOf(suspended.name)).getByText("No team")).toBeInTheDocument();
    // Suspended says why.
    expect(within(rowOf(suspended.name)).getByText(SUSPENDED_HINT)).toBeInTheDocument();
    expect(screen.getAllByText(SUSPENDED_HINT)).toHaveLength(1);
    // The expiry is a time, or "Never".
    expect(within(rowOf(expired.name)).getByText(/2026/).tagName).toBe("TIME");
    expect(within(rowOf(active.name)).getByText("Never")).toBeInTheDocument();
    // The key itself is nowhere: the list has only what the gateway shows of it.
    expect(shown()).not.toContain(SECRET);
    expectOneMain();
  });

  test("revoked keys are hidden by default", async () => {
    await page();
    await table();
    expect(showRevoked()).not.toBeChecked();
    expect(listed()).toEqual([active.name, suspended.name, expired.name, noOwner.name]);
    expect(screen.queryByText(revoked.name)).toBeNull();

    await userEvent.click(showRevoked());
    expect(showRevoked()).toBeChecked();
    expect(listed()).toEqual(fixtures.keyList.map((key) => key.name));
    expect(within(rowOf(revoked.name)).getByText("revoked")).toBeInTheDocument();

    await userEvent.click(showRevoked());
    expect(screen.queryByText(revoked.name)).toBeNull();
  });

  describe("filters", () => {
    async function searchFor(text: string): Promise<void> {
      const search = screen.getByRole("searchbox", { name: "Search" });
      await userEvent.clear(search);
      if (text !== "") await userEvent.type(search, text);
    }

    test("the text narrows by the name, the owner and what is shown of the key", async () => {
      await page();
      await table();
      await searchFor("platform");
      expect(listed()).toEqual([active.name, noOwner.name]);
      await searchFor("TOMAS@");
      expect(listed()).toEqual([expired.name]);
      await searchFor(suspended.display.slice(-4));
      expect(listed()).toEqual([suspended.name]);
      await searchFor("");
      expect(listed()).toHaveLength(4);
    });

    test("the team narrows", async () => {
      await page();
      await table();
      expect(await optionsOf(filter("Team"))).toEqual([
        "All teams",
        platform.name,
        research.name,
        "No team",
      ]);
      await choose(filter("Team"), platform.name);
      expect(listed()).toEqual([active.name, noOwner.name]);
      await choose(filter("Team"), "No team");
      expect(listed()).toEqual([suspended.name]);
      await choose(filter("Team"), "All teams");
      expect(listed()).toHaveLength(4);
    });

    test("the status narrows, and revoked is a choice only while revoked keys are shown", async () => {
      await page();
      await table();
      expect(await optionsOf(filter("Status"))).toEqual([
        "All statuses",
        "active",
        "suspended",
        "expired",
      ]);
      await choose(filter("Status"), "active");
      expect(listed()).toEqual([active.name, noOwner.name]);
      await choose(filter("Status"), "expired");
      expect(listed()).toEqual([expired.name]);

      await choose(filter("Status"), "All statuses");
      await userEvent.click(showRevoked());
      expect(await optionsOf(filter("Status"))).toEqual([
        "All statuses",
        "active",
        "suspended",
        "expired",
        "revoked",
      ]);
      await choose(filter("Status"), "revoked");
      expect(listed()).toEqual([revoked.name]);
      // Hidden again, the choice goes with them.
      await userEvent.click(showRevoked());
      expect(filter("Status")).toHaveTextContent("All statuses");
      expect(listed()).toHaveLength(4);
    });

    test("combined they intersect", async () => {
      await page();
      await table();
      await choose(filter("Status"), "active");
      await choose(filter("Team"), platform.name);
      expect(listed()).toEqual([active.name, noOwner.name]);
      await searchFor("ci");
      expect(listed()).toEqual([noOwner.name]);
      // Nothing is left: the list says so, and the filters stay.
      await searchFor("research");
      expect(listed()).toEqual([]);
      expect(screen.getByRole("heading", { name: "No keys match" })).toBeInTheDocument();
      expect(screen.queryByRole("table")).toBeNull();
      // With the revoked ones of the team.
      await searchFor("");
      await choose(filter("Status"), "All statuses");
      await userEvent.click(showRevoked());
      expect(listed()).toEqual([active.name, revoked.name, noOwner.name]);
    });
  });

  test("a status the console does not know is shown as it is, and can be chosen", async () => {
    // The API description names four; a later gateway may know more.
    keysAre([active, { ...noOwner, status: "paused" }]);
    await page();
    await table();
    const pill = within(rowOf(noOwner.name)).getByText("paused");
    expect(pill).toHaveAttribute("data-slot", "badge");
    expect(pill).toHaveAttribute("data-variant", "outline");
    expect(await optionsOf(filter("Status"))).toEqual([
      "All statuses",
      "active",
      "suspended",
      "expired",
      "paused",
    ]);
    await choose(filter("Status"), "paused");
    expect(listed()).toEqual([noOwner.name]);
  });

  test("the list sorts by name", async () => {
    await page();
    const keys = await table();
    await userEvent.click(within(keys).getByRole("button", { name: "Name" }));
    expect(listed()).toEqual([suspended.name, noOwner.name, active.name, expired.name]);
  });

  test("revoke is offered where the gateway allows it: an admin", async () => {
    await page();
    await table();
    await userEvent.click(showRevoked());
    for (const key of [active, suspended, expired, noOwner]) {
      expect(within(rowOf(key.name)).getByRole("button", { name: "Revoke" })).toBeInTheDocument();
    }
    expect(within(rowOf(revoked.name)).queryByRole("button")).toBeNull();
  });

  test("revoke is offered where the gateway allows it: a lead", async () => {
    // More than the gateway lists for the lead, to see that the console decides too.
    keysAre([active, noOwner, expired, suspended]);
    await page({ user: fixtures.me.arjun });
    await table();
    // Their own key, and a key of the team they lead.
    for (const key of [active, noOwner]) {
      expect(within(rowOf(key.name)).getByRole("button", { name: "Revoke" })).toBeInTheDocument();
    }
    // Not a key of a team they are only a member of, and not the key of another user.
    for (const key of [expired, suspended]) {
      expect(within(rowOf(key.name)).queryByRole("button")).toBeNull();
    }
  });

  test("revoke is offered where the gateway allows it: a member", async () => {
    const own: fixtures.Key = {
      ...active,
      id: 7,
      name: "lena-laptop",
      owner_id: lena.id,
      owner_email: lena.email,
    };
    keysAre([own, active, noOwner]);
    await page({ user: fixtures.me.lena });
    await table();
    expect(within(rowOf(own.name)).getByRole("button", { name: "Revoke" })).toBeInTheDocument();
    for (const key of [active, noOwner]) {
      expect(within(rowOf(key.name)).queryByRole("button")).toBeNull();
    }
  });

  test("a list with nothing to revoke has no column of actions", async () => {
    keysAre([active, noOwner]);
    await page({ user: fixtures.me.lena });
    const keys = await table();
    expect(within(keys).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Name",
      "Key",
      "Owner",
      "Team",
      "Expires",
      "Status",
    ]);
    expect(screen.queryByRole("button", { name: "Revoke" })).toBeNull();
  });

  test("loading shows skeleton rows", async () => {
    const door = gate();
    override("get", "/api/keys", async () => {
      await door.opened;
      return ok("get", "/api/keys", 200, { keys: fixtures.keyList });
    });
    await page();
    const keys = screen.getByRole("table", { name: "Virtual keys" });
    expect(keys).toHaveAttribute("aria-busy", "true");
    expect(keys.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expect(screen.queryByText(active.name)).toBeNull();
    act(() => {
      door.open();
    });
    await table();
    expect(keys.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0);
  });

  test("an empty list says so", async () => {
    keysAre([]);
    await page({ user: fixtures.me.priya });
    expect(await screen.findByRole("heading", { name: "No virtual keys" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.getByRole("button", { name: "Create key" })).toBeInTheDocument();
    // There is nothing to filter.
    expect(screen.queryByRole("group", { name: "Filters" })).toBeNull();
  });

  test("a list of revoked keys only says that they are hidden", async () => {
    keysAre([revoked]);
    await page();
    expect(await screen.findByRole("heading", { name: "No keys match" })).toBeInTheDocument();
    expect(screen.getByText("Change the filters, or turn on Show revoked.")).toBeInTheDocument();
    await userEvent.click(showRevoked());
    expect(listed()).toEqual([revoked.name]);
  });

  test("a failed list call shows the error with Retry, and Retry asks again", async () => {
    const failing = counted("get", "/api/keys", () => refuse(errors.internal_error));
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(screen.queryByRole("table")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(failing.calls).toBe(1);
    expect(screen.queryByRole("button", { name: "Create key" })).toBeNull();
    expect(screen.queryByRole("searchbox")).toBeNull();

    const again = keysAre(fixtures.keyList);
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await table();
    expect(again.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: "Create key" })).toBeInTheDocument();
  });

  test("a gateway that cannot be reached is an error with Retry", async () => {
    override("get", "/api/keys", networkFailure);
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("at width 390 the rows are cards with their labels and actions", async () => {
    await page({ width: 390 });
    const cards = await screen.findByRole("list", { name: "Virtual keys" });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    expect(screen.queryByRole("table")).toBeNull();
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(4);
    const [first] = items;
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Name",
      "Key",
      "Owner",
      "Team",
      "Expires",
      "Status",
    ]);
    expect(first).toHaveTextContent(active.name);
    const revoke = within(first).getByRole("button", { name: "Revoke" });
    expect(revoke.className.split(/\s+/)).toContain("min-h-11");
    // The filters wrap, and are high enough to touch.
    const filters = screen.getByRole("group", { name: "Filters" });
    expect(filters.className.split(/\s+/)).toContain("flex-wrap");
    for (const control of [
      screen.getByRole("searchbox", { name: "Search" }),
      filter("Team"),
      filter("Status"),
      screen.getByRole("button", { name: "Create key" }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
  });

  test("the list call answers 403: not available", async () => {
    forbid("/api/keys");
    await page();
    await expectNotAvailable();
    expectOneMain();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByRole("button", { name: "Create key" })).toBeNull();
  });

  test("the session ends while the list is open", async () => {
    startGateway({ signedIn: true });
    const app = await page();
    await table();
    await expectSessionEndsOnPage(app, {
      path: "/api/keys",
      queryKey: queryKeys.keys.list(),
      at: "/keys",
    });
    expect(shown()).not.toContain(active.name);
  });
});

describe("creating a key", () => {
  test("member creates a key for themselves", async () => {
    const state = keeps([]);
    const users = counted("get", "/api/users", () => refuse(errors.internal_error));
    const teams = counted("get", "/api/teams", () => refuse(errors.internal_error));
    const details = counted("get", "/api/teams/{id}", () => refuse(errors.internal_error));
    await page({ user: fixtures.me.lena });
    const dialog = await openCreate(false);
    // The owner is a text, not a choice.
    expect(within(dialog).queryByRole("combobox", { name: "Owner" })).toBeNull();
    const owner = within(dialog).getByText("Owner");
    expect(owner.tagName).toBe("DT");
    expect(owner.nextElementSibling).toHaveTextContent(lena.name);
    expect(owner.nextElementSibling).toHaveTextContent(lena.email);
    // Their own teams, and none.
    expect(field(dialog, "Team")).toHaveTextContent("No team");
    expect(await optionsOf(field(dialog, "Team"))).toEqual(["No team", platform.name]);

    await named(dialog, "laptop");
    await send(dialog);
    await secretDialog();
    // No `owner_id`: the gateway takes the caller.
    expect(state.created).toEqual([{ name: "laptop" }]);
    // Nothing was asked for to fill the form.
    expect([users.calls, teams.calls, details.calls]).toEqual([0, 0, 0]);
  });

  test("a member creates a key in their team", async () => {
    const state = keeps([]);
    await page({ user: fixtures.me.lena });
    const dialog = await openCreate(false);
    await named(dialog, "laptop");
    await choose(field(dialog, "Team"), platform.name);
    await send(dialog);
    await secretDialog();
    expect(state.created).toEqual([{ name: "laptop", team_id: platform.id }]);
  });

  test("lead can choose a team member", async () => {
    // Dana was added to the team and disabled since.
    const details = counted("get", "/api/teams/{id}", ({ params }) =>
      params.id === String(platform.id)
        ? ok("get", "/api/teams/{id}", 200, {
            team: platform,
            members: [
              ...fixtures.teamDetails.platform.members,
              { user_id: dana.id, email: dana.email, name: dana.name, role: "member" },
            ],
          })
        : refuse(errors.not_found),
    );
    await page({ user: fixtures.me.arjun });
    const dialog = await openCreate();
    // The gateway of the tests lists every user; the console offers the lead
    // themselves and the active members of the team they lead.
    expect(field(dialog, "Owner")).toHaveTextContent(person(arjun));
    expect(await optionsOf(field(dialog, "Owner"))).toEqual([person(arjun), person(lena)]);
    // Only the team they lead was asked for: not Research, where they are a member.
    expect(details.calls).toBe(1);
  });

  test("a lead creates a key for a member, in a team they lead", async () => {
    const state = keeps();
    await page({ user: fixtures.me.arjun });
    const dialog = await openCreate();
    // For themselves: none, or a team they belong to in any role.
    expect(field(dialog, "Team")).toHaveTextContent("No team");
    expect(await optionsOf(field(dialog, "Team"))).toEqual([
      "No team",
      platform.name,
      research.name,
    ]);

    await choose(field(dialog, "Owner"), person(lena));
    // For another user the gateway takes only a team the lead leads: "No team" is not offered.
    expect(field(dialog, "Team")).toHaveTextContent("Choose a team");
    expect(await optionsOf(field(dialog, "Team"))).toEqual([platform.name]);

    await named(dialog, "lena-ci");
    await send(dialog);
    await waitFor(() => {
      expect(descriptionOf(field(dialog, "Team"))).toContain("Choose a team.");
    });
    expect(field(dialog, "Team")).toHaveAttribute("aria-invalid", "true");
    await settle();
    expect(state.created).toEqual([]);

    await choose(field(dialog, "Team"), platform.name);
    await send(dialog);
    await secretDialog();
    expect(state.created).toEqual([{ name: "lena-ci", owner_id: lena.id, team_id: platform.id }]);
  });

  test("team choices follow the owner", async () => {
    const state = keeps();
    await page();
    const dialog = await openCreate();
    // An admin chooses among the active users: not Sam, who is invited, nor Dana, who is disabled.
    expect(field(dialog, "Owner")).toHaveTextContent(person(maya));
    expect(await optionsOf(field(dialog, "Owner"))).toEqual(
      [maya, arjun, lena, priya, tomas].map(person),
    );
    for (const user of [sam, dana]) {
      expect(fixtures.userList).toContain(user);
    }
    // Maya is in no team.
    expect(await optionsOf(field(dialog, "Team"))).toEqual(["No team"]);

    await choose(field(dialog, "Owner"), person(arjun));
    expect(field(dialog, "Team")).toHaveTextContent("No team");
    expect(await optionsOf(field(dialog, "Team"))).toEqual([
      "No team",
      platform.name,
      research.name,
    ]);
    await choose(field(dialog, "Team"), platform.name);
    expect(field(dialog, "Team")).toHaveTextContent(platform.name);

    // Another owner: the team is none again, and the choices are theirs.
    await choose(field(dialog, "Owner"), person(tomas));
    expect(field(dialog, "Team")).toHaveTextContent("No team");
    expect(await optionsOf(field(dialog, "Team"))).toEqual(["No team", research.name]);

    await choose(field(dialog, "Team"), research.name);
    await named(dialog, "tomas-notebook");
    await send(dialog);
    await secretDialog();
    expect(state.created).toEqual([
      { name: "tomas-notebook", owner_id: tomas.id, team_id: research.id },
    ]);
  });

  test("an admin creates a key for another user without a team", async () => {
    const state = keeps();
    await page();
    const dialog = await openCreate();
    await choose(field(dialog, "Owner"), person(priya));
    await named(dialog, "priya-cli");
    await send(dialog);
    await secretDialog();
    expect(state.created).toEqual([{ name: "priya-cli", owner_id: priya.id }]);
  });

  describe("expiry is sent in UTC", () => {
    /** Late in the UTC day: the next day already east of Greenwich, the same day west of it. */
    const NOW = "2026-10-01T23:30:00Z";

    async function created(choice: string, date?: string): Promise<unknown> {
      vi.setSystemTime(new Date(NOW));
      onTestFinished(() => {
        vi.useRealTimers();
      });
      const state = keeps();
      await page({ user: fixtures.me.lena });
      const dialog = await openCreate(false);
      await named(dialog, "laptop");
      const expires = within(dialog).getByRole("radiogroup", { name: "Expires" });
      expect(within(expires).getAllByRole("radio")).toEqual(
        ["Never", "In 30 days", "In 90 days", "On a date"].map((name) =>
          within(expires).getByRole("radio", { name }),
        ),
      );
      expect(within(expires).getByRole("radio", { name: "Never" })).toBeChecked();
      if (choice !== "Never") await userEvent.click(within(expires).getByRole("radio", { name: choice }));
      const day = within(dialog).queryByLabelText("Expiry date");
      if (date === undefined) {
        expect(day).toBeNull();
      } else {
        if (day === null) throw new Error("no field for the date");
        expect(day).toHaveAttribute("type", "date");
        // Today, by the calendar of UTC, is the first day that can be chosen.
        expect(day).toHaveAttribute("min", "2026-10-01");
        fireEvent.change(day, { target: { value: date } });
      }
      await send(dialog);
      await secretDialog();
      expect(state.created).toHaveLength(1);
      return state.created[0];
    }

    test("never sends no expires_at", async () => {
      const body = await created("Never");
      expect(body).toEqual({ name: "laptop" });
      expect(Object.keys(body as object)).not.toContain("expires_at");
    });

    test("a date is sent as the end of that day", async () => {
      expect(await created("On a date", "2027-01-31")).toEqual({
        name: "laptop",
        expires_at: "2027-01-31 23:59:59",
      });
    });

    test("30 days are counted from today in UTC", async () => {
      expect(await created("In 30 days")).toEqual({
        name: "laptop",
        expires_at: "2026-10-31 23:59:59",
      });
    });

    test("90 days are counted from today in UTC", async () => {
      expect(await created("In 90 days")).toEqual({
        name: "laptop",
        expires_at: "2026-12-30 23:59:59",
      });
    });

    test("without a date nothing is sent, and the field says so", async () => {
      const state = keeps();
      await page({ user: fixtures.me.lena });
      const dialog = await openCreate(false);
      await named(dialog, "laptop");
      const expires = within(dialog).getByRole("radiogroup", { name: "Expires" });
      await userEvent.click(within(expires).getByRole("radio", { name: "On a date" }));
      await send(dialog);
      await waitFor(() => {
        expect(descriptionOf(expires)).toContain("Choose a date.");
      });
      expect(within(dialog).getByLabelText("Expiry date")).toHaveAttribute("aria-invalid", "true");
      await settle();
      expect(state.created).toEqual([]);
      expect(toasts()).toEqual([]);
    });
  });

  test("new key is shown once", async () => {
    const written = listenToConsole();
    const state = keeps();
    const app = await page();
    await table();
    expect(state.lists).toBe(1);
    const dialog = await openCreate();
    await named(dialog, "laptop");
    await send(dialog);

    const secret = await secretDialog();
    expect(screen.queryByRole("dialog", { name: "Create key" })).toBeNull();
    expect(secret).toHaveTextContent(SHOWN_ONCE);
    expect(within(secret).getByLabelText("Your new key")).toHaveValue(SECRET);
    // How to use it: the address of this gateway, and the header with a placeholder.
    const example = within(secret).getByRole("group", { name: "How to use the key" });
    expect(example).toHaveTextContent(`${window.location.origin}/v1`);
    expect(example).toHaveTextContent("Authorization: Bearer <key>");
    expect(example.textContent).not.toContain(SECRET);
    expect(example.textContent).not.toContain(SECRET.slice(-8));
    expect(secret.textContent).not.toContain(SECRET);
    // While it is shown, the key is in the state of the page only.
    expect(cached(app.queryClient)).not.toContain(SECRET);
    expect(JSON.stringify(app.router.state)).not.toContain(SECRET);
    expect(href(app)).toBe("/keys");
    expect(toasts()).toEqual([]);

    await closeSecret();
    expectNoSecret(app, SECRET);
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    expect(toasts().join()).not.toContain(SECRET);
    // The list was asked for again, and has the key.
    expect(await screen.findByRole("cell", { name: "laptop" })).toBeInTheDocument();
    expect(state.lists).toBe(2);
    // An admin who chose nobody else is the owner: the gateway takes the caller.
    expect(state.created).toEqual([{ name: "laptop" }]);
    // Nothing was written to the console about it.
    expect(written()).not.toContain(SECRET);
    expect(written()).not.toContain(SECRET.slice(6));
  });

  test("the console listener hears what is written", () => {
    const written = listenToConsole(true);
    console.info("a marker", { in: "an object" });
    console.error("another marker");
    expect(written()).toContain("a marker");
    expect(written()).toContain('{"in":"an object"}');
    expect(written()).toContain("another marker");
  });

  test("field error on team", async () => {
    const posts = counted("post", "/api/keys", () =>
      refuse(validationFailed({ team_id: fieldMessages.teamIdOwner })),
    );
    await page();
    const dialog = await openCreate();
    await named(dialog, "laptop");
    await choose(field(dialog, "Owner"), person(arjun));
    await choose(field(dialog, "Team"), platform.name);
    await send(dialog);
    const team = field(dialog, "Team");
    await waitFor(() => {
      expect(descriptionOf(team)).toBe("owner is not a member of this team");
    });
    expect(fieldMessages.teamIdOwner).toBe("owner is not a member of this team");
    expect(team).toHaveAttribute("aria-invalid", "true");
    expect(within(dialog).getAllByRole("alert")).toHaveLength(1);
    expect(field(dialog, "Owner")).not.toHaveAttribute("aria-invalid");
    expect(dialog).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Name")).toHaveValue("laptop");
    expect(screen.queryByRole("dialog", { name: "Your new key" })).toBeNull();
    expect(toasts()).toEqual([]);

    // Another team: the error goes, and the next attempt is sent.
    await choose(team, research.name);
    expect(team).not.toHaveAttribute("aria-invalid");
    await send(dialog);
    await waitFor(() => {
      expect(posts.calls).toBe(2);
    });
    expect(posts.bodies[1]).toEqual({ name: "laptop", owner_id: arjun.id, team_id: research.id });
  });

  test("the other field errors show on their fields", async () => {
    override("post", "/api/keys", () =>
      refuse(
        validationFailed({
          name: fieldMessages.name,
          owner_id: fieldMessages.ownerId,
          team_id: fieldMessages.teamId,
          expires_at: fieldMessages.expiresAtPast,
        }),
      ),
    );
    await page();
    const dialog = await openCreate();
    await send(dialog);
    const name = within(dialog).getByLabelText("Name");
    await waitFor(() => {
      expect(descriptionOf(name)).toBe(fieldMessages.name);
    });
    expect(name).toHaveFocus();
    expect(descriptionOf(field(dialog, "Owner"))).toBe(fieldMessages.ownerId);
    expect(descriptionOf(field(dialog, "Team"))).toBe(fieldMessages.teamId);
    expect(
      descriptionOf(within(dialog).getByRole("radiogroup", { name: "Expires" })),
    ).toContain(fieldMessages.expiresAtPast);
    expect(within(dialog).getAllByRole("alert")).toHaveLength(4);
  });

  test("a refusal that is about no field shows at the top of the form", async () => {
    override("post", "/api/keys", () => refuse(errors.forbidden));
    await page({ user: fixtures.me.lena });
    const dialog = await openCreate(false);
    await named(dialog, "laptop");
    await send(dialog);
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent(errors.forbidden.body.error.message);
    expect(alert).toHaveFocus();
    expect(screen.queryByRole("heading", { name: "Not available" })).toBeNull();
    expect(toasts()).toEqual([]);
  });

  test("a dialog that is opened again is empty and shows no old error", async () => {
    override("post", "/api/keys", () => refuse(validationFailed({ name: fieldMessages.name })));
    const app = await page();
    await table();
    const opener = screen.getByRole("button", { name: "Create key" });
    const dialog = await openCreate();
    await named(dialog, "x");
    await choose(field(dialog, "Owner"), person(arjun));
    await choose(field(dialog, "Team"), platform.name);
    await userEvent.click(within(dialog).getByRole("radio", { name: "In 30 days" }));
    await send(dialog);
    await within(dialog).findByText(fieldMessages.name);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    // The mutation was reset, and nothing observes it: it is gone.
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    await waitFor(() => {
      expect(opener).toHaveFocus();
    });

    const again = await openCreate();
    expect(within(again).getByLabelText("Name")).toHaveValue("");
    expect(field(again, "Owner")).toHaveTextContent(person(maya));
    expect(field(again, "Team")).toHaveTextContent("No team");
    expect(within(again).getByRole("radio", { name: "Never" })).toBeChecked();
    expect(within(again).queryByRole("alert")).toBeNull();
  });

  test("the choices of the dialog: loading, failed with Retry", async () => {
    const failing = counted("get", "/api/users", () => refuse(errors.internal_error));
    await page();
    await table();
    // The page itself asks for neither the users nor the teams.
    expect(failing.calls).toBe(0);
    const dialog = await openCreate(false);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(within(dialog).queryByRole("combobox")).toBeNull();
    expect(within(dialog).getByRole("button", { name: "Create key" })).toBeDisabled();

    const door = gate();
    override("get", "/api/users", async () => {
      await door.opened;
      return ok("get", "/api/users", 200, { users: fixtures.userList });
    });
    await userEvent.click(within(dialog).getByRole("button", { name: "Retry" }));
    const loading = await within(dialog).findByRole("status", {
      name: "Loading the users and teams",
    });
    expect(loading).toHaveAttribute("aria-busy", "true");
    expect(within(dialog).getByRole("button", { name: "Create key" })).toBeDisabled();
    act(() => {
      door.open();
    });
    expect(await within(dialog).findByRole("combobox", { name: "Owner" })).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Create key" })).toBeEnabled();
    expect(within(dialog).queryByRole("alert")).toBeNull();
  });

  test("the members of a team that cannot be read: failed with Retry", async () => {
    const failing = counted("get", "/api/teams/{id}", ({ params }) =>
      params.id === String(research.id)
        ? refuse(errors.internal_error)
        : ok("get", "/api/teams/{id}", 200, fixtures.teamDetails.platform),
    );
    await page();
    const dialog = await openCreate(false);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(failing.calls).toBe(fixtures.teamList.length);
    const again = counted("get", "/api/teams/{id}", ({ params }) => {
      const detail = fixtures.teamDetailList.find((one) => String(one.team.id) === params.id);
      return detail === undefined
        ? refuse(errors.not_found)
        : ok("get", "/api/teams/{id}", 200, detail);
    });
    await userEvent.click(within(dialog).getByRole("button", { name: "Retry" }));
    expect(await within(dialog).findByRole("combobox", { name: "Owner" })).toBeInTheDocument();
    // Only what failed was asked for again.
    expect(again.calls).toBe(1);
  });

  test("while the key is created the button is disabled and says so", async () => {
    const door = gate();
    const posts = counted("post", "/api/keys", async () => {
      await door.opened;
      return ok("post", "/api/keys", 201, { key: active, secret: SECRET });
    });
    await page({ user: fixtures.me.lena });
    const dialog = await openCreate(false);
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, "laptop");
    await send(dialog);
    const running = await within(dialog).findByRole("button", { name: "Creating the key" });
    expect(running).toBeDisabled();
    await userEvent.type(name, "{Enter}");
    expect(posts.calls).toBe(1);
    act(() => {
      door.open();
    });
    await secretDialog();
    expect(posts.calls).toBe(1);
  });

  test("the session has ended when the key is created: signed out, and the form says nothing", async () => {
    startGateway({ signedIn: true, me: fixtures.me.lena });
    const posts = counted("post", "/api/keys", unauthenticated);
    const app = await page();
    const dialog = await openCreate(false);
    await named(dialog, "laptop");
    await send(dialog);
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/keys")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    await settle();
    expect(posts.calls).toBe(1);
    expect(screen.getByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
    // No error of a field, none of a form, and not the answer of the gateway.
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(errors.unauthenticated.body.error.message)).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
  });

  test("a key whose answer came for a session that is over is not shown", async () => {
    startGateway({ signedIn: true, me: fixtures.me.lena });
    const door = gate();
    override("post", "/api/keys", async () => {
      await door.opened;
      return ok("post", "/api/keys", 201, { key: active, secret: SECRET });
    });
    const app = await page();
    const dialog = await openCreate(false);
    await named(dialog, "laptop");
    await send(dialog);
    await within(dialog).findByRole("button", { name: "Creating the key" });
    await aCallFindsTheSessionEnded("/api/providers");
    door.open();
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expectNoSecret(app, SECRET);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/keys")}`);
  });

  test("the session ends while the new key is shown", async () => {
    startGateway({ signedIn: true, me: fixtures.me.lena });
    const app = await page();
    const dialog = await openCreate(false);
    await named(dialog, "laptop");
    await send(dialog);
    await secretDialog();
    expect(shown()).toContain(SECRET);

    await aCallFindsTheSessionEnded("/api/providers");

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expectNoSecret(app, SECRET);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fkeys");
    });
    expectNoSecret(app, SECRET);
    expect(toasts()).toEqual([]);
  });

  test("at width 390 the controls of the dialog are high enough to touch", async () => {
    await page({ width: 390 });
    const dialog = await openCreate();
    for (const control of [
      within(dialog).getByLabelText("Name"),
      field(dialog, "Owner"),
      field(dialog, "Team"),
      within(dialog).getByRole("button", { name: "Create key" }),
      within(dialog).getByRole("button", { name: "Cancel" }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
    expect(dialog.className).toContain("max-w-[calc(100%-2rem)]");
  });
});

describe("revoking a key", () => {
  test("revoke confirms", async () => {
    const state = keeps();
    await page();
    await table();
    const dialog = await askToRevoke(active);
    expect(dialog).toHaveAccessibleDescription(REVOKE);
    expect(state.revoked).toEqual([]);
    await confirm(dialog);
    await closed();
    expect(state.revoked).toEqual([String(active.id)]);
    // The toast names nothing.
    expect(toasts()).toEqual(["Key revoked."]);

    // The list was asked for again. Revoked keys are hidden, so the key went.
    await waitFor(() => {
      expect(screen.queryByText(active.name)).toBeNull();
    });
    expect(state.lists).toBe(2);
    await userEvent.click(showRevoked());
    const row = rowOf(active.name);
    expect(within(row).getByText("revoked")).toHaveAttribute("data-slot", "badge");
    expect(within(row).queryByText("active")).toBeNull();
    // What is revoked cannot be revoked again.
    expect(within(row).queryByRole("button")).toBeNull();
  });

  test("with the revoked keys shown, the pill of the key changes where it is", async () => {
    keeps();
    await page();
    await table();
    await userEvent.click(showRevoked());
    const dialog = await askToRevoke(noOwner);
    await confirm(dialog);
    await closed();
    await waitFor(() => {
      expect(within(rowOf(noOwner.name)).getByText("revoked")).toBeInTheDocument();
    });
  });

  test("cancel changes nothing", async () => {
    const state = keeps();
    await page();
    await table();
    const dialog = await askToRevoke(active);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    expect(state.revoked).toEqual([]);
    expect(toasts()).toEqual([]);
    expect(within(rowOf(active.name)).getByText("active")).toBeInTheDocument();
  });

  test("a lead revokes a key of their team", async () => {
    const state = keeps([active, noOwner]);
    await page({ user: fixtures.me.arjun });
    await table();
    const dialog = await askToRevoke(noOwner);
    await confirm(dialog);
    await closed();
    expect(state.revoked).toEqual([String(noOwner.id)]);
  });

  test("a refusal stays in the dialog, and a key that is gone is asked for again", async () => {
    const state = keeps();
    const removes = counted("delete", "/api/keys/{id}", () => refuse(errors.not_found));
    await page();
    await table();
    const dialog = await askToRevoke(active);
    await confirm(dialog);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.not_found.body.error.message,
    );
    expect(removes.calls).toBe(1);
    expect(toasts()).toEqual([]);
    // The list is in doubt: it is asked for again.
    await waitFor(() => {
      expect(state.lists).toBe(2);
    });
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
  });

  test("a revocation whose answer came for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("delete", "/api/keys/{id}", async () => {
      await door.opened;
      return noContent();
    });
    const app = await page();
    await table();
    const dialog = await askToRevoke(active);
    await confirm(dialog);
    await aCallFindsTheSessionEnded("/api/providers");
    door.open();
    await settle();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/keys")}`);
  });
});
