import type { QueryClient } from "@tanstack/react-query";
import { act, screen, waitFor, within } from "@testing-library/react";
import { createMemoryHistory } from "@tanstack/react-router";
import userEvent from "@testing-library/user-event";
import { HttpResponse } from "msw";
import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";
import { ApiError, ConsoleRefusal } from "@/api/errors";
import { queryKeys } from "@/api/queries";
import { inviteUrl } from "@/pages/Users";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, noContent, ok, override, refuse } from "@/test/handlers";
import {
  aCallFindsTheSessionEnded,
  aWayThatIsHeld,
  cached,
  clientThatKeepsDataFresh,
  counted,
  descriptionOf,
  expectNoSecret,
  expectLabelsNameControls,
  expectNotAvailable,
  expectOneRequestWhileTheDialogStays,
  expectOneH1,
  expectOneMain,
  expectSessionEndsOnPage,
  expectTheDialogCanBeLeft,
  forbid,
  forgetToasts,
  held,
  href,
  inside,
  installPointerCapture,
  NOT_FOUND,
  sendAndLeaveAtOnce,
  SESSION_ENDED,
  settle,
  shown,
  theBrowserIsOffline,
  theWindowGetsTheFocus,
  toasts,
  watchTheDocument,
  watchTheWayFrom,
} from "@/test/pages";
import { renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

const { maya, arjun, lena, tomas, sam, dana } = fixtures.users;
const LINK = window.location.origin + fixtures.newInviteLink;
const TOKEN = fixtures.newInviteToken;
const LINK_DESCRIPTION = "Send this link to the user. It works once and expires in 7 days.";

const ROLE = "Their current sessions end and they must sign in again.";
const DISABLE =
  "They are signed out, their access tokens are revoked, and their virtual keys stop working until they are enabled again.";
const DELETE_ACTIVE =
  "Their virtual keys keep working without an owner. Revoke the keys first if they should stop.";
const DELETE_NOT_ACTIVE = "Their virtual keys are revoked.";

beforeAll(installPointerCapture);
afterEach(forgetToasts);

function usersAre(list: readonly fixtures.User[]) {
  return counted("get", "/api/users", () => ok("get", "/api/users", 200, { users: [...list] }));
}

function list(
  options: { user?: fixtures.Me; width?: number; queryClient?: QueryClient } = {},
): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/users", ...options });
}

function detail(
  user: { id: number },
  options: { user?: fixtures.Me; width?: number } = {},
): Promise<AppRenderResult> {
  return renderWithApp(null, { route: `/users/${user.id}`, ...options });
}

async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: "Users" });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

function rowOf(name: string): HTMLElement {
  const row = screen.getByRole("link", { name }).closest("tr");
  if (row === null) throw new Error(`no row for ${name}`);
  return row;
}

function button(name: string): HTMLElement {
  return screen.getByRole("button", { name });
}

function actions(): string[] {
  const group = screen.queryByRole("group", { name: "Actions" });
  if (group === null) return [];
  return within(group)
    .getAllByRole("button")
    .map((control) => control.textContent);
}

async function openInvite(): Promise<HTMLElement> {
  await userEvent.click(button("Invite user"));
  return screen.findByRole("dialog", { name: "Invite user" });
}

/**
 * Fills the invite form and sends it. Each field takes its text at once, as a
 * paste does: how a field takes what is typed is not what the tests that call
 * it look at. "invite errors: fields that are not valid" types for real.
 */
async function fillInvite(name = "Sam Carter", email = "sam@example.test"): Promise<void> {
  await userEvent.click(screen.getByLabelText("Name"));
  await userEvent.paste(name);
  await userEvent.click(screen.getByLabelText("Email"));
  await userEvent.paste(email);
  await userEvent.click(button("Create invite link"));
}

async function closeSecret(): Promise<void> {
  await userEvent.click(button("Done"));
  const question = await screen.findByRole("alertdialog");
  await userEvent.click(within(question).getByRole("button", { name: "Close" }));
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

/** Opens the page of the user and the dialog of the action. */
async function ask(action: string, title: string): Promise<HTMLElement> {
  await userEvent.click(await screen.findByRole("button", { name: action }));
  return screen.findByRole("alertdialog", { name: title });
}

function confirm(dialog: HTMLElement, label: string): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name: label }));
}

/**
 * What shows of Lena, a user of the fixtures: the heading, the email, the
 * controls. For `watchTheDocument`.
 */
function ofLena(scope: Element): string[] {
  const found: string[] = [];
  if (inside(scope, "h1").some((heading) => heading.textContent === lena.name)) {
    found.push("the heading");
  }
  if (scope.textContent.includes(lena.email)) found.push("the email");
  if (inside(scope, '[role="group"][aria-label="Actions"]').length > 0) found.push("the controls");
  return found;
}

describe("the list of users", () => {
  test("admin sees the list and the invite button", async () => {
    await list();
    const users = await table();
    expect(screen.getByRole("heading", { level: 1, name: "Users" })).toBeInTheDocument();
    expect(within(users).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Name",
      "Email",
      "Role",
      "Status",
      "Last active",
    ]);
    for (const user of fixtures.userList) {
      const row = rowOf(user.name);
      expect(within(row).getByRole("link", { name: user.name })).toHaveAttribute(
        "href",
        `/users/${user.id}`,
      );
      expect(row).toHaveTextContent(user.email);
    }
    expect(rowOf(maya.name)).toHaveTextContent("Admin");
    expect(rowOf(arjun.name)).toHaveTextContent("Member");
    // A user who was never active.
    expect(rowOf(sam.name)).toHaveTextContent("Never");
    expect(within(rowOf(maya.name)).getByText(/2026/).tagName).toBe("TIME");
    expect(button("Invite user")).toBeInTheDocument();
    expectOneMain();
    expectOneH1();
  });

  test("member sees only what the API returns and no invite button", async () => {
    usersAre([arjun, tomas]);
    await list({ user: fixtures.me.tomas });
    await table();
    expect(screen.getAllByRole("row")).toHaveLength(3);
    expect(rowOf(arjun.name)).toBeInTheDocument();
    expect(rowOf(tomas.name)).toBeInTheDocument();
    expect(screen.queryByText(maya.email)).toBeNull();
    expect(screen.queryByRole("button", { name: "Invite user" })).toBeNull();
  });

  test("a row links to the page of the user", async () => {
    await list();
    await table();
    await userEvent.click(screen.getByRole("link", { name: lena.name }));
    expect(await screen.findByRole("heading", { level: 1, name: lena.name })).toBeInTheDocument();
  });

  test("status pills", async () => {
    await list();
    await table();
    for (const [user, tone] of [
      [maya, "default"],
      [sam, "secondary"],
      [dana, "outline"],
    ] as const) {
      const pill = within(rowOf(user.name)).getByText(user.status);
      expect(pill).toHaveAttribute("data-slot", "badge");
      expect(pill).toHaveAttribute("data-variant", tone);
    }
  });

  test("a status and a role the console does not know are shown as they are", async () => {
    override("get", "/api/users", () =>
      // What a later gateway may answer: the description of the API has neither value.
      HttpResponse.json({ users: [{ ...lena, status: "locked", role: "auditor" }] }),
    );
    await list();
    await table();
    for (const text of ["locked", "auditor"]) {
      const pill = within(rowOf(lena.name)).getByText(text);
      expect(pill).toHaveAttribute("data-slot", "badge");
      expect(pill).toHaveAttribute("data-variant", "outline");
    }
  });

  test("own row is marked", async () => {
    await list({ user: fixtures.me.arjun });
    await table();
    expect(within(rowOf(arjun.name)).getByText("You")).toBeInTheDocument();
    expect(screen.getAllByText("You")).toHaveLength(1);
  });

  test("the list sorts by name", async () => {
    await list();
    const users = await table();
    await userEvent.click(within(users).getByRole("button", { name: "Name" }));
    const names = within(users)
      .getAllByRole("link")
      .map((link) => link.textContent);
    expect(names).toEqual(fixtures.userList.map((user) => user.name).sort());
  });

  test("loading shows skeleton rows", async () => {
    const door = gate();
    override("get", "/api/users", async () => {
      await door.opened;
      return ok("get", "/api/users", 200, { users: fixtures.userList });
    });
    await list();
    const users = screen.getByRole("table", { name: "Users" });
    expect(users).toHaveAttribute("aria-busy", "true");
    expect(users.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expect(screen.queryByRole("link", { name: maya.name })).toBeNull();
    act(() => {
      door.open();
    });
    await table();
    expect(users.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0);
  });

  test("an empty list says so", async () => {
    usersAre([]);
    await list();
    expect(await screen.findByRole("heading", { name: "No users" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
  });

  test("a failed list call shows the error with Retry, and Retry asks again", async () => {
    const failing = counted("get", "/api/users", () => refuse(errors.internal_error));
    await list();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(screen.queryByRole("table")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(failing.calls).toBe(1);
    // Nobody is invited into a list that cannot be shown.
    expect(screen.queryByRole("button", { name: "Invite user" })).toBeNull();

    const again = usersAre(fixtures.userList);
    await userEvent.click(button("Retry"));
    await table();
    expect(again.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(rowOf(maya.name)).toBeInTheDocument();
    expect(button("Invite user")).toBeInTheDocument();
  });

  test("a gateway that cannot be reached is an error with Retry", async () => {
    override("get", "/api/users", networkFailure);
    await list();
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(button("Retry")).toBeInTheDocument();
  });

  test("while the browser says it is offline the list is asked for: a gateway that cannot be reached is an error with Retry", async () => {
    const app = await renderWithApp(null, { route: "/teams" });
    await screen.findByRole("table", { name: "Teams" });
    theBrowserIsOffline();
    const failing = counted("get", "/api/users", networkFailure);
    await act(async () => {
      await app.router.navigate({ to: "/users" });
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(failing.calls).toBe(1);
    // Still offline: Retry asks again, and the list the gateway gives is shown.
    const again = usersAre(fixtures.userList);
    await userEvent.click(button("Retry"));
    await table();
    expect(again.calls).toBe(1);
  });

  test("at width 390 the rows are cards with their labels and the link", async () => {
    await list({ width: 390 });
    const cards = await screen.findByRole("list", { name: "Users" });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    expect(screen.queryByRole("table")).toBeNull();
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(fixtures.userList.length);
    const [first] = items;
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Name",
      "Email",
      "Role",
      "Status",
      "Last active",
    ]);
    const link = within(first).getByRole("link", { name: maya.name });
    expect(link).toHaveAttribute("href", `/users/${maya.id}`);
    // High enough to touch.
    expect(link.className.split(/\s+/)).toContain("min-h-11");
    expect(within(first).getByText("You")).toBeInTheDocument();
    expect(within(first).getByText("active")).toBeInTheDocument();
    expect(button("Invite user").className.split(/\s+/)).toContain("min-h-11");
  });

  test("a failed list call shows the error under the title of the page: one h1", async () => {
    override("get", "/api/users", () => refuse(errors.internal_error));
    await list();
    await screen.findByRole("alert");
    expectOneH1("Users");
    expectOneMain();
  });

  test("the list call answers 403: not available", async () => {
    forbid("/api/users");
    await list();
    await expectNotAvailable();
    expectOneMain();
    expectOneH1();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByRole("button", { name: "Invite user" })).toBeNull();
  });

  test("the session ends while the list is open", async () => {
    startGateway({ signedIn: true });
    const app = await list();
    await table();
    await expectSessionEndsOnPage(app, {
      path: "/api/users",
      queryKey: queryKeys.users.list(),
      at: "/users",
    });
    expect(shown()).not.toContain(lena.email);
  });
});

describe("screens without a page", () => {
  test("an address that is no page has one main landmark", async () => {
    await renderWithApp(null, { route: "/users/3/keys" });
    expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();
    expectOneMain();
    expectOneH1();
    expect(screen.getByRole("link", { name: "Back to Overview" })).toHaveAttribute("href", "/");
  });

  test("the audit log of a member is not available, with one main landmark", async () => {
    await renderWithApp(null, { route: "/audit", user: fixtures.me.tomas });
    await expectNotAvailable();
    expectOneMain();
    expectOneH1();
  });
});

describe("inviting", () => {
  test("invite shows the link once", async () => {
    const invited = counted("post", "/api/users", () =>
      ok("post", "/api/users", 201, { user: sam, invite_link: fixtures.newInviteLink }),
    );
    const app = await list();
    await table();
    const asked = usersAre(fixtures.userList);
    await openInvite();
    await userEvent.click(screen.getByRole("radio", { name: "Admin" }));
    await fillInvite();

    const dialog = await screen.findByRole("dialog", { name: "Invite link" });
    expect(screen.queryByRole("dialog", { name: "Invite user" })).toBeNull();
    expect(dialog).toHaveTextContent(LINK_DESCRIPTION);
    const field = within(dialog).getByLabelText("Invite link");
    expect(field).toHaveValue(LINK);
    expect(LINK.startsWith(window.location.origin)).toBe(true);
    expect(LINK).toMatch(/^https?:\/\/[^/]+\/accept-invite\?token=uf-inv-/);
    expect(invited.bodies).toEqual([
      { name: "Sam Carter", email: "sam@example.test", role: "admin" },
    ]);
    // While it is shown, the link is in the state of the page only.
    expect(JSON.stringify(app.queryClient.getMutationCache().getAll().map((m) => m.state))).not
      .toContain(TOKEN);
    expectNoSecretBut(app);

    await closeSecret();
    expectNoSecret(app, TOKEN);
    await waitFor(() => {
      expect(asked.calls).toBe(1);
    });
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    expect(toasts().join()).not.toContain(TOKEN);
  });

  /** The link is in the document, and nowhere else. */
  function expectNoSecretBut(app: AppRenderResult): void {
    expect(shown()).toContain(TOKEN);
    expect(JSON.stringify(app.queryClient.getQueryCache().getAll().map((q) => q.state))).not
      .toContain(TOKEN);
    expect(JSON.stringify(app.router.state)).not.toContain(TOKEN);
    expect(href(app)).toBe("/users");
  }

  test("invite errors: an email that is taken", async () => {
    const invited = counted("post", "/api/users", () => refuse(errors.user_exists));
    await list();
    await table();
    const dialog = await openInvite();
    await fillInvite("Maya Again", maya.email);
    const email = screen.getByLabelText("Email");
    await waitFor(() => {
      expect(descriptionOf(email)).toBe(errors.user_exists.body.error.message);
    });
    expect(email).toHaveAttribute("aria-invalid", "true");
    expect(email).toHaveFocus();
    expect(screen.getByLabelText("Name")).not.toHaveAttribute("aria-invalid");
    expect(dialog).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveValue("Maya Again");
    expect(email).toHaveValue(maya.email);
    expect(toasts()).toEqual([]);

    // The next attempt is sent.
    await userEvent.clear(email);
    await userEvent.type(email, "other@example.test");
    expect(email).not.toHaveAttribute("aria-invalid");
    await userEvent.click(button("Create invite link"));
    await waitFor(() => {
      expect(invited.calls).toBe(2);
    });
  });

  test("invite errors: fields that are not valid", async () => {
    override("post", "/api/users", () =>
      refuse(validationFailed({ email: fieldMessages.email, name: fieldMessages.name })),
    );
    await list();
    await table();
    const dialog = await openInvite();
    const name = screen.getByLabelText("Name");
    const email = screen.getByLabelText("Email");
    // This test of the form types for real; the others fill it at once.
    await userEvent.type(name, "x");
    await userEvent.type(email, "not-an-email");
    await userEvent.click(button("Create invite link"));
    await waitFor(() => {
      expect(descriptionOf(name)).toBe(fieldMessages.name);
    });
    expect(descriptionOf(email)).toBe(fieldMessages.email);
    expect(dialog).toBeInTheDocument();
    expect(name).toHaveValue("x");
    expect(email).toHaveValue("not-an-email");
    expect(screen.queryByRole("dialog", { name: "Invite link" })).toBeNull();
  });

  test("the labels of the form name controls, and the group of roles is named once, by its field", async () => {
    await list();
    await table();
    const dialog = await openInvite();
    expect(within(dialog).getByRole("radiogroup", { name: "Role" })).toBeInTheDocument();
    expectLabelsNameControls(dialog);
  });

  test("a dialog that is opened again is empty and shows no old error", async () => {
    override("post", "/api/users", () => refuse(errors.user_exists));
    const app = await list();
    await table();
    await openInvite();
    await fillInvite();
    await screen.findByText(errors.user_exists.body.error.message);
    await userEvent.click(button("Cancel"));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    // The mutation was reset, and nothing observes it: it is gone.
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    await waitFor(() => {
      expect(button("Invite user")).toHaveFocus();
    });
    await openInvite();
    expect(screen.getByLabelText("Name")).toHaveValue("");
    expect(screen.getByLabelText("Email")).toHaveValue("");
    expect(screen.getByRole("radio", { name: "Member" })).toBeChecked();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("the session ends while the invite link is shown", async () => {
    startGateway({ signedIn: true });
    const app = await list();
    await table();
    await openInvite();
    await fillInvite();
    await screen.findByRole("dialog", { name: "Invite link" });
    expect(shown()).toContain(TOKEN);

    await aCallFindsTheSessionEnded("/api/teams");

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expectNoSecret(app, TOKEN);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fusers");
    });
    expectNoSecret(app, TOKEN);
  });

  test("an invite whose answer came for a session that is over shows nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("post", "/api/users", async () => {
      await door.opened;
      return ok("post", "/api/users", 201, { user: sam, invite_link: fixtures.newInviteLink });
    });
    const app = await list();
    await table();
    await openInvite();
    await fillInvite();
    await aCallFindsTheSessionEnded("/api/teams");
    door.open();
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expectNoSecret(app, TOKEN);
  });
});

describe("an invite link that cannot be used", () => {
  const UNUSABLE = "The gateway returned an invite link that cannot be used.";
  const paths = [
    `//other.example.test/accept-invite?token=${TOKEN}`,
    `https://other.example.test/accept-invite?token=${TOKEN}`,
    `accept-invite?token=${TOKEN}`,
    `/\\other.example.test/accept-invite?token=${TOKEN}`,
    "",
  ];

  test.each(paths)("the invite dialog says so for %j, and shows no link", async (path) => {
    override("post", "/api/users", () =>
      ok("post", "/api/users", 201, { user: sam, invite_link: path }),
    );
    const app = await list();
    await table();
    const dialog = await openInvite();
    await fillInvite();
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(UNUSABLE);
    expect(screen.queryByRole("dialog", { name: "Invite link" })).toBeNull();
    expect(screen.getByLabelText("Name")).toHaveValue("Sam Carter");
    expect(toasts()).toEqual([]);
    expectNoSecret(app, TOKEN);
    expect(shown()).not.toContain("other.example.test");
  });

  test.each(paths)("the dialog of a new link says so for %j, and shows no link", async (path) => {
    override("post", "/api/users/{id}/invite", () =>
      ok("post", "/api/users/{id}/invite", 201, { invite_link: path }),
    );
    const app = await detail(sam);
    const dialog = await ask("New invite link", "Create a new invite link?");
    await confirm(dialog, "Create link");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(UNUSABLE);
    expect(screen.queryByRole("dialog", { name: "Invite link" })).toBeNull();
    expectNoSecret(app, TOKEN);
    expect(shown()).not.toContain("other.example.test");
  });

  test.each(paths)("for %j the console refuses itself: no answer of the gateway is made up", (path) => {
    let thrown: unknown;
    try {
      inviteUrl(path);
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(ConsoleRefusal);
    expect(thrown).not.toBeInstanceOf(ApiError);
    expect(thrown).toMatchObject({ message: UNUSABLE });
    expect(thrown).not.toHaveProperty("status");
    expect(thrown).not.toHaveProperty("code");
  });

  test("a usable path keeps its query string", async () => {
    await list();
    await table();
    await openInvite();
    await fillInvite();
    const dialog = await screen.findByRole("dialog", { name: "Invite link" });
    expect(within(dialog).getByLabelText("Invite link")).toHaveValue(
      `${window.location.origin}/accept-invite?token=${TOKEN}`,
    );
  });
});

describe("while a request runs", () => {
  const HOLDS =
    "two submits at once are one request, it stays while the request runs, and after a refusal it can be left";

  test("the invite dialog: the button is disabled and says so", async () => {
    const door = gate();
    const invited = counted("post", "/api/users", async () => {
      await door.opened;
      return ok("post", "/api/users", 201, { user: sam, invite_link: fixtures.newInviteLink });
    });
    await list();
    await table();
    await openInvite();
    expect(button("Create invite link")).toBeEnabled();
    await fillInvite();
    const running = await screen.findByRole("button", { name: "Creating the link" });
    expect(running).toBeDisabled();
    expect(running).toHaveAttribute("type", "submit");
    expect(screen.queryByRole("button", { name: "Create invite link" })).toBeNull();
    // Enter in a field sends nothing a second time.
    await userEvent.type(screen.getByLabelText("Name"), "{Enter}");
    expect(invited.calls).toBe(1);
    act(() => {
      door.open();
    });
    expect(await screen.findByRole("dialog", { name: "Invite link" })).toBeInTheDocument();
    expect(invited.calls).toBe(1);
  });

  test(`the invite dialog: ${HOLDS}`, async () => {
    const request = held("post", "/api/users");
    await list();
    await table();
    const dialog = await openInvite();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, "Sam Carter");
    await userEvent.type(within(dialog).getByLabelText("Email"), "sam@example.test");
    await expectOneRequestWhileTheDialogStays(dialog, name, "Creating the link", request);
    expect(request.bodies).toEqual([
      { name: "Sam Carter", email: "sam@example.test", role: "member" },
    ]);
    // Nothing was made, so there is no link to show.
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(toasts()).toEqual([]);
  });

  test("the invite dialog: Enter and Escape pressed with no wait between them leave it open; one request, and the link is shown", async () => {
    const door = gate();
    const invited = counted("post", "/api/users", async () => {
      await door.opened;
      return ok("post", "/api/users", 201, { user: sam, invite_link: fixtures.newInviteLink });
    });
    await list();
    await table();
    const dialog = await openInvite();
    await userEvent.click(within(dialog).getByLabelText("Name"));
    await userEvent.paste("Sam Carter");
    const email = within(dialog).getByLabelText("Email");
    await userEvent.click(email);
    await userEvent.paste("sam@example.test");
    await userEvent.type(email, "{Enter}{Escape}", { delay: null });
    await settle();
    expect(screen.queryByRole("dialog")).toBe(dialog);
    expect(await within(dialog).findByRole("button", { name: "Creating the link" })).toBeDisabled();
    expect(invited.calls).toBe(1);

    act(() => {
      door.open();
    });
    const secret = await screen.findByRole("dialog", { name: "Invite link" });
    expect(within(secret).getByLabelText("Invite link")).toHaveValue(LINK);
    expect(invited.calls).toBe(1);
  });

  test("the invite dialog: a submit and Cancel in one tick leave it open; the refusal is said in it; one request", async () => {
    const request = held("post", "/api/users");
    await list();
    await table();
    const dialog = await openInvite();
    await userEvent.click(within(dialog).getByLabelText("Name"));
    await userEvent.paste("Sam Carter");
    await userEvent.click(within(dialog).getByLabelText("Email"));
    await userEvent.paste("sam@example.test");
    sendAndLeaveAtOnce(dialog, "Cancel");
    await settle();
    expect(screen.queryByRole("dialog")).toBe(dialog);
    expect(await within(dialog).findByRole("button", { name: "Creating the link" })).toBeDisabled();
    expect(request.calls).toBe(1);

    request.answer();
    expect(await within(dialog).findByText(errors.forbidden.body.error.message)).toBeInTheDocument();
    await expectTheDialogCanBeLeft(dialog);
    expect(request.calls).toBe(1);
    expect(toasts()).toEqual([]);
  });

  test(`the name dialog: ${HOLDS}`, async () => {
    const request = held("patch", "/api/users/{id}");
    await detail(lena);
    await userEvent.click(await screen.findByRole("button", { name: "Edit name" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit name" });
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, " K");
    await expectOneRequestWhileTheDialogStays(dialog, name, "Saving", request);
    expect(request.bodies).toEqual([{ name: `${lena.name} K` }]);
    expect(toasts()).toEqual([]);
  });

  test("the name dialog: the button is disabled and says so", async () => {
    const door = gate();
    const patches = counted("patch", "/api/users/{id}", async () => {
      await door.opened;
      return ok("patch", "/api/users/{id}", 200, { ...lena, name: "Lena K" });
    });
    await detail(lena);
    await userEvent.click(await screen.findByRole("button", { name: "Edit name" }));
    const name = screen.getByLabelText("Name");
    await userEvent.type(name, " K");
    await userEvent.click(button("Save"));
    const running = await screen.findByRole("button", { name: "Saving" });
    expect(running).toBeDisabled();
    await userEvent.type(name, "{Enter}");
    expect(patches.calls).toBe(1);
    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(patches.calls).toBe(1);
  });

  test("a confirm dialog: both buttons are disabled", async () => {
    const door = gate();
    const patches = counted("patch", "/api/users/{id}", async () => {
      await door.opened;
      return ok("patch", "/api/users/{id}", 200, { ...lena, status: "disabled" });
    });
    await detail(lena);
    const dialog = await ask("Disable", "Disable this user?");
    await confirm(dialog, "Disable");
    await waitFor(() => {
      expect(within(dialog).getByRole("button", { name: "Disable" })).toBeDisabled();
    });
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
    await userEvent.keyboard("{Escape}");
    expect(dialog).toBeInTheDocument();
    expect(patches.calls).toBe(1);
    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
  });
});

describe("the list after a change", () => {
  /** A gateway whose list and whose user are the same people. */
  function keepsAll(start: fixtures.User) {
    const state = keeps(start);
    const listed = counted("get", "/api/users", () =>
      ok("get", "/api/users", 200, {
        users: fixtures.userList.map((user) => (user.id === start.id ? state.user : user)),
      }),
    );
    return { state, listed };
  }

  // The list is left and mounted again. Its data stays fresh, so that it is
  // asked for again only because the change marked it as stale.
  async function fromTheListTo(user: fixtures.User): Promise<AppRenderResult> {
    const app = await list({ queryClient: clientThatKeepsDataFresh() });
    await table();
    await userEvent.click(screen.getByRole("link", { name: user.name }));
    await screen.findByRole("heading", { level: 1, name: user.name });
    return app;
  }

  async function backToTheList(): Promise<void> {
    await userEvent.click(screen.getByRole("link", { name: "Back to users" }));
    await table();
  }

  test("a role change shows in the list", async () => {
    const { listed } = keepsAll(lena);
    await fromTheListTo(lena);
    expect(listed.calls).toBe(1);
    const dialog = await ask("Make admin", "Make this user an admin?");
    await confirm(dialog, "Make admin");
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    await backToTheList();
    await waitFor(() => {
      expect(rowOf(lena.name)).toHaveTextContent("Admin");
    });
    expect(listed.calls).toBe(2);
  });

  test("a status change shows in the list", async () => {
    const { listed } = keepsAll(lena);
    await fromTheListTo(lena);
    const dialog = await ask("Disable", "Disable this user?");
    await confirm(dialog, "Disable");
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    await backToTheList();
    await waitFor(() => {
      expect(within(rowOf(lena.name)).getByText("disabled")).toBeInTheDocument();
    });
    expect(listed.calls).toBe(2);
  });

  test("a new name shows in the list", async () => {
    const { listed } = keepsAll(lena);
    await fromTheListTo(lena);
    await userEvent.click(button("Edit name"));
    const name = screen.getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Lena K");
    await userEvent.click(button("Save"));
    await screen.findByRole("heading", { level: 1, name: "Lena K" });
    await backToTheList();
    expect(await screen.findByRole("link", { name: "Lena K" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: lena.name })).toBeNull();
    expect(listed.calls).toBe(2);
  });
});

describe("the page of a user", () => {
  test("it shows the fields of the user", async () => {
    await detail(dana);
    expect(await screen.findByRole("heading", { level: 1, name: dana.name })).toBeInTheDocument();
    const details = screen.getByLabelText("Details");
    expect([...details.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Email",
      "Role",
      "Status",
      "Created",
      "Last active",
    ]);
    expect(details).toHaveTextContent(dana.email);
    expect(within(details).getByText("Member")).toHaveAttribute("data-variant", "outline");
    expect(within(details).getByText("disabled")).toHaveAttribute("data-variant", "outline");
    expect(details.querySelectorAll("time")).toHaveLength(2);
    expect(screen.getByRole("link", { name: "Back to users" })).toHaveAttribute("href", "/users");
    expect(screen.queryByText("You")).toBeNull();
    expectOneMain();
    expectOneH1();
  });

  test("the own page is marked", async () => {
    await detail(maya);
    await screen.findByRole("heading", { level: 1, name: maya.name });
    expect(screen.getByText("You")).toBeInTheDocument();
  });

  test("a status and a role the console does not know are shown as they are", async () => {
    override("get", "/api/users/{id}", () =>
      // What a later gateway may answer: the description of the API has neither value.
      HttpResponse.json({ ...lena, status: "locked", role: "auditor" }),
    );
    await detail(lena);
    const details = await screen.findByLabelText("Details");
    expect(within(details).getByText("locked")).toHaveAttribute("data-variant", "outline");
    expect(within(details).getByText("auditor")).toHaveAttribute("data-variant", "outline");
  });

  test("detail controls by role: an admin sees all controls", async () => {
    await detail(lena);
    await screen.findByRole("heading", { level: 1, name: lena.name });
    expect(actions()).toEqual(["Edit name", "Make admin", "Disable", "Delete"]);
  });

  test("detail controls by role: a member on their own page sees only name edit", async () => {
    await detail(tomas, { user: fixtures.me.tomas });
    await screen.findByRole("heading", { level: 1, name: tomas.name });
    expect(actions()).toEqual(["Edit name"]);
  });

  test("detail controls by role: a lead on a team member's page sees no controls", async () => {
    await detail(lena, { user: fixtures.me.arjun });
    await screen.findByRole("heading", { level: 1, name: lena.name });
    expect(screen.getByLabelText("Details")).toHaveTextContent(lena.email);
    expect(actions()).toEqual([]);
    expect(within(screen.getByRole("main")).queryAllByRole("button")).toEqual([]);
  });

  test("an admin and a disabled user: Enable, and Make member for an admin", async () => {
    await detail(dana);
    await screen.findByRole("heading", { level: 1, name: dana.name });
    expect(actions()).toEqual(["Edit name", "Make admin", "Enable", "Delete"]);
  });

  test("resend invite only when invited: the button New invite link", async () => {
    const first = await detail(sam);
    await screen.findByRole("heading", { level: 1, name: sam.name });
    expect(actions()).toEqual(["Edit name", "Make admin", "Disable", "New invite link", "Delete"]);
    first.unmount();
    for (const user of [lena, dana]) {
      const app = await detail(user);
      await screen.findByRole("heading", { level: 1, name: user.name });
      expect(screen.queryByRole("button", { name: "New invite link" })).toBeNull();
      app.unmount();
    }
  });

  test("loading shows a skeleton", async () => {
    const door = gate();
    override("get", "/api/users/{id}", async () => {
      await door.opened;
      return ok("get", "/api/users/{id}", 200, lena);
    });
    await detail(lena);
    const loading = screen.getByRole("status", { name: "Loading the user" });
    expect(loading).toHaveAttribute("aria-busy", "true");
    expect(loading.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    act(() => {
      door.open();
    });
    expect(await screen.findByRole("heading", { level: 1, name: lena.name })).toBeInTheDocument();
    expect(screen.queryByRole("status", { name: "Loading the user" })).toBeNull();
  });

  test("hidden user is not found", async () => {
    const asked = counted("get", "/api/users/{id}", () => refuse(errors.not_found));
    await detail(tomas, { user: fixtures.me.lena });
    expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();
    expect(asked.calls).toBe(1);
    expectOneMain();
    expectOneH1();
    // It is inside the shell, which is still there.
    expect(screen.getByRole("navigation", { name: "Main" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(tomas.email)).toBeNull();
    expect(actions()).toEqual([]);
  });

  test.each(["0", "-1", "1.5", "abc", "01", "1e3", "%20", "99999999999999999999"])(
    "the id %s is not found, and the API is not asked",
    async (id) => {
      const asked = counted("get", "/api/users/{id}", () => refuse(errors.not_found));
      await renderWithApp(null, { route: `/users/${id}` });
      expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();
      await settle();
      expect(asked.calls).toBe(0);
      expectOneMain();
      expectOneH1();
    },
  );

  test("a failed call shows the error with Retry, and Retry asks again", async () => {
    override("get", "/api/users/{id}", () => refuse(errors.internal_error));
    await detail(lena);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    const again = counted("get", "/api/users/{id}", () => ok("get", "/api/users/{id}", 200, lena));
    await userEvent.click(button("Retry"));
    expect(await screen.findByRole("heading", { level: 1, name: lena.name })).toBeInTheDocument();
    expect(again.calls).toBe(1);
  });

  test("a failed call shows the error under a heading that says what the page is: one h1", async () => {
    override("get", "/api/users/{id}", () => refuse(errors.internal_error));
    await detail(lena);
    await screen.findByRole("alert");
    expectOneH1("User");
    expectOneMain();
    // Loaded, the page is called by the name of the user.
    override("get", "/api/users/{id}", () => ok("get", "/api/users/{id}", 200, lena));
    await userEvent.click(button("Retry"));
    expect(await screen.findByRole("heading", { level: 1, name: lena.name })).toBeInTheDocument();
    expectOneH1(lena.name);
  });

  test("the call answers 403: not available", async () => {
    forbid("/api/users/{id}");
    await detail(lena);
    await expectNotAvailable();
    expectOneMain();
    expectOneH1();
  });

  /** Lena's page is open; then she is deleted, and her page asks for her again. */
  async function lenaIsGone(): Promise<{ app: AppRenderResult; asked: { calls: number } }> {
    const app = await detail(lena);
    await screen.findByRole("heading", { level: 1, name: lena.name });
    const asked = counted("get", "/api/users/{id}", () => refuse(errors.not_found));
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: queryKeys.users.detail(lena.id) });
    });
    return { app, asked };
  }

  test("a user who is gone when they are asked for again is not found, though they were shown", async () => {
    const { asked } = await lenaIsGone();
    expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();
    expect(asked.calls).toBe(1);
    expectOneMain();
    expectOneH1();
    expect(screen.queryByRole("heading", { name: lena.name })).toBeNull();
    expect(shown()).not.toContain(lena.email);
    expect(actions()).toEqual([]);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("nothing of a user who is gone when they are asked for again is kept", async () => {
    const app = await detail(lena);
    await screen.findByRole("heading", { level: 1, name: lena.name });
    const key = queryKeys.users.detail(lena.id);
    expect(app.queryClient.getQueryData(key)).toEqual(lena);
    expect(cached(app.queryClient)).toContain(lena.email);
    override("get", "/api/users/{id}", () => refuse(errors.not_found));
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: key });
    });
    await waitFor(() => {
      expect(app.queryClient.getQueryState(key)?.error).toMatchObject({ status: 404 });
    });
    expect(app.queryClient.getQueryData(key)).toBeUndefined();
    expect(cached(app.queryClient)).not.toContain(lena.email);
  });

  test.each([
    [
      "an error of the gateway",
      () => refuse(errors.internal_error),
      errors.internal_error.body.error.message,
    ],
    ["a gateway that cannot be reached", networkFailure, "Could not reach the gateway."],
  ])(
    "a user who was gone does not come back when asking again fails with %s",
    async (_, answer, message) => {
      const { app } = await lenaIsGone();
      expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();

      // Asked for again, as the app does when the window gets the focus.
      const way = watchTheDocument(ofLena);
      const again = counted("get", "/api/users/{id}", answer);
      await act(async () => {
        await app.queryClient.invalidateQueries({ queryKey: queryKeys.users.detail(lena.id) });
      });
      expect(again.calls).toBe(1);
      await settle();
      // The failure is what shows, with Retry, and not what was loaded once.
      expect(way.seen()).toEqual([]);
      expect(await screen.findByRole("alert")).toHaveTextContent(message);
      expect(button("Retry")).toBeInTheDocument();
      expect(screen.queryByRole("heading", { name: lena.name })).toBeNull();
      expect(shown()).not.toContain(lena.email);
      expect(actions()).toEqual([]);
      expectOneMain();
      expectOneH1();
    },
  );

  test("a user who was gone does not come back on a return to their page while the gateway cannot be reached", async () => {
    usersAre(fixtures.userList.filter((user) => user.id !== lena.id));
    const { app } = await lenaIsGone();
    expect(await screen.findByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();

    // To the list, and back while nothing answers.
    await userEvent.click(screen.getByRole("link", { name: "Users" }));
    await table();
    expect(screen.queryByRole("link", { name: lena.name })).toBeNull();
    const again = counted("get", "/api/users/{id}", networkFailure);
    const way = watchTheDocument(ofLena);
    await act(async () => {
      await app.router.navigate({ to: `/users/${lena.id}` });
    });
    await waitFor(() => {
      expect(again.calls).toBe(1);
    });
    await settle();
    expect(href(app)).toBe(`/users/${lena.id}`);
    expect(way.seen()).toEqual([]);
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(screen.queryByRole("heading", { name: lena.name })).toBeNull();
    expect(shown()).not.toContain(lena.email);
    expect(actions()).toEqual([]);
  });

  test("asking again that fails with something else keeps what is shown", async () => {
    const app = await detail(lena);
    await screen.findByRole("heading", { level: 1, name: lena.name });
    const again = counted("get", "/api/users/{id}", () => refuse(errors.internal_error));
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: queryKeys.users.detail(lena.id) });
    });
    expect(again.calls).toBe(1);
    await settle();
    expect(screen.getByRole("heading", { level: 1, name: lena.name })).toBeInTheDocument();
    expect(screen.getByLabelText("Details")).toHaveTextContent(lena.email);
    expect(actions()).toEqual(["Edit name", "Make admin", "Disable", "Delete"]);
    expect(screen.queryByRole("heading", { name: NOT_FOUND })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("the watch of the user sees what shows of them", async () => {
    const way = watchTheDocument(ofLena);
    await detail(lena);
    await screen.findByRole("heading", { level: 1, name: lena.name });
    expect(way.seen()).toEqual(["the controls", "the email", "the heading"]);
  });

  test("the session ends while the page of a user is open", async () => {
    startGateway({ signedIn: true });
    const app = await detail(lena);
    await screen.findByRole("heading", { level: 1, name: lena.name });
    await expectSessionEndsOnPage(app, {
      path: "/api/users/{id}",
      queryKey: queryKeys.users.detail(lena.id),
      at: `/users/${lena.id}`,
    });
    expect(shown()).not.toContain(lena.email);
  });

  test("at width 390 the controls are high enough to touch", async () => {
    await detail(sam, { width: 390 });
    await screen.findByRole("heading", { level: 1, name: sam.name });
    const group = screen.getByRole("group", { name: "Actions" });
    expect(group.className.split(/\s+/)).toContain("flex-wrap");
    for (const control of within(group).getAllByRole("button")) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
    expect(screen.getByLabelText("Details")).toHaveTextContent(sam.email);
  });
});

/** A gateway that keeps the user and changes them as it is told. */
function keeps(start: fixtures.User) {
  const state = { user: start, patches: [] as unknown[], reads: 0 };
  override("get", "/api/users/{id}", () => {
    state.reads += 1;
    return ok("get", "/api/users/{id}", 200, state.user);
  });
  override("patch", "/api/users/{id}", async ({ request }) => {
    const body: unknown = await request.json();
    state.patches.push(body);
    const change = typeof body === "object" && body !== null ? body : {};
    const role: unknown = Reflect.get(change, "role");
    const status: unknown = Reflect.get(change, "status");
    const name: unknown = Reflect.get(change, "name");
    state.user = {
      ...state.user,
      ...(role === "admin" || role === "member" ? { role } : {}),
      ...(status === "active" || status === "disabled" ? { status } : {}),
      ...(typeof name === "string" ? { name: name.trim() } : {}),
    };
    return ok("patch", "/api/users/{id}", 200, state.user);
  });
  return state;
}

describe("changing a user", () => {
  test("role change confirms and reports", async () => {
    const gateway = keeps(lena);
    const app = await detail(lena);
    const dialog = await ask("Make admin", "Make this user an admin?");
    expect(dialog).toHaveTextContent(ROLE);
    expect(gateway.patches).toEqual([]);

    await confirm(dialog, "Make admin");
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(gateway.patches).toEqual([{ role: "admin" }]);
    const details = screen.getByLabelText("Details");
    await waitFor(() => {
      expect(within(details).getByText("Admin")).toBeInTheDocument();
    });
    expect(within(details).queryByText("Member")).toBeNull();
    await waitFor(() => {
      expect(toasts()).toEqual(["Role changed."]);
    });
    expect(actions()).toContain("Make member");
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll().map((m) => m.state.status)).not.toContain(
        "success",
      );
    });
  });

  test("an admin is made a member with the same consequence", async () => {
    const gateway = keeps(maya);
    await detail(maya, { user: { ...fixtures.me.maya, user: { ...arjun, role: "admin" } } });
    const dialog = await ask("Make member", "Make this user a member?");
    expect(dialog).toHaveTextContent(ROLE);
    await confirm(dialog, "Make member");
    await waitFor(() => {
      expect(gateway.patches).toEqual([{ role: "member" }]);
    });
  });

  test("cancel changes nothing", async () => {
    const gateway = keeps(lena);
    await detail(lena);
    const dialog = await ask("Make admin", "Make this user an admin?");
    await confirm(dialog, "Cancel");
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(gateway.patches).toEqual([]);
    expect(toasts()).toEqual([]);
  });

  test("last admin refusal stays in the dialog", async () => {
    override("patch", "/api/users/{id}", () => refuse(errors.last_admin));
    // Another admin looks at the page; the own page has its own texts and tests.
    await detail(maya, { user: { ...fixtures.me.maya, user: { ...arjun, role: "admin" } } });
    const dialog = await ask("Make member", "Make this user a member?");
    await confirm(dialog, "Make member");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.last_admin.body.error.message,
    );
    expect(dialog).toBeInTheDocument();
    expect(dialog).toHaveTextContent(ROLE);
    expect(toasts()).toEqual([]);
    expect(within(screen.getByLabelText("Details")).getByText("Admin")).toBeInTheDocument();

    // Closed and opened again, the dialog does not show the old refusal.
    await confirm(dialog, "Cancel");
    const again = await ask("Make member", "Make this user a member?");
    expect(within(again).queryByRole("alert")).toBeNull();
  });

  test("the last admin cannot be disabled or deleted: the refusal stays in its dialog", async () => {
    override("patch", "/api/users/{id}", () => refuse(errors.last_admin));
    override("delete", "/api/users/{id}", () => refuse(errors.last_admin));
    const app = await detail(maya, {
      user: { ...fixtures.me.maya, user: { ...arjun, role: "admin" } },
    });
    const disable = await ask("Disable", "Disable this user?");
    await confirm(disable, "Disable");
    expect(await within(disable).findByRole("alert")).toHaveTextContent(
      errors.last_admin.body.error.message,
    );
    await confirm(disable, "Cancel");

    const remove = await ask("Delete", "Delete this user?");
    await confirm(remove, "Delete");
    expect(await within(remove).findByRole("alert")).toHaveTextContent(
      errors.last_admin.body.error.message,
    );
    expect(href(app)).toBe(`/users/${maya.id}`);
    expect(toasts()).toEqual([]);
  });

  test("disable states the consequence and reports", async () => {
    const gateway = keeps(lena);
    await detail(lena);
    const dialog = await ask("Disable", "Disable this user?");
    expect(dialog).toHaveTextContent(DISABLE);
    await confirm(dialog, "Disable");
    const details = screen.getByLabelText("Details");
    await waitFor(() => {
      expect(within(details).getByText("disabled")).toBeInTheDocument();
    });
    expect(gateway.patches).toEqual([{ status: "disabled" }]);
    await waitFor(() => {
      expect(toasts()).toEqual(["User disabled."]);
    });
    expect(actions()).toContain("Enable");
    expect(actions()).not.toContain("Disable");
  });

  test("enable reports", async () => {
    const gateway = keeps(dana);
    await detail(dana);
    const dialog = await ask("Enable", "Enable this user?");
    expect(dialog).toHaveTextContent("They can sign in again, and their virtual keys work again.");
    await confirm(dialog, "Enable");
    const details = screen.getByLabelText("Details");
    await waitFor(() => {
      expect(within(details).getByText("active")).toBeInTheDocument();
    });
    expect(gateway.patches).toEqual([{ status: "active" }]);
    await waitFor(() => {
      expect(toasts()).toEqual(["User enabled."]);
    });
  });

  test("a user without a password cannot be enabled: the refusal stays in the dialog", async () => {
    override("patch", "/api/users/{id}", () => refuse(errors.no_password));
    await detail(dana);
    const dialog = await ask("Enable", "Enable this user?");
    await confirm(dialog, "Enable");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.no_password.body.error.message,
    );
    expect(toasts()).toEqual([]);
    expect(within(screen.getByLabelText("Details")).getByText("disabled")).toBeInTheDocument();
  });

  test("an admin edits the name of a user", async () => {
    const gateway = keeps(lena);
    await detail(lena);
    await userEvent.click(await screen.findByRole("button", { name: "Edit name" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit name" });
    const name = within(dialog).getByLabelText("Name");
    expect(name).toHaveValue(lena.name);
    await userEvent.clear(name);
    await userEvent.type(name, "Lena K");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Lena K" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(gateway.patches).toEqual([{ name: "Lena K" }]);
    await waitFor(() => {
      expect(toasts()).toEqual(["Name changed."]);
    });
  });

  test("a member edits their own name, and the app asks again who they are", async () => {
    const gateway = keeps(tomas);
    await detail(tomas, { user: fixtures.me.tomas });
    const me = counted("get", "/api/auth/me", () =>
      ok("get", "/api/auth/me", 200, { ...fixtures.me.tomas, user: gateway.user }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "Edit name" }));
    const name = screen.getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Tom Novak");
    await userEvent.click(button("Save"));
    expect(await screen.findByRole("heading", { level: 1, name: "Tom Novak" })).toBeInTheDocument();
    expect(gateway.patches).toEqual([{ name: "Tom Novak" }]);
    await waitFor(() => {
      expect(me.calls).toBe(1);
    });
    expect(actions()).toEqual(["Edit name"]);
  });

  test("a name that is not valid shows on its field, and the dialog keeps the value", async () => {
    override("patch", "/api/users/{id}", () =>
      refuse(validationFailed({ name: fieldMessages.name })),
    );
    await detail(lena);
    await userEvent.click(await screen.findByRole("button", { name: "Edit name" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit name" });
    const name = within(dialog).getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(descriptionOf(name)).toBe(fieldMessages.name);
    });
    expect(name).toHaveValue("");
    expect(dialog).toBeInTheDocument();
    expect(toasts()).toEqual([]);

    // Closed and opened again, the dialog has the name of the user and no error.
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    await userEvent.click(button("Edit name"));
    expect(screen.getByLabelText("Name")).toHaveValue(lena.name);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("a change whose answer came for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("patch", "/api/users/{id}", async () => {
      await door.opened;
      return ok("patch", "/api/users/{id}", 200, { ...lena, role: "admin" });
    });
    const app = await detail(lena);
    const dialog = await ask("Make admin", "Make this user an admin?");
    await confirm(dialog, "Make admin");
    await aCallFindsTheSessionEnded("/api/teams");
    door.open();
    await settle();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent(`/users/${lena.id}`)}`);
  });
});

describe("changing the own account", () => {
  const OWN_ROLE = "You will lose your admin rights and be signed out.";
  const OWN_DISABLE =
    "You will be signed out and cannot sign in again until another admin enables your account.";
  const CHANGED = "You changed your own account. Sign in again.";

  /** A gateway that ends the sessions of the user when their role or status changes. */
  function endsTheSessionOnChange() {
    const gateway = startGateway({ signedIn: true });
    const patches = counted("patch", "/api/users/{id}", () => {
      gateway.signedIn = false;
      for (const path of ["/api/users", "/api/teams", "/api/keys", "/api/audit"] as const) {
        override("get", path, unauthenticated);
      }
      override("get", "/api/users/{id}", unauthenticated);
      return ok("patch", "/api/users/{id}", 200, { ...maya, role: "member" });
    });
    return { gateway, patches };
  }

  async function expectSignedOutByOwnChange(app: AppRenderResult): Promise<void> {
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(screen.getByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(CHANGED);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    // The answers of what was asked before the session ended change nothing.
    await settle(60);
    expect(href(app)).toBe("/sign-in");
    expect(screen.getByRole("status")).toHaveTextContent(CHANGED);
    expect(screen.queryByText(SESSION_ENDED)).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(shown()).not.toContain(maya.email);
  }

  test("an admin who makes themselves a member is told so, and signed out", async () => {
    const { patches } = endsTheSessionOnChange();
    const app = await detail(maya);
    const dialog = await ask("Make member", "Make this user a member?");
    expect(dialog).toHaveTextContent(OWN_ROLE);
    expect(dialog).not.toHaveTextContent(ROLE);
    expect(patches.calls).toBe(0);
    await confirm(dialog, "Make member");
    await expectSignedOutByOwnChange(app);
    expect(patches.bodies).toEqual([{ role: "member" }]);
  });

  test("an admin who disables themselves is told so, and signed out", async () => {
    const { patches } = endsTheSessionOnChange();
    const app = await detail(maya);
    const dialog = await ask("Disable", "Disable this user?");
    expect(dialog).toHaveTextContent(OWN_DISABLE);
    expect(dialog).not.toHaveTextContent(DISABLE);
    await confirm(dialog, "Disable");
    await expectSignedOutByOwnChange(app);
    expect(patches.bodies).toEqual([{ status: "disabled" }]);
  });

  test("the texts for another user are the ones of the brief", async () => {
    await detail(lena);
    const role = await ask("Make admin", "Make this user an admin?");
    expect(role).toHaveTextContent(ROLE);
    await confirm(role, "Cancel");
    const disable = await ask("Disable", "Disable this user?");
    expect(disable).toHaveTextContent(DISABLE);
    expect(disable).not.toHaveTextContent(OWN_DISABLE);
  });

  test.each([
    ["Make member", "Make this user a member?", OWN_ROLE],
    ["Disable", "Disable this user?", OWN_DISABLE],
  ])("last admin on oneself stays in the dialog: %s", async (action, title, text) => {
    startGateway({ signedIn: true });
    const patches = counted("patch", "/api/users/{id}", () => refuse(errors.last_admin));
    const app = await detail(maya);
    const dialog = await ask(action, title);
    await confirm(dialog, action);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.last_admin.body.error.message,
    );
    expect(patches.calls).toBe(1);
    expect(dialog).toHaveTextContent(text);
    await settle();
    // Nothing changed: the same page, the same session, the same user.
    expect(href(app)).toBe(`/users/${maya.id}`);
    expect(toasts()).toEqual([]);
    const details = screen.getByLabelText("Details");
    expect(within(details).getByText("Admin")).toBeInTheDocument();
    expect(within(details).getByText("active")).toBeInTheDocument();
    expect(app.queryClient.getQueryData(queryKeys.me())).toBeDefined();
    await confirm(dialog, "Cancel");
    expect(actions()).toEqual(["Edit name", "Make member", "Disable"]);
  });

  test("a change of the own name signs nobody out", async () => {
    startGateway({ signedIn: true });
    keeps(maya);
    const app = await detail(maya);
    await userEvent.click(await screen.findByRole("button", { name: "Edit name" }));
    const name = screen.getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Maya O");
    await userEvent.click(button("Save"));
    expect(await screen.findByRole("heading", { level: 1, name: "Maya O" })).toBeInTheDocument();
    await waitFor(() => {
      expect(toasts()).toEqual(["Name changed."]);
    });
    expect(href(app)).toBe(`/users/${maya.id}`);
  });
});

describe("a new invite", () => {
  test("the new link is shown once", async () => {
    const app = await detail(sam);
    const dialog = await ask("New invite link", "Create a new invite link?");
    expect(within(dialog).getByText("Earlier links stop working.")).toBeInTheDocument();
    await confirm(dialog, "Create link");
    const secret = await screen.findByRole("dialog", { name: "Invite link" });
    expect(secret).toHaveTextContent(LINK_DESCRIPTION);
    expect(within(secret).getByLabelText("Invite link")).toHaveValue(LINK);
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(JSON.stringify(app.queryClient.getMutationCache().getAll().map((m) => m.state))).not
      .toContain(TOKEN);
    expect(JSON.stringify(app.queryClient.getQueryCache().getAll().map((q) => q.state))).not
      .toContain(TOKEN);
    expect(JSON.stringify(app.router.state)).not.toContain(TOKEN);

    await closeSecret();
    expectNoSecret(app, TOKEN);
    expect(toasts().join()).not.toContain(TOKEN);
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
  });

  test("a user who has accepted cannot get one: the refusal stays in the dialog", async () => {
    override("post", "/api/users/{id}/invite", () => refuse(errors.not_invited));
    await detail(sam);
    const dialog = await ask("New invite link", "Create a new invite link?");
    await confirm(dialog, "Create link");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.not_invited.body.error.message,
    );
    expect(screen.queryByRole("dialog", { name: "Invite link" })).toBeNull();
    expect(toasts()).toEqual([]);
  });

  test("the session ends while the new link is shown", async () => {
    startGateway({ signedIn: true });
    const app = await detail(sam);
    const dialog = await ask("New invite link", "Create a new invite link?");
    await confirm(dialog, "Create link");
    await screen.findByRole("dialog", { name: "Invite link" });
    expect(shown()).toContain(TOKEN);

    await aCallFindsTheSessionEnded("/api/teams");

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expectNoSecret(app, TOKEN);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
  });
});

describe("deleting a user", () => {
  test("delete texts differ by status", async () => {
    for (const [user, text, other] of [
      [lena, DELETE_ACTIVE, DELETE_NOT_ACTIVE],
      [dana, DELETE_NOT_ACTIVE, DELETE_ACTIVE],
      [sam, DELETE_NOT_ACTIVE, DELETE_ACTIVE],
    ] as const) {
      const app = await detail(user);
      const dialog = await ask("Delete", "Delete this user?");
      expect(dialog).toHaveTextContent(text);
      expect(dialog).not.toHaveTextContent(other);
      app.unmount();
    }
  });

  test("delete returns to the list", async () => {
    const removed = counted("delete", "/api/users/{id}", noContent);
    const read = counted("get", "/api/users/{id}", () => refuse(errors.not_found));
    const rest = fixtures.userList.filter((user) => user.id !== lena.id);
    // The list stays fresh: it is asked for again only because the delete marked it as stale.
    const app = await list({ queryClient: clientThatKeepsDataFresh() });
    await table();
    // The page of the user is reached from the list, whose data is then in the cache.
    override("get", "/api/users/{id}", () => ok("get", "/api/users/{id}", 200, lena));
    await userEvent.click(screen.getByRole("link", { name: lena.name }));
    const dialog = await ask("Delete", "Delete this user?");
    const asked = usersAre(rest);
    override("get", "/api/users/{id}", () => {
      read.calls += 1;
      return refuse(errors.not_found);
    });
    // The answer of `me` differs afterwards, so the page renders once more.
    override("get", "/api/auth/me", () =>
      ok("get", "/api/auth/me", 200, {
        ...fixtures.me.maya,
        user: { ...maya, last_active_at: "2026-09-29 10:00:00" },
      }),
    );
    await confirm(dialog, "Delete");

    await waitFor(() => {
      expect(href(app)).toBe("/users");
    });
    await table();
    await waitFor(() => {
      expect(screen.queryByRole("link", { name: lena.name })).toBeNull();
    });
    expect(removed.calls).toBe(1);
    expect(asked.calls).toBe(1);
    expect(screen.getByRole("link", { name: maya.name })).toBeInTheDocument();
    await waitFor(() => {
      expect(toasts()).toEqual(["User deleted."]);
    });
    // The user who is gone was not asked for again, and nothing says "not found".
    await settle();
    expect(read.calls).toBe(0);
    expect(screen.queryByRole("heading", { name: NOT_FOUND })).toBeNull();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  test("the watch of the way sees what shows for a moment", async () => {
    const door = gate();
    override("get", "/api/users/{id}", async () => {
      await door.opened;
      return refuse(errors.not_found);
    });
    const way = watchTheWayFrom("Loading the user");
    await detail(lena);
    expect(screen.getByRole("status", { name: "Loading the user" })).toBeInTheDocument();
    act(() => {
      door.open();
    });
    await screen.findByRole("heading", { name: NOT_FOUND });
    // The skeleton is gone by now: it was seen on the way.
    expect(screen.queryByRole("status", { name: "Loading the user" })).toBeNull();
    expect(way.seen()).toEqual(["Loading the user", NOT_FOUND]);
  });

  test("who deletes a user sees neither the skeleton nor 'not found' on the way to the list", async () => {
    const gateway = startGateway({ signedIn: true });
    const read = counted("get", "/api/users/{id}", () => ok("get", "/api/users/{id}", 200, lena));
    const rest = fixtures.userList.filter((user) => user.id !== lena.id);
    // The way to the list is held open, so that everything that can happen on
    // it does: the answer of `me` arrives while the page of the user is shown.
    const door = gate();
    const history = createMemoryHistory({ initialEntries: [`/users/${lena.id}`] });
    const push = history.push.bind(history);
    const pushed = vi.spyOn(history, "push").mockImplementation((...to) => {
      void door.opened.then(() => {
        push(...to);
      });
    });
    const app = await renderWithApp(null, { history });
    const dialog = await ask("Delete", "Delete this user?");
    // Afterwards the user is gone, and the answer of `me` differs, so that the
    // page renders once more.
    const removed = counted("delete", "/api/users/{id}", () => {
      override("get", "/api/users/{id}", () => {
        read.calls += 1;
        return refuse(errors.not_found);
      });
      override("get", "/api/auth/me", () => {
        gateway.meCalls += 1;
        return ok("get", "/api/auth/me", 200, {
          ...fixtures.me.maya,
          user: { ...maya, last_active_at: "2026-09-29 10:00:00" },
        });
      });
      return noContent();
    });
    usersAre(rest);
    const before = gateway.meCalls;
    expect(read.calls).toBe(1);

    const way = watchTheWayFrom("Loading the user");
    await confirm(dialog, "Delete");
    await waitFor(() => {
      expect(pushed).toHaveBeenCalledTimes(1);
    });
    await waitFor(() => {
      expect(gateway.meCalls).toBe(before + 1);
    });
    await settle(60);
    // Still on the page of the user, which is what it shows.
    expect(way.seen()).toEqual([]);
    expect(href(app)).toBe(`/users/${lena.id}`);
    expect(
      screen.getByRole("heading", { level: 1, name: lena.name, hidden: true }),
    ).toBeInTheDocument();
    expect(read.calls).toBe(1);

    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(href(app)).toBe("/users");
    });
    await table();
    await waitFor(() => {
      expect(toasts()).toEqual(["User deleted."]);
    });
    await settle(60);
    expect(way.seen()).toEqual([]);
    // The user who is gone was not asked for again, and is kept no longer.
    expect(removed.calls).toBe(1);
    expect(read.calls).toBe(1);
    expect(
      app.queryClient.getQueryCache().find({ queryKey: queryKeys.users.detail(lena.id) }),
    ).toBeUndefined();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("link", { name: lena.name })).toBeNull();
  });

  test("who deletes a user sees neither of them when the window gets the focus on the way", async () => {
    startGateway({ signedIn: true });
    const read = counted("get", "/api/users/{id}", () => ok("get", "/api/users/{id}", 200, lena));
    const way = aWayThatIsHeld(`/users/${lena.id}`);
    const app = await renderWithApp(null, { history: way.history });
    const dialog = await ask("Delete", "Delete this user?");
    // Afterwards the user is gone.
    override("delete", "/api/users/{id}", () => {
      override("get", "/api/users/{id}", () => {
        read.calls += 1;
        return refuse(errors.not_found);
      });
      return noContent();
    });
    usersAre(fixtures.userList.filter((user) => user.id !== lena.id));
    expect(read.calls).toBe(1);

    const watch = watchTheWayFrom("Loading the user");
    await confirm(dialog, "Delete");
    await waitFor(() => {
      expect(way.pushes()).toBe(1);
    });
    // The user is deleted, and their page is still shown. The admin comes back to the tab.
    theWindowGetsTheFocus();
    await waitFor(() => {
      expect(read.calls).toBe(2);
    });
    await settle(60);
    // The read answered 404, and the page shows what it showed.
    expect(watch.seen()).toEqual([]);
    expect(href(app)).toBe(`/users/${lena.id}`);
    expect(
      screen.getByRole("heading", { level: 1, name: lena.name, hidden: true }),
    ).toBeInTheDocument();

    way.open();
    await waitFor(() => {
      expect(href(app)).toBe("/users");
    });
    await table();
    await waitFor(() => {
      expect(toasts()).toEqual(["User deleted."]);
    });
    await settle(60);
    expect(watch.seen()).toEqual([]);
    expect(
      app.queryClient.getQueryCache().find({ queryKey: queryKeys.users.detail(lena.id) }),
    ).toBeUndefined();
  });

  test("delete is not offered on the own page", async () => {
    await detail(maya);
    await screen.findByRole("heading", { level: 1, name: maya.name });
    expect(actions()).toEqual(["Edit name", "Make member", "Disable"]);
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  test("the own account cannot be deleted: the refusal stays in the dialog", async () => {
    // The page takes the user shown for another one, and the gateway knows better.
    override("delete", "/api/users/{id}", () => refuse(errors.cannot_delete_self));
    const app = await detail(maya, {
      user: { ...fixtures.me.maya, user: { ...arjun, role: "admin" } },
    });
    const dialog = await ask("Delete", "Delete this user?");
    await confirm(dialog, "Delete");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.cannot_delete_self.body.error.message,
    );
    expect(href(app)).toBe(`/users/${maya.id}`);
    expect(toasts()).toEqual([]);
    // The page still shows the user, and asks for them when it must.
    await confirm(dialog, "Cancel");
    expect(screen.getByRole("heading", { level: 1, name: maya.name })).toBeInTheDocument();
    const again = counted("get", "/api/users/{id}", () => ok("get", "/api/users/{id}", 200, maya));
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: queryKeys.users.detail(maya.id) });
    });
    expect(again.calls).toBe(1);
  });
});
