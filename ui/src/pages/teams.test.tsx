import type { QueryClient } from "@tanstack/react-query";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HttpResponse } from "msw";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { queryKeys } from "@/api/queries";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, noContent, ok, override, refuse } from "@/test/handlers";
import {
  aCallFindsTheSessionEnded,
  clientThatKeepsDataFresh,
  counted,
  descriptionOf,
  expectNotAvailable,
  expectOneMain,
  expectSessionEndsOnPage,
  forbid,
  forgetToasts,
  href,
  installPointerCapture,
  NOT_FOUND,
  settle,
  shown,
  toasts,
} from "@/test/pages";
import { renderWithApp, type AppRenderResult } from "@/test/render";

const { maya, arjun, lena, tomas, priya, sam, dana } = fixtures.users;
const { platform, research, growth } = fixtures.teams;

const DELETE = "Keys that belong to this team keep working and lose their team.";
const LEAVE = "You will lose access to this team.";
const NO_USER = "No user with that ID.";
const ID_HINT = "Ask an admin for the user's ID.";

/** Maya, the admin, as the lead of Platform. */
const mayaInPlatform: fixtures.Me = {
  ...fixtures.me.maya,
  teams: [{ team_id: platform.id, name: platform.name, role: "lead" }],
};
const platformWithMaya: fixtures.TeamDetail = {
  team: { ...platform, member_count: 3 },
  members: [
    ...fixtures.teamDetails.platform.members,
    { user_id: maya.id, email: maya.email, name: maya.name, role: "lead" },
  ],
};

beforeAll(installPointerCapture);
afterEach(forgetToasts);

type Options = { user?: fixtures.Me; width?: number; queryClient?: QueryClient };

function list(options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/teams", ...options });
}

function detail(team: { id: number }, options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: `/teams/${team.id}`, ...options });
}

function teamsAre(teams: readonly fixtures.Team[]) {
  return counted("get", "/api/teams", () => ok("get", "/api/teams", 200, { teams: [...teams] }));
}

async function table(name: "Teams" | "Members"): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

function rowOf(text: string): HTMLElement {
  const row = screen.getByRole("cell", { name: new RegExp(`^${text}`) }).closest("tr");
  if (row === null) throw new Error(`no row for ${text}`);
  return row;
}

function button(name: string): HTMLElement {
  return screen.getByRole("button", { name });
}

function names(scope: HTMLElement): string[] {
  return within(scope)
    .queryAllByRole("button")
    .map((control) => control.textContent);
}

function actions(): string[] {
  const group = screen.queryByRole("group", { name: "Actions" });
  return group === null ? [] : names(group);
}

/** Presses the button of the member's row and gives the dialog that asks. */
async function askOf(member: { name: string }, action: string): Promise<HTMLElement> {
  await table("Members");
  await userEvent.click(within(rowOf(member.name)).getByRole("button", { name: action }));
  return screen.findByRole("alertdialog");
}

async function ask(action: string, title: string): Promise<HTMLElement> {
  await userEvent.click(await screen.findByRole("button", { name: action }));
  return screen.findByRole("alertdialog", { name: title });
}

function confirm(dialog: HTMLElement, label: string): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name: label }));
}

async function closed(): Promise<void> {
  await waitFor(() => {
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

async function open(action: string, title: string): Promise<HTMLElement> {
  await userEvent.click(await screen.findByRole("button", { name: action }));
  return screen.findByRole("dialog", { name: title });
}

/** A gateway that keeps one team and changes it as it is told. */
function keeps(start: fixtures.TeamDetail) {
  const state = {
    detail: start,
    /** Whether the caller may still see the team. */
    hidden: false,
    reads: 0,
    lists: 0,
    puts: [] as { userId: string | undefined; body: unknown }[],
    removed: [] as (string | undefined)[],
    patches: [] as unknown[],
  };
  function count(members: fixtures.TeamDetail["members"]): fixtures.TeamDetail {
    return { team: { ...state.detail.team, member_count: members.length }, members };
  }
  override("get", "/api/teams", () => {
    state.lists += 1;
    const teams = fixtures.teamList
      .map((team) => (team.id === start.team.id ? state.detail.team : team))
      .filter((team) => !(state.hidden && team.id === start.team.id));
    return ok("get", "/api/teams", 200, { teams });
  });
  override("get", "/api/teams/{id}", () => {
    state.reads += 1;
    return state.hidden ? refuse(errors.not_found) : ok("get", "/api/teams/{id}", 200, state.detail);
  });
  override("patch", "/api/teams/{id}", async ({ request }) => {
    const body: unknown = await request.json();
    state.patches.push(body);
    const name: unknown = typeof body === "object" && body !== null ? Reflect.get(body, "name") : "";
    if (typeof name === "string") {
      state.detail = { ...state.detail, team: { ...state.detail.team, name: name.trim() } };
    }
    return ok("patch", "/api/teams/{id}", 200, state.detail.team);
  });
  override("put", "/api/teams/{id}/members/{user_id}", async ({ request, params }) => {
    const body: unknown = await request.json();
    state.puts.push({ userId: params.user_id, body });
    const user = fixtures.userList.find((one) => String(one.id) === params.user_id);
    if (user === undefined) return refuse(errors.not_found);
    if (user.status === "disabled") return refuse(errors.user_disabled);
    const role: unknown = typeof body === "object" && body !== null ? Reflect.get(body, "role") : "";
    if (role !== "lead" && role !== "member") return refuse(errors.bad_request);
    state.detail = count([
      ...state.detail.members.filter((member) => member.user_id !== user.id),
      { user_id: user.id, email: user.email, name: user.name, role },
    ]);
    return noContent();
  });
  override("delete", "/api/teams/{id}/members/{user_id}", ({ params }) => {
    state.removed.push(params.user_id);
    state.detail = count(
      state.detail.members.filter((member) => String(member.user_id) !== params.user_id),
    );
    return noContent();
  });
  return state;
}

function meIs(me: fixtures.Me) {
  return counted("get", "/api/auth/me", () => ok("get", "/api/auth/me", 200, me));
}

describe("the list of teams", () => {
  test("list shows counts", async () => {
    await list();
    const teams = await table("Teams");
    expect(screen.getByRole("heading", { level: 1, name: "Teams" })).toBeInTheDocument();
    expect(within(teams).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Name",
      "Members",
      "Created",
    ]);
    for (const team of fixtures.teamList) {
      const link = screen.getByRole("link", { name: team.name });
      expect(link).toHaveAttribute("href", `/teams/${team.id}`);
      const row = link.closest("tr");
      if (row === null) throw new Error("no row");
      const cells = within(row).getAllByRole("cell");
      expect(cells[1]).toHaveTextContent(new RegExp(`^${team.member_count}$`));
      expect(within(row).getByText(/2026/).tagName).toBe("TIME");
    }
    expectOneMain();
  });

  test("new team is for admins", async () => {
    const first = await list();
    await table("Teams");
    expect(button("New team")).toBeInTheDocument();
    first.unmount();

    for (const me of [fixtures.me.arjun, fixtures.me.lena, fixtures.me.priya]) {
      teamsAre([platform]);
      const app = await list({ user: me });
      await table("Teams");
      expect(screen.queryByRole("button", { name: "New team" })).toBeNull();
      app.unmount();
    }
  });

  test("a new team is created, reported and listed", async () => {
    const created = counted("post", "/api/teams", () =>
      ok("post", "/api/teams", 201, { ...growth, id: 4, name: "Design" }),
    );
    await list();
    await table("Teams");
    const dialog = await open("New team", "New team");
    expect(within(dialog).getAllByRole("textbox")).toHaveLength(1);
    const listed = teamsAre([...fixtures.teamList, { ...growth, id: 4, name: "Design" }]);
    await userEvent.type(screen.getByLabelText("Name"), "Design");
    await userEvent.click(button("Create team"));
    await closed();
    expect(created.bodies).toEqual([{ name: "Design" }]);
    expect(await screen.findByRole("link", { name: "Design" })).toBeInTheDocument();
    expect(listed.calls).toBe(1);
    expect(toasts()).toEqual(["Team created."]);
  });

  test("duplicate name stays in the dialog", async () => {
    const created = counted("post", "/api/teams", () => refuse(errors.team_exists));
    await list();
    await table("Teams");
    const dialog = await open("New team", "New team");
    const name = screen.getByLabelText("Name");
    await userEvent.type(name, "Platform");
    await userEvent.click(button("Create team"));
    await waitFor(() => {
      expect(name).toHaveAttribute("aria-invalid", "true");
    });
    expect(descriptionOf(name)).toBe(errors.team_exists.body.error.message);
    expect(within(dialog).getAllByRole("alert")).toHaveLength(1);
    expect(name).toHaveValue("Platform");
    expect(name).toHaveFocus();
    expect(dialog).toBeInTheDocument();
    expect(toasts()).toEqual([]);
    expect(created.calls).toBe(1);

    // Opened again, the dialog is empty and shows no old error.
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    await open("New team", "New team");
    expect(screen.getByLabelText("Name")).toHaveValue("");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("a name that is not valid shows on its field", async () => {
    override("post", "/api/teams", () =>
      refuse(validationFailed({ name: fieldMessages.teamName })),
    );
    await list();
    await table("Teams");
    await open("New team", "New team");
    await userEvent.click(button("Create team"));
    const name = screen.getByLabelText("Name");
    await waitFor(() => {
      expect(descriptionOf(name)).toBe(fieldMessages.teamName);
    });
    expect(toasts()).toEqual([]);
  });

  test("while the team is created the button is disabled and says so", async () => {
    const door = gate();
    const created = counted("post", "/api/teams", async () => {
      await door.opened;
      return ok("post", "/api/teams", 201, growth);
    });
    await list();
    await table("Teams");
    await open("New team", "New team");
    const name = screen.getByLabelText("Name");
    await userEvent.type(name, "Design");
    await userEvent.click(button("Create team"));
    const running = await screen.findByRole("button", { name: "Creating the team" });
    expect(running).toBeDisabled();
    await userEvent.type(name, "{Enter}");
    expect(created.calls).toBe(1);
    act(() => {
      door.open();
    });
    await closed();
    expect(created.calls).toBe(1);
  });

  test("a team whose answer came for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("post", "/api/teams", async () => {
      await door.opened;
      return ok("post", "/api/teams", 201, growth);
    });
    const app = await list();
    await table("Teams");
    await open("New team", "New team");
    await userEvent.type(screen.getByLabelText("Name"), "Design");
    await userEvent.click(button("Create team"));
    await aCallFindsTheSessionEnded("/api/users");
    door.open();
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/teams")}`);
  });

  test("loading shows skeleton rows", async () => {
    const door = gate();
    override("get", "/api/teams", async () => {
      await door.opened;
      return ok("get", "/api/teams", 200, { teams: fixtures.teamList });
    });
    await list();
    const teams = screen.getByRole("table", { name: "Teams" });
    expect(teams).toHaveAttribute("aria-busy", "true");
    expect(teams.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expect(screen.queryByRole("link", { name: platform.name })).toBeNull();
    act(() => {
      door.open();
    });
    await table("Teams");
    expect(teams.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0);
  });

  test("an empty list says so", async () => {
    teamsAre([]);
    await list({ user: fixtures.me.priya });
    expect(await screen.findByRole("heading", { name: "No teams" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
  });

  test("a failed list call shows the error with Retry, and Retry asks again", async () => {
    const failing = counted("get", "/api/teams", () => refuse(errors.internal_error));
    await list();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(screen.queryByRole("table")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(failing.calls).toBe(1);
    expect(screen.queryByRole("button", { name: "New team" })).toBeNull();

    const again = teamsAre(fixtures.teamList);
    await userEvent.click(button("Retry"));
    await table("Teams");
    expect(again.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(button("New team")).toBeInTheDocument();
  });

  test("a gateway that cannot be reached is an error with Retry", async () => {
    override("get", "/api/teams", networkFailure);
    await list();
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(button("Retry")).toBeInTheDocument();
  });

  test("at width 390 the rows are cards with their labels and the link", async () => {
    await list({ width: 390 });
    const cards = await screen.findByRole("list", { name: "Teams" });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    expect(screen.queryByRole("table")).toBeNull();
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(fixtures.teamList.length);
    const [first] = items;
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Name",
      "Members",
      "Created",
    ]);
    const link = within(first).getByRole("link", { name: platform.name });
    expect(link).toHaveAttribute("href", `/teams/${platform.id}`);
    expect(link.className.split(/\s+/)).toContain("min-h-11");
    expect(button("New team").className.split(/\s+/)).toContain("min-h-11");
  });

  test("the list call answers 403: not available", async () => {
    forbid("/api/teams");
    await list();
    await expectNotAvailable();
    expectOneMain();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByRole("button", { name: "New team" })).toBeNull();
  });

  test("the session ends while the list is open", async () => {
    startGateway({ signedIn: true });
    const app = await list();
    await table("Teams");
    await expectSessionEndsOnPage(app, {
      path: "/api/teams",
      queryKey: queryKeys.teams.list(),
      at: "/teams",
    });
    expect(shown()).not.toContain(platform.name);
  });
});

describe("the page of a team", () => {
  test("it shows the team and its members", async () => {
    await detail(platform);
    expect(await screen.findByRole("heading", { level: 1, name: platform.name })).toBeVisible();
    const members = await table("Members");
    expect(within(members).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Name",
      "Email",
      "Role",
      "Actions",
    ]);
    expect(rowOf(arjun.name)).toHaveTextContent(arjun.email);
    expect(rowOf(arjun.name)).toHaveTextContent("Lead");
    expect(rowOf(lena.name)).toHaveTextContent(lena.email);
    expect(rowOf(lena.name)).toHaveTextContent("Member");
    expect(screen.getByRole("link", { name: "Back to teams" })).toHaveAttribute("href", "/teams");
    expectOneMain();
  });

  test("the own row is marked", async () => {
    await detail(platform, { user: fixtures.me.lena });
    await table("Members");
    expect(within(rowOf(lena.name)).getByText("You")).toBeInTheDocument();
    expect(within(rowOf(arjun.name)).queryByText("You")).toBeNull();
  });

  test("a role the console does not know is shown as it is", async () => {
    // The API description knows two roles; a later gateway may know more.
    override("get", "/api/teams/{id}", () =>
      HttpResponse.json({
        team: platform,
        members: [{ user_id: lena.id, email: lena.email, name: lena.name, role: "observer" }],
      }),
    );
    await detail(platform);
    await table("Members");
    const badge = within(rowOf(lena.name)).getByText("observer");
    expect(badge).toHaveAttribute("data-slot", "badge");
  });

  test("a team without members says so", async () => {
    await detail(growth);
    expect(await screen.findByRole("heading", { name: "No members" })).toBeInTheDocument();
    expect(actions()).toEqual(["Rename", "Add member", "Delete"]);
  });

  test("detail controls by role: an admin sees all controls", async () => {
    await detail(platform);
    await table("Members");
    expect(actions()).toEqual(["Rename", "Add member", "Delete"]);
    expect(names(rowOf(arjun.name))).toEqual(["Make member", "Remove"]);
    expect(names(rowOf(lena.name))).toEqual(["Make lead", "Remove"]);
  });

  test("detail controls by role: the lead of the team", async () => {
    await detail(platform, { user: fixtures.me.arjun });
    await table("Members");
    expect(actions()).toEqual(["Rename", "Add member"]);
    expect(names(rowOf(arjun.name))).toEqual(["Remove"]);
    expect(names(rowOf(lena.name))).toEqual(["Remove"]);
  });

  test("detail controls by role: a member of the team has no controls", async () => {
    await detail(platform, { user: fixtures.me.lena });
    const members = await table("Members");
    expect(actions()).toEqual([]);
    expect(names(members)).toEqual(["Name", "Email", "Role"]);
    expect(within(members).getAllByRole("columnheader")).toHaveLength(3);
  });

  test("detail controls by role: the lead of one team is a member in another", async () => {
    await detail(research, { user: fixtures.me.arjun });
    await table("Members");
    expect(actions()).toEqual([]);
    expect(names(rowOf(tomas.name))).toEqual([]);
  });

  test("lead cannot make a lead", async () => {
    await detail(platform, { user: fixtures.me.arjun });
    await table("Members");
    expect(screen.queryByRole("button", { name: "Make lead" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Make member" })).toBeNull();

    // Whatever the API refuses is said where it was asked for.
    const removes = counted("delete", "/api/teams/{id}/members/{user_id}", () =>
      refuse(errors.forbidden),
    );
    const dialog = await askOf(lena, "Remove");
    await confirm(dialog, "Remove");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.forbidden.body.error.message,
    );
    expect(removes.calls).toBe(1);
    expect(toasts()).toEqual([]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();

    const puts = counted("put", "/api/teams/{id}/members/{user_id}", () =>
      refuse(errors.forbidden),
    );
    const form = await open("Add member", "Add member");
    await userEvent.type(screen.getByLabelText("User ID"), String(priya.id));
    await userEvent.click(within(form).getByRole("button", { name: "Add member" }));
    expect(await within(form).findByRole("alert")).toHaveTextContent(
      errors.forbidden.body.error.message,
    );
    expect(puts.calls).toBe(1);
    expect(toasts()).toEqual([]);
    expect(screen.queryByRole("heading", { name: "Not available" })).toBeNull();
  });

  test("an admin whose change of a role is refused is told in the dialog", async () => {
    override("put", "/api/teams/{id}/members/{user_id}", () => refuse(errors.forbidden));
    await detail(platform);
    const dialog = await askOf(lena, "Make lead");
    await confirm(dialog, "Make lead");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.forbidden.body.error.message,
    );
    expect(toasts()).toEqual([]);
  });

  test("loading shows a skeleton", async () => {
    const door = gate();
    override("get", "/api/teams/{id}", async () => {
      await door.opened;
      return ok("get", "/api/teams/{id}", 200, fixtures.teamDetails.platform);
    });
    await detail(platform);
    const loading = screen.getByRole("status", { name: "Loading the team" });
    expect(loading).toHaveAttribute("aria-busy", "true");
    expect(loading.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    act(() => {
      door.open();
    });
    expect(await screen.findByRole("heading", { level: 1, name: platform.name })).toBeVisible();
    expect(screen.queryByRole("status", { name: "Loading the team" })).toBeNull();
  });

  test("hidden team is not found", async () => {
    const asked = counted("get", "/api/teams/{id}", () => refuse(errors.not_found));
    await detail(research, { user: fixtures.me.lena });
    expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();
    expect(asked.calls).toBe(1);
    expectOneMain();
    expect(screen.getByRole("navigation", { name: "Main" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(research.name)).toBeNull();
    expect(actions()).toEqual([]);
  });

  test.each(["0", "-1", "1.5", "abc", "01", "1e3", "%20", "99999999999999999999"])(
    "the id %s is not found, and the API is not asked",
    async (id) => {
      const asked = counted("get", "/api/teams/{id}", () => refuse(errors.not_found));
      await renderWithApp(null, { route: `/teams/${id}` });
      expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();
      await settle();
      expect(asked.calls).toBe(0);
      expectOneMain();
      expect(screen.getByRole("navigation", { name: "Main" })).toBeInTheDocument();
    },
  );

  test("a failed call shows the error with Retry, and Retry asks again", async () => {
    override("get", "/api/teams/{id}", () => refuse(errors.internal_error));
    await detail(platform);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(actions()).toEqual([]);
    const again = counted("get", "/api/teams/{id}", () =>
      ok("get", "/api/teams/{id}", 200, fixtures.teamDetails.platform),
    );
    await userEvent.click(button("Retry"));
    expect(await screen.findByRole("heading", { level: 1, name: platform.name })).toBeVisible();
    expect(again.calls).toBe(1);
  });

  test("the call answers 403: not available", async () => {
    forbid("/api/teams/{id}");
    await detail(platform);
    await expectNotAvailable();
    expectOneMain();
  });

  test("a team that is hidden when it is asked for again is not found, though it was shown", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    const app = await detail(platform);
    await table("Members");
    state.hidden = true;
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: queryKeys.teams.detail(platform.id) });
    });
    expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();
    expect(state.reads).toBe(2);
    expectOneMain();
    expect(screen.queryByRole("heading", { name: platform.name })).toBeNull();
    expect(shown()).not.toContain(lena.email);
    expect(actions()).toEqual([]);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("asking again that fails with something else keeps what is shown", async () => {
    const app = await detail(platform);
    await table("Members");
    const again = counted("get", "/api/teams/{id}", () => refuse(errors.internal_error));
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: queryKeys.teams.detail(platform.id) });
    });
    expect(again.calls).toBe(1);
    await settle();
    expect(screen.getByRole("heading", { level: 1, name: platform.name })).toBeInTheDocument();
    expect(rowOf(lena.name)).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: NOT_FOUND })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("the session ends while the page of a team is open", async () => {
    startGateway({ signedIn: true });
    const app = await detail(platform);
    await table("Members");
    await expectSessionEndsOnPage(app, {
      path: "/api/teams/{id}",
      queryKey: queryKeys.teams.detail(platform.id),
      at: `/teams/${platform.id}`,
    });
    expect(shown()).not.toContain(lena.email);
  });

  test("at width 390 the members are cards with their labels and actions", async () => {
    await detail(platform, { width: 390 });
    const cards = await screen.findByRole("list", { name: "Members" });
    expect(screen.queryByRole("table")).toBeNull();
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    const [first] = items;
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Name",
      "Email",
      "Role",
    ]);
    expect(names(first)).toEqual(["Make member", "Remove"]);
    const group = screen.getByRole("group", { name: "Actions" });
    expect(group.className.split(/\s+/)).toContain("flex-wrap");
    for (const control of screen.getAllByRole("button", { name: /Rename|Add|Delete|Make|Remove/ })) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
  });
});

describe("renaming a team", () => {
  test("a new name is sent, shown and reported", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    const me = meIs(fixtures.me.maya);
    await detail(platform);
    await open("Rename", "Rename team");
    const name = screen.getByLabelText("Name");
    expect(name).toHaveValue(platform.name);
    await userEvent.clear(name);
    await userEvent.type(name, "Core");
    const before = me.calls;
    await userEvent.click(button("Save"));
    expect(await screen.findByRole("heading", { level: 1, name: "Core" })).toBeVisible();
    await closed();
    expect(state.patches).toEqual([{ name: "Core" }]);
    expect(toasts()).toEqual(["Team renamed."]);
    // The names of the caller's teams are in `me`.
    await waitFor(() => {
      expect(me.calls).toBe(before + 1);
    });
  });

  test("a lead renames their team", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    await detail(platform, { user: fixtures.me.arjun });
    await open("Rename", "Rename team");
    await userEvent.type(screen.getByLabelText("Name"), " 2");
    await userEvent.click(button("Save"));
    await closed();
    expect(state.patches).toEqual([{ name: "Platform 2" }]);
  });

  test("a name that is taken stays in the dialog, on the field", async () => {
    override("patch", "/api/teams/{id}", () => refuse(errors.team_exists));
    await detail(platform);
    const dialog = await open("Rename", "Rename team");
    const name = screen.getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Research");
    await userEvent.click(button("Save"));
    await waitFor(() => {
      expect(descriptionOf(name)).toBe(errors.team_exists.body.error.message);
    });
    expect(name).toHaveValue("Research");
    expect(dialog).toBeInTheDocument();
    expect(toasts()).toEqual([]);
  });

  test("while the name is saved the button is disabled and says so", async () => {
    const door = gate();
    const patches = counted("patch", "/api/teams/{id}", async () => {
      await door.opened;
      return ok("patch", "/api/teams/{id}", 200, platform);
    });
    await detail(platform);
    await open("Rename", "Rename team");
    const name = screen.getByLabelText("Name");
    await userEvent.type(name, " 2");
    await userEvent.click(button("Save"));
    expect(await screen.findByRole("button", { name: "Saving" })).toBeDisabled();
    await userEvent.type(name, "{Enter}");
    expect(patches.calls).toBe(1);
    act(() => {
      door.open();
    });
    await closed();
  });
});

describe("adding a member", () => {
  test("admin picks a user from a list", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    await detail(platform);
    const dialog = await open("Add member", "Add member");
    const group = await within(dialog).findByRole("radiogroup", { name: "User" });
    const offered = within(group)
      .getAllByRole("radio")
      .map((radio) => radio.getAttribute("aria-label") ?? radio.id);
    // Not Arjun and Lena, who are in the team, and not Dana, who is disabled.
    const expected = [maya, tomas, priya, sam];
    expect(within(group).getAllByRole("radio")).toHaveLength(expected.length);
    expect(offered).toHaveLength(expected.length);
    for (const user of expected) {
      expect(within(group).getByRole("radio", { name: new RegExp(user.name) })).toBeEnabled();
      expect(group).toHaveTextContent(user.email);
    }
    for (const user of [arjun, lena, dana]) {
      expect(group).not.toHaveTextContent(user.name);
    }
    expect(screen.queryByLabelText("User ID")).toBeNull();

    await userEvent.click(within(group).getByRole("radio", { name: new RegExp(priya.name) }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Add member" }));
    await closed();
    expect(state.puts).toEqual([{ userId: String(priya.id), body: { role: "member" } }]);
    expect(toasts()).toEqual(["Member added."]);
    await waitFor(() => {
      expect(rowOf(priya.name)).toHaveTextContent("Member");
    });
  });

  test("an admin who chose nobody is asked to choose, and nothing is sent", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    await detail(platform);
    const dialog = await open("Add member", "Add member");
    const group = await within(dialog).findByRole("radiogroup", { name: "User" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Add member" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Choose a user.");
    // The list says it, and nothing else does.
    expect(descriptionOf(group)).toBe("Choose a user.");
    expect(within(dialog).getAllByRole("alert")).toHaveLength(1);
    expect(toasts()).toEqual([]);
    await settle();
    expect(state.puts).toEqual([]);
  });

  test("when everybody is in the team the dialog says so", async () => {
    override("get", "/api/users", () => ok("get", "/api/users", 200, { users: [arjun, lena, dana] }));
    await detail(platform);
    const dialog = await open("Add member", "Add member");
    expect(
      await within(dialog).findByText("Every user who can be added is in this team already."),
    ).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Add member" })).toBeDisabled();
  });

  test("the list of users of the dialog: loading, failed with Retry", async () => {
    const failing = counted("get", "/api/users", () => refuse(errors.internal_error));
    await detail(platform);
    await table("Members");
    // The page itself does not ask for the users.
    expect(failing.calls).toBe(0);
    const dialog = await open("Add member", "Add member");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(within(dialog).getByRole("button", { name: "Add member" })).toBeDisabled();
    const door = gate();
    override("get", "/api/users", async () => {
      await door.opened;
      return ok("get", "/api/users", 200, { users: fixtures.userList });
    });
    await userEvent.click(within(dialog).getByRole("button", { name: "Retry" }));
    const loading = await within(dialog).findByRole("status", { name: "Loading the users" });
    expect(loading).toHaveAttribute("aria-busy", "true");
    act(() => {
      door.open();
    });
    expect(await within(dialog).findByRole("radiogroup", { name: "User" })).toBeInTheDocument();
  });

  test("a user who was disabled meanwhile: the refusal is on the list", async () => {
    override("put", "/api/teams/{id}/members/{user_id}", () => refuse(errors.user_disabled));
    await detail(platform);
    const dialog = await open("Add member", "Add member");
    const group = await within(dialog).findByRole("radiogroup", { name: "User" });
    await userEvent.click(within(group).getByRole("radio", { name: new RegExp(priya.name) }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Add member" }));
    await waitFor(() => {
      expect(descriptionOf(group)).toBe(errors.user_disabled.body.error.message);
    });
    expect(dialog).toBeInTheDocument();
    expect(toasts()).toEqual([]);
  });

  test("lead adds by id", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    const users = counted("get", "/api/users", () => refuse(errors.forbidden));
    await detail(platform, { user: fixtures.me.arjun });
    const dialog = await open("Add member", "Add member");
    const id = screen.getByLabelText("User ID");
    expect(descriptionOf(id)).toBe(ID_HINT);
    expect(within(dialog).queryByRole("radiogroup")).toBeNull();

    // An id that no user has.
    await userEvent.type(id, "999");
    await userEvent.click(within(dialog).getByRole("button", { name: "Add member" }));
    await waitFor(() => {
      expect(id).toHaveAttribute("aria-invalid", "true");
    });
    expect(descriptionOf(id)).toBe(`${NO_USER} ${ID_HINT}`);
    expect(screen.queryByText(errors.not_found.body.error.message)).toBeNull();
    expect(state.puts).toEqual([{ userId: "999", body: { role: "member" } }]);
    expect(toasts()).toEqual([]);
    expect(dialog).toBeInTheDocument();
    expect(id).toHaveValue("999");
    // The error goes when the field is changed.
    await userEvent.clear(id);
    expect(descriptionOf(id)).toBe(ID_HINT);
    expect(users.calls).toBe(0);
  });

  test("lead adds by id: the user is added as a member", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    const users = counted("get", "/api/users", () => refuse(errors.forbidden));
    await detail(platform, { user: fixtures.me.arjun });
    const dialog = await open("Add member", "Add member");
    await userEvent.type(screen.getByLabelText("User ID"), String(priya.id));
    await userEvent.click(within(dialog).getByRole("button", { name: "Add member" }));
    await closed();
    expect(state.puts).toEqual([{ userId: String(priya.id), body: { role: "member" } }]);
    expect(toasts()).toEqual(["Member added."]);
    await waitFor(() => {
      expect(rowOf(priya.name)).toHaveTextContent("Member");
    });
    // A lead is not given the list of users.
    expect(users.calls).toBe(0);
  });

  test.each(["0", "-4", "1.5", "abc", "007", " "])(
    "what is no id (%s) is refused on the field, and the API is not asked",
    async (text) => {
      const state = keeps(fixtures.teamDetails.platform);
      await detail(platform, { user: fixtures.me.arjun });
      const dialog = await open("Add member", "Add member");
      const id = screen.getByLabelText("User ID");
      await userEvent.type(id, text);
      await userEvent.click(within(dialog).getByRole("button", { name: "Add member" }));
      await waitFor(() => {
        expect(descriptionOf(id)).toBe(`${NO_USER} ${ID_HINT}`);
      });
      expect(within(dialog).getAllByRole("alert")).toHaveLength(1);
      await settle();
      expect(state.puts).toEqual([]);
    },
  );

  test("a 404 can be the team: a team that is gone meanwhile is not found", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    const app = await detail(platform, { user: fixtures.me.arjun });
    const dialog = await open("Add member", "Add member");
    await userEvent.type(screen.getByLabelText("User ID"), String(priya.id));
    expect(state.reads).toBe(1);
    // Meanwhile the team was deleted, or hidden from the caller. The gateway
    // answers 404 for the team before it looks at the user.
    state.hidden = true;
    const puts = counted("put", "/api/teams/{id}/members/{user_id}", () =>
      refuse(errors.not_found),
    );
    await userEvent.click(within(dialog).getByRole("button", { name: "Add member" }));
    expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();
    expectOneMain();
    expect(puts.calls).toBe(1);
    expect(state.reads).toBe(2);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("heading", { name: platform.name })).toBeNull();
    expect(shown()).not.toContain(lena.email);
    expect(actions()).toEqual([]);
    expect(toasts()).toEqual([]);
    expect(screen.getByRole("navigation", { name: "Main" })).toBeInTheDocument();
    expect(href(app)).toBe(`/teams/${platform.id}`);
  });

  test("a 404 can be the team: when the team is still there the field says it, and the page stays", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    const app = await detail(platform, { user: fixtures.me.arjun });
    const dialog = await open("Add member", "Add member");
    const id = screen.getByLabelText("User ID");
    await userEvent.type(id, "999");
    expect(state.reads).toBe(1);
    await userEvent.click(within(dialog).getByRole("button", { name: "Add member" }));
    await waitFor(() => {
      expect(descriptionOf(id)).toBe(`${NO_USER} ${ID_HINT}`);
    });
    // The team was asked for again, and is still there.
    await waitFor(() => {
      expect(state.reads).toBe(2);
    });
    await settle();
    expect(state.reads).toBe(2);
    expect(dialog).toBeInTheDocument();
    expect(id).toHaveValue("999");
    expect(descriptionOf(id)).toBe(`${NO_USER} ${ID_HINT}`);
    // Behind the dialog, which hides the page from the roles.
    expect(
      screen.getByRole("heading", { level: 1, name: platform.name, hidden: true }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: NOT_FOUND, hidden: true })).toBeNull();
    expect(document.querySelectorAll("main")).toHaveLength(1);
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/teams/${platform.id}`);
  });

  test("a lead adds a disabled user: the refusal is on the field", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    await detail(platform, { user: fixtures.me.arjun });
    const dialog = await open("Add member", "Add member");
    const id = screen.getByLabelText("User ID");
    await userEvent.type(id, String(dana.id));
    await userEvent.click(within(dialog).getByRole("button", { name: "Add member" }));
    await waitFor(() => {
      expect(descriptionOf(id)).toBe(`${errors.user_disabled.body.error.message} ${ID_HINT}`);
    });
    expect(state.puts).toHaveLength(1);
    expect(toasts()).toEqual([]);
  });

  test("while the member is added the button is disabled and says so", async () => {
    const door = gate();
    const puts = counted("put", "/api/teams/{id}/members/{user_id}", async () => {
      await door.opened;
      return noContent();
    });
    await detail(platform, { user: fixtures.me.arjun });
    const dialog = await open("Add member", "Add member");
    const id = screen.getByLabelText("User ID");
    await userEvent.type(id, String(priya.id));
    await userEvent.click(within(dialog).getByRole("button", { name: "Add member" }));
    expect(await within(dialog).findByRole("button", { name: "Adding" })).toBeDisabled();
    await userEvent.type(id, "{Enter}");
    expect(puts.calls).toBe(1);
    act(() => {
      door.open();
    });
    await closed();
  });
});

describe("changing the role of a member", () => {
  test("make lead confirms with the name and reports", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    await detail(platform);
    const dialog = await askOf(lena, "Make lead");
    expect(dialog).toHaveAccessibleName(`Make ${lena.name} a lead?`);
    expect(dialog).toHaveTextContent(
      "They can rename the team, add and remove its members, and manage its keys.",
    );
    expect(state.puts).toEqual([]);
    await confirm(dialog, "Make lead");
    await closed();
    expect(state.puts).toEqual([{ userId: String(lena.id), body: { role: "lead" } }]);
    expect(toasts()).toEqual(["Role changed."]);
    await waitFor(() => {
      expect(names(rowOf(lena.name))).toEqual(["Make member", "Remove"]);
    });
    expect(rowOf(lena.name)).toHaveTextContent("Lead");
  });

  test("make member confirms with the name and reports", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    await detail(platform);
    const dialog = await askOf(arjun, "Make member");
    expect(dialog).toHaveAccessibleName(`Make ${arjun.name} a member?`);
    expect(dialog).toHaveTextContent("They can no longer manage the team.");
    await confirm(dialog, "Make member");
    await closed();
    expect(state.puts).toEqual([{ userId: String(arjun.id), body: { role: "member" } }]);
    expect(toasts()).toEqual(["Role changed."]);
  });

  test("cancel changes nothing", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    await detail(platform);
    const dialog = await askOf(lena, "Make lead");
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    expect(state.puts).toEqual([]);
    expect(toasts()).toEqual([]);
  });

  test("an admin who changes their own role in the team stays signed in", async () => {
    const state = keeps(platformWithMaya);
    const me = meIs(mayaInPlatform);
    const app = await detail(platform);
    const dialog = await askOf(maya, "Make member");
    expect(dialog).toHaveAccessibleName("Make yourself a member?");
    expect(dialog).toHaveTextContent("Your role in this team changes. You stay signed in.");
    const before = me.calls;
    await confirm(dialog, "Make member");
    await closed();
    expect(state.puts).toEqual([{ userId: String(maya.id), body: { role: "member" } }]);
    // The teams of the caller are asked for again.
    await waitFor(() => {
      expect(me.calls).toBe(before + 1);
    });
    await settle();
    expect(href(app)).toBe(`/teams/${platform.id}`);
    expect(toasts()).toEqual(["Role changed."]);
    expect(rowOf(maya.name)).toHaveTextContent("Member");
    expect(actions()).toEqual(["Rename", "Add member", "Delete"]);
  });

  test("a lead who is made a member meanwhile loses the controls at once", async () => {
    const app = await detail(platform, { user: fixtures.me.arjun });
    await table("Members");
    expect(actions()).toEqual(["Rename", "Add member"]);
    meIs({
      ...fixtures.me.arjun,
      teams: [{ team_id: platform.id, name: platform.name, role: "member" }],
    });
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: queryKeys.me() });
    });
    await waitFor(() => {
      expect(actions()).toEqual([]);
    });
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
  });

  test("a change whose answer came for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("put", "/api/teams/{id}/members/{user_id}", async () => {
      await door.opened;
      return noContent();
    });
    const app = await detail(platform);
    const dialog = await askOf(lena, "Make lead");
    await confirm(dialog, "Make lead");
    await aCallFindsTheSessionEnded("/api/keys");
    door.open();
    await settle();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent(`/teams/${platform.id}`)}`);
  });
});

describe("removing a member", () => {
  test("remove confirms with the name", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    const app = await detail(platform);
    const dialog = await askOf(lena, "Remove");
    expect(dialog).toHaveAccessibleName(`Remove ${lena.name}?`);
    expect(dialog).toHaveTextContent("They will lose access to this team.");
    expect(dialog).not.toHaveTextContent(LEAVE);
    expect(state.removed).toEqual([]);
    await confirm(dialog, "Remove");
    await closed();
    expect(state.removed).toEqual([String(lena.id)]);
    expect(toasts()).toEqual(["Member removed."]);
    await waitFor(() => {
      expect(screen.queryByRole("cell", { name: new RegExp(lena.name) })).toBeNull();
    });
    expect(rowOf(arjun.name)).toBeInTheDocument();
    expect(href(app)).toBe(`/teams/${platform.id}`);
  });

  test("a lead removes a member and stays on the page", async () => {
    const state = keeps(fixtures.teamDetails.platform);
    const app = await detail(platform, { user: fixtures.me.arjun });
    const dialog = await askOf(lena, "Remove");
    expect(dialog).toHaveAccessibleName(`Remove ${lena.name}?`);
    expect(dialog).not.toHaveTextContent(LEAVE);
    await confirm(dialog, "Remove");
    await closed();
    expect(state.removed).toEqual([String(lena.id)]);
    expect(href(app)).toBe(`/teams/${platform.id}`);
  });

  test("lead removing themselves is warned", async () => {
    const gateway = startGateway({ signedIn: true, me: fixtures.me.arjun });
    const state = keeps(fixtures.teamDetails.platform);
    // The page is reached from the list, which is mounted again afterwards.
    // Its data stays fresh: it is asked for again only because leaving
    // marked it as stale.
    const app = await list({ queryClient: clientThatKeepsDataFresh() });
    await table("Teams");
    await userEvent.click(screen.getByRole("link", { name: platform.name }));
    const dialog = await askOf(arjun, "Remove");
    expect(state.lists).toBe(1);
    expect(dialog).toHaveAccessibleName(`Remove ${arjun.name}?`);
    expect(dialog).toHaveTextContent(LEAVE);
    expect(dialog).not.toHaveTextContent("They will");

    // Afterwards the gateway hides the team from them.
    const left = counted("delete", "/api/teams/{id}/members/{user_id}", () => {
      state.hidden = true;
      override("get", "/api/auth/me", () => {
        gateway.meCalls += 1;
        return ok("get", "/api/auth/me", 200, {
          ...fixtures.me.arjun,
          teams: fixtures.me.arjun.teams.filter((team) => team.team_id !== platform.id),
        });
      });
      return noContent();
    });
    const before = gateway.meCalls;
    await confirm(dialog, "Remove");
    await waitFor(() => {
      expect(href(app)).toBe("/teams");
    });
    await table("Teams");
    await waitFor(() => {
      expect(screen.queryByRole("link", { name: platform.name })).toBeNull();
    });
    expect(screen.getByRole("link", { name: research.name })).toBeInTheDocument();
    expect(left.calls).toBe(1);
    expect(state.lists).toBe(2);
    await waitFor(() => {
      expect(gateway.meCalls).toBe(before + 1);
    });
    await waitFor(() => {
      expect(toasts()).toEqual(["You left the team."]);
    });
    await settle();
    // They are still signed in, and nothing says "not found" or that the session ended.
    expect(href(app)).toBe("/teams");
    expect(gateway.logouts).toBe(0);
    expect(screen.queryByRole("heading", { name: NOT_FOUND })).toBeNull();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("navigation", { name: "Main" })).toBeInTheDocument();

    // The page of the team is not theirs to see any more.
    await act(async () => {
      await app.router.navigate({ to: `/teams/${platform.id}` });
    });
    expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();
    expectOneMain();
  });

  test("an admin who removes themselves keeps the team and stays on the page", async () => {
    const state = keeps(platformWithMaya);
    const me = meIs(mayaInPlatform);
    const app = await detail(platform);
    const dialog = await askOf(maya, "Remove");
    expect(dialog).not.toHaveTextContent(LEAVE);
    expect(dialog).toHaveTextContent("As an admin you can still see and manage this team.");
    const before = me.calls;
    await confirm(dialog, "Remove");
    await closed();
    expect(state.removed).toEqual([String(maya.id)]);
    await waitFor(() => {
      expect(me.calls).toBe(before + 1);
    });
    expect(href(app)).toBe(`/teams/${platform.id}`);
    expect(toasts()).toEqual(["Member removed."]);
  });

  test("while a member is removed both buttons are disabled", async () => {
    const door = gate();
    const removes = counted("delete", "/api/teams/{id}/members/{user_id}", async () => {
      await door.opened;
      return noContent();
    });
    await detail(platform);
    const dialog = await askOf(lena, "Remove");
    await confirm(dialog, "Remove");
    await waitFor(() => {
      expect(within(dialog).getByRole("button", { name: "Remove" })).toBeDisabled();
    });
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(removes.calls).toBe(1);
    act(() => {
      door.open();
    });
    await closed();
  });
});

describe("deleting a team", () => {
  test("delete confirms with the consequence", async () => {
    const removed = counted("delete", "/api/teams/{id}", noContent);
    // The list is left and mounted again. Its data stays fresh, so that it is
    // asked for again only because the delete marked it as stale.
    const app = await list({ queryClient: clientThatKeepsDataFresh() });
    await table("Teams");
    await userEvent.click(screen.getByRole("link", { name: platform.name }));
    const dialog = await ask("Delete", "Delete this team?");
    expect(dialog).toHaveTextContent(DELETE);
    expect(removed.calls).toBe(0);

    const read = counted("get", "/api/teams/{id}", () => refuse(errors.not_found));
    const listed = teamsAre([research, growth]);
    await confirm(dialog, "Delete");
    await waitFor(() => {
      expect(href(app)).toBe("/teams");
    });
    await table("Teams");
    await waitFor(() => {
      expect(screen.queryByRole("link", { name: platform.name })).toBeNull();
    });
    expect(removed.calls).toBe(1);
    expect(listed.calls).toBe(1);
    await waitFor(() => {
      expect(toasts()).toEqual(["Team deleted."]);
    });
    await settle();
    expect(read.calls).toBe(0);
    expect(screen.queryByRole("heading", { name: NOT_FOUND })).toBeNull();
  });

  test("a refusal stays in the dialog", async () => {
    override("delete", "/api/teams/{id}", () => refuse(errors.forbidden));
    const app = await detail(platform);
    const dialog = await ask("Delete", "Delete this team?");
    await confirm(dialog, "Delete");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.forbidden.body.error.message,
    );
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/teams/${platform.id}`);
  });
});
