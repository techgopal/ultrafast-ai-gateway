import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HttpResponse } from "msw";
import { afterEach, beforeAll, describe, expect, onTestFinished, test, vi } from "vitest";
import { queryKeys } from "@/api/queries";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, noContent, ok, override, refuse } from "@/test/handlers";
import {
  aCallFindsTheSessionEnded,
  cached,
  counted,
  descriptionOf,
  expectLabelsNameControls,
  expectNoSecret,
  expectOneMain,
  expectSessionEndsOnPage,
  forgetToasts,
  href,
  installSelect,
  listenToConsole,
  SESSION_ENDED,
  settle,
  shown,
  toasts,
} from "@/test/pages";
import { renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

const { maya, arjun } = fixtures.users;
const { active, neverUsed, revoked, expired } = fixtures.tokens;
const SECRET = fixtures.newTokenSecret;

const PASSWORD_CHANGED =
  "Password changed. Your other sessions and all your access tokens were ended.";
const WRONG_CURRENT = "Current password is incorrect.";
const TOO_MANY = "Too many attempts. Try again in a few minutes.";
const DIFFER = "The passwords do not match.";
const SHOWN_ONCE = "Copy this token now. It is not shown again.";
const REVOKE = "Scripts using this token stop working at once. This cannot be undone.";
const PART_NOT_AVAILABLE = "Not available to your account.";

/** The password the gateway of these tests knows, and the one it is changed to. */
const CURRENT = "current-horse-battery";
const NEXT = "next-staple-correct-7";

beforeAll(installSelect);
afterEach(forgetToasts);

type Options = { user?: fixtures.Me; width?: number };

function page(options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/account", ...options });
}

function main(): HTMLElement {
  return screen.getByRole("main");
}

function part(name: "Profile" | "Password" | "Access tokens"): HTMLElement {
  return within(main()).getByRole("region", { name });
}

/** What the profile says, by its labels. */
function profile(): Map<string, HTMLElement> {
  const found = new Map<string, HTMLElement>();
  for (const label of part("Profile").querySelectorAll("dt")) {
    const value = label.nextElementSibling;
    if (value instanceof HTMLElement) found.set(label.textContent, value);
  }
  return found;
}

function said(label: string): HTMLElement {
  const value = profile().get(label);
  if (value === undefined) throw new Error(`the profile has no ${label}`);
  return value;
}

/** What the sidebar says of who is signed in. */
function shellName(): string {
  const signOut = within(screen.getByRole("navigation", { name: "Main" })).getByRole("button", {
    name: "Sign out",
  });
  return signOut.parentElement?.querySelector(".font-medium")?.textContent ?? "";
}

// ---------------------------------------------------------------- the name

/** A gateway that keeps the name of who is signed in, and says it in `me`. */
function keepsTheName(start: fixtures.Me = fixtures.me.maya) {
  const user: fixtures.User = { ...start.user };
  const state = { user, patches: [] as unknown[], me: 0 };
  override("get", "/api/auth/me", () => {
    state.me += 1;
    return ok("get", "/api/auth/me", 200, { ...start, user: state.user });
  });
  override("patch", "/api/users/{id}", async ({ request, params }) => {
    if (params.id !== String(state.user.id)) return refuse(errors.not_found);
    const body: unknown = await request.json();
    state.patches.push(body);
    const name: unknown = typeof body === "object" && body !== null ? Reflect.get(body, "name") : null;
    if (typeof name === "string") state.user = { ...state.user, name };
    return ok("patch", "/api/users/{id}", 200, state.user);
  });
  return state;
}

async function openName(): Promise<HTMLElement> {
  await userEvent.click(within(part("Profile")).getByRole("button", { name: "Edit name" }));
  return screen.findByRole("dialog", { name: "Edit name" });
}

async function closed(): Promise<void> {
  await waitFor(() => {
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

function overlay(): Element {
  const found = document.querySelector('[data-slot="dialog-overlay"]');
  if (found === null) throw new Error("no overlay");
  return found;
}

// ------------------------------------------------------------ the password

function field(name: "Current password" | "New password" | "Confirm new password"): HTMLElement {
  return within(part("Password")).getByLabelText(name);
}

function fields(): HTMLElement[] {
  return [field("Current password"), field("New password"), field("Confirm new password")];
}

async function fill(current: string, next: string, confirm: string = next): Promise<void> {
  for (const [name, text] of [
    ["Current password", current],
    ["New password", next],
    ["Confirm new password", confirm],
  ] as const) {
    await userEvent.clear(field(name));
    if (text !== "") await userEvent.type(field(name), text);
  }
}

function changeButton(): HTMLElement {
  return within(part("Password")).getByRole("button", { name: "Change password" });
}

function expectCleared(): void {
  for (const one of fields()) expect(one).toHaveValue("");
}

/** A gateway that changes the password as `POST /api/auth/password` does. */
function passwordIs(current: string, tokens?: { tokens: fixtures.Token[] }) {
  const state = { bodies: [] as unknown[], csrf: [] as (string | null)[] };
  override("post", "/api/auth/password", async ({ request }) => {
    const body: unknown = await request.json();
    state.bodies.push(body);
    state.csrf.push(request.headers.get("x-csrf-token"));
    const given = (name: string): unknown =>
      typeof body === "object" && body !== null ? Reflect.get(body, name) : undefined;
    if (given("current_password") !== current) return refuse(errors.invalid_credentials);
    const next = given("new_password");
    if (typeof next !== "string" || next.length < 12) {
      return refuse(validationFailed({ new_password: fieldMessages.password }));
    }
    // All access tokens of the caller end.
    if (tokens !== undefined) {
      tokens.tokens = tokens.tokens.map((token) => ({
        ...token,
        revoked_at: token.revoked_at ?? "2026-09-30 10:00:00",
      }));
    }
    return noContent();
  });
  return state;
}

// -------------------------------------------------------------- the tokens

/** A gateway that keeps the access tokens and changes them as it is told. */
function keeps(start: readonly fixtures.Token[] = fixtures.tokenList) {
  const state = {
    tokens: [...start],
    lists: 0,
    created: [] as unknown[],
    csrf: [] as (string | null)[],
    revoked: [] as (string | undefined)[],
  };
  override("get", "/api/tokens", () => {
    state.lists += 1;
    return ok("get", "/api/tokens", 200, { tokens: state.tokens });
  });
  override("post", "/api/tokens", async ({ request }) => {
    const body: unknown = await request.json();
    state.created.push(body);
    state.csrf.push(request.headers.get("x-csrf-token"));
    const given = (name: string): unknown =>
      typeof body === "object" && body !== null ? Reflect.get(body, name) : undefined;
    const name = given("name");
    const expires = given("expires_at");
    const token: fixtures.Token = {
      ...active,
      id: 9,
      name: typeof name === "string" ? name : "",
      expires_at: typeof expires === "string" ? expires : null,
      last_used_at: null,
    };
    state.tokens = [token, ...state.tokens];
    return ok("post", "/api/tokens", 201, { token, secret: SECRET });
  });
  override("delete", "/api/tokens/{id}", ({ params }) => {
    state.revoked.push(params.id);
    state.tokens = state.tokens.map((token) =>
      String(token.id) === params.id ? { ...token, revoked_at: "2026-09-30 10:00:00" } : token,
    );
    return noContent();
  });
  return state;
}

function tokensAre(list: readonly fixtures.Token[]) {
  return counted("get", "/api/tokens", () => ok("get", "/api/tokens", 200, { tokens: [...list] }));
}

async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: "Access tokens" });
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

/** The cells of the row of the token, as their texts. */
function cellsOf(name: string): string[] {
  return within(rowOf(name))
    .getAllByRole("cell")
    .map((cell) => cell.textContent);
}

async function openCreate(): Promise<HTMLElement> {
  await userEvent.click(within(part("Access tokens")).getByRole("button", { name: "Create token" }));
  return screen.findByRole("dialog", { name: "Create token" });
}

function send(dialog: HTMLElement): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name: "Create token" }));
}

function secretDialog(): Promise<HTMLElement> {
  return screen.findByRole("dialog", { name: "Your new access token" });
}

async function closeSecret(): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: "Done" }));
  const question = await screen.findByRole("alertdialog");
  await userEvent.click(within(question).getByRole("button", { name: "Close" }));
  await closed();
}

async function askToRevoke(token: fixtures.Token): Promise<HTMLElement> {
  await userEvent.click(within(rowOf(token.name)).getByRole("button", { name: "Revoke" }));
  return screen.findByRole("alertdialog", { name: `Revoke ${token.name}?` });
}

function confirm(dialog: HTMLElement): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name: "Revoke" }));
}

// ===========================================================================

describe("the account page", () => {
  test("it has three parts, under one heading and in one main", async () => {
    await page();
    await table();
    expect(screen.getByRole("heading", { level: 1, name: "Account" })).toBeInTheDocument();
    expect(
      within(main())
        .getAllByRole("heading", { level: 2 })
        .map((heading) => heading.textContent),
    ).toEqual(["Profile", "Password", "Access tokens"]);
    expect(within(main()).getAllByRole("region")).toEqual([
      part("Profile"),
      part("Password"),
      part("Access tokens"),
    ]);
    expectOneMain();
    expect(toasts()).toEqual([]);
  });

  test("the session ends while the page is open", async () => {
    startGateway({ signedIn: true });
    const app = await page();
    await table();
    await expectSessionEndsOnPage(app, {
      path: "/api/tokens",
      queryKey: queryKeys.tokens.list(),
      at: "/account",
    });
    expect(shown()).not.toContain(active.display);
    expect(shown()).not.toContain(maya.email);
  });

  test("at width 390 the controls are high enough to touch", async () => {
    await page({ width: 390 });
    await screen.findByRole("list", { name: "Access tokens" });
    for (const control of [
      within(part("Profile")).getByRole("button", { name: "Edit name" }),
      ...fields(),
      changeButton(),
      within(part("Access tokens")).getByRole("button", { name: "Create token" }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
    // The fields are as wide as the page at most.
    for (const one of fields()) expect(one.className.split(/\s+/)).toContain("w-full");
    expectOneMain();
  });
});

describe("the profile", () => {
  test("it shows the name, the email, the role and the teams with the role in each", async () => {
    await page({ user: fixtures.me.arjun });
    expect([...profile().keys()]).toEqual(["Name", "Email", "Role", "Teams"]);
    expect(said("Name")).toHaveTextContent(arjun.name);
    expect(said("Email")).toHaveTextContent(arjun.email);
    const role = within(said("Role")).getByText("Member");
    expect(role).toHaveAttribute("data-slot", "badge");
    // Lead of Platform, member of Research.
    const teams = within(said("Teams")).getAllByRole("listitem");
    expect(teams.map((team) => team.textContent)).toEqual(["Platform Lead", "Research Member"]);
    for (const [index, text] of ["Lead", "Member"].entries()) {
      const team = teams[index];
      if (team === undefined) throw new Error("no team");
      expect(within(team).getByText(text)).toHaveAttribute("data-slot", "badge");
    }
  });

  test("an admin in no team", async () => {
    await page();
    expect(within(said("Role")).getByText("Admin")).toHaveAttribute("data-slot", "badge");
    expect(said("Teams")).toHaveTextContent("No teams");
    expect(within(said("Teams")).queryByRole("list")).toBeNull();
  });

  test("a role the console does not know is shown as it is, in a neutral pill", async () => {
    override("get", "/api/auth/me", () =>
      // What a later gateway may answer: the description of the API has neither value.
      HttpResponse.json({
        user: { ...arjun, role: "auditor" },
        teams: [{ team_id: 1, name: "Platform", role: "deputy" }],
        csrf_token: fixtures.csrfToken,
      }),
    );
    await page();
    for (const [label, text] of [
      ["Role", "auditor"],
      ["Teams", "deputy"],
    ] as const) {
      const pill = within(said(label)).getByText(text);
      expect(pill).toHaveAttribute("data-slot", "badge");
      expect(pill).toHaveAttribute("data-variant", "outline");
    }
  });

  test("profile name edit", async () => {
    const gateway = keepsTheName();
    await page();
    expect(said("Name")).toHaveTextContent(maya.name);
    expect(shellName()).toBe(maya.name);
    expect(gateway.me).toBe(1);

    const dialog = await openName();
    const name = within(dialog).getByLabelText("Name");
    expect(name).toHaveValue(maya.name);
    await userEvent.clear(name);
    await userEvent.type(name, "Maya O");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await closed();
    // The user's own id, and only the name.
    expect(gateway.patches).toEqual([{ name: "Maya O" }]);
    await waitFor(() => {
      expect(toasts()).toEqual(["Name changed."]);
    });
    // Who is signed in was asked for again: the profile and the shell follow.
    await waitFor(() => {
      expect(said("Name")).toHaveTextContent("Maya O");
    });
    expect(shellName()).toBe("Maya O");
    expect(gateway.me).toBe(2);
    expect(screen.queryByText(maya.name)).toBeNull();
    // The focus is back where the dialog was opened.
    await waitFor(() => {
      expect(within(part("Profile")).getByRole("button", { name: "Edit name" })).toHaveFocus();
    });
  });

  test("a member edits their own name: the call names their own id", async () => {
    // This gateway knows the id of who is signed in, and no other.
    const gateway = keepsTheName(fixtures.me.lena);
    await page();
    expect(said("Name")).toHaveTextContent(fixtures.users.lena.name);
    const dialog = await openName();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Lena F");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await closed();
    expect(gateway.patches).toEqual([{ name: "Lena F" }]);
    await waitFor(() => {
      expect(said("Name")).toHaveTextContent("Lena F");
    });
    expect(shellName()).toBe("Lena F");
  });

  test("a name that is not valid shows on its field, and the dialog keeps the value", async () => {
    const patches = counted("patch", "/api/users/{id}", () =>
      refuse(validationFailed({ name: fieldMessages.name })),
    );
    await page();
    const dialog = await openName();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(descriptionOf(name)).toBe(fieldMessages.name);
    });
    expect(name).toHaveValue("");
    expect(name).toHaveAttribute("aria-invalid", "true");
    expect(patches.calls).toBe(1);
    expect(toasts()).toEqual([]);

    // Closed and opened again, the dialog has the name as it is and no error.
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    expect(said("Name")).toHaveTextContent(maya.name);
    const again = await openName();
    expect(within(again).getByLabelText("Name")).toHaveValue(maya.name);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("while the name is saved the dialog stays: no Escape, no click beside it, no Cancel, no second submit", async () => {
    const door = gate();
    const patches = counted("patch", "/api/users/{id}", async () => {
      await door.opened;
      return ok("patch", "/api/users/{id}", 200, { ...maya, name: "Maya O" });
    });
    await page();
    const dialog = await openName();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, " O");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    const running = await within(dialog).findByRole("button", { name: "Saving" });
    expect(running).toBeDisabled();
    const open = () => screen.queryByRole("dialog", { name: "Edit name" });

    await userEvent.keyboard("{Escape}");
    expect(open()).toBe(dialog);
    await userEvent.click(overlay());
    expect(open()).toBe(dialog);
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(within(dialog).queryByRole("button", { name: "Close" })).toBeNull();
    await userEvent.type(name, "{Enter}");
    fireEvent.submit(within(dialog).getByRole("form", { name: "Edit name" }));
    await settle();
    expect(open()).toBe(dialog);
    expect(patches.calls).toBe(1);

    act(() => {
      door.open();
    });
    await closed();
    expect(patches.calls).toBe(1);
  });

  test("the session has ended when the name is saved: signed out, and the form says nothing", async () => {
    startGateway({ signedIn: true });
    const patches = counted("patch", "/api/users/{id}", unauthenticated);
    const app = await page();
    const dialog = await openName();
    await userEvent.type(within(dialog).getByLabelText("Name"), " O");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    await settle();
    expect(patches.calls).toBe(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(errors.unauthenticated.body.error.message)).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
  });
});

describe("the password", () => {
  test("the form: three fields that hide what is typed, and say what they are for", async () => {
    await page();
    expect(fields().map((one) => one.getAttribute("type"))).toEqual([
      "password",
      "password",
      "password",
    ]);
    expect(fields().map((one) => one.getAttribute("autocomplete"))).toEqual([
      "current-password",
      "new-password",
      "new-password",
    ]);
    // The names the API has for them; the confirmation is the console's own.
    expect(fields().map((one) => one.getAttribute("name"))).toEqual([
      "current_password",
      "new_password",
      "confirm_password",
    ]);
    for (const one of fields()) expect(one).toBeRequired();
    // Whose password it is, for the password manager of the browser: not shown, not sent.
    const form = within(part("Password")).getByRole("form", { name: "Change password" });
    const username = form.querySelector('input[autocomplete="username"]');
    expect(username).toHaveValue(maya.email);
    expect(username).toHaveAttribute("hidden");
    expect(username).toHaveAttribute("readonly");
    expect(username).not.toBeVisible();
    expect(form.querySelectorAll("input")).toHaveLength(4);
    expect(descriptionOf(field("New password"))).toBe("12 characters or more");
    expectLabelsNameControls(part("Password"));
    expectCleared();
  });

  test("password change success", async () => {
    startGateway({ signedIn: true });
    const tokens = keeps();
    const gateway = passwordIs(CURRENT, tokens);
    const app = await page();
    await table();
    expect(tokens.lists).toBe(1);
    expect(cellsOf(active.name)[4]).toBe("active");

    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    // The message of rule 3.
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    // What the API takes: the confirmation is not sent.
    expect(gateway.bodies).toEqual([{ current_password: CURRENT, new_password: NEXT }]);
    expect(gateway.csrf).toEqual([fixtures.csrfToken]);
    // The fields are cleared, and nothing keeps the passwords.
    expectCleared();
    expect(screen.queryByRole("alert")).toBeNull();
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    for (const password of [CURRENT, NEXT]) expectNoSecret(app, password);

    // All access tokens were ended: the list is read again, and says so.
    await waitFor(() => {
      expect(cellsOf(active.name)[4]).toBe("revoked");
    });
    expect(tokens.lists).toBe(2);
    expect(cellsOf(neverUsed.name)[4]).toBe("revoked");
    expect(within(part("Access tokens")).queryByRole("button", { name: "Revoke" })).toBeNull();

    // The session of the caller goes on: still signed in, with the token it had.
    expect(href(app)).toBe("/account");
    expect(screen.getByRole("heading", { level: 1, name: "Account" })).toBeInTheDocument();
    expect(screen.queryByText(SESSION_ENDED)).toBeNull();
    expect(app.queryClient.getQueryData(queryKeys.me())).toBeDefined();
    // A further call succeeds.
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    expect(await secretDialog()).toBeInTheDocument();
    expect(tokens.csrf).toEqual([fixtures.csrfToken]);
  });

  test("while the password is changed the button is disabled and says so; one request", async () => {
    const door = gate();
    const posts = counted("post", "/api/auth/password", async () => {
      await door.opened;
      return noContent();
    });
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    const running = await within(part("Password")).findByRole("button", {
      name: "Changing the password",
    });
    expect(running).toBeDisabled();
    await userEvent.type(field("Confirm new password"), "{Enter}");
    fireEvent.submit(within(part("Password")).getByRole("form", { name: "Change password" }));
    await settle();
    expect(posts.calls).toBe(1);
    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    expect(changeButton()).toBeEnabled();
    expect(posts.calls).toBe(1);
    expectCleared();
  });

  test("after the change the focus is back on the button, which lost it while it was disabled", async () => {
    const door = gate();
    override("post", "/api/auth/password", async () => {
      await door.opened;
      return noContent();
    });
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    const running = await within(part("Password")).findByRole("button", {
      name: "Changing the password",
    });
    expect(running).toBeDisabled();
    // A browser takes the focus from a button that is disabled, and nothing has it
    // then. jsdom leaves it: here it goes as it does when what has it is removed.
    act(() => {
      const probe = document.createElement("button");
      document.body.append(probe);
      probe.focus();
      probe.remove();
    });
    expect(document.body).toHaveFocus();
    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    await waitFor(() => {
      expect(changeButton()).toHaveFocus();
    });
  });

  test("a change that was sent from a field leaves the focus in that field", async () => {
    passwordIs(CURRENT);
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.type(field("Confirm new password"), "{Enter}");
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    await settle();
    expect(field("Confirm new password")).toHaveFocus();
    expectCleared();
  });

  test("wrong current password", async () => {
    const session = startGateway({ signedIn: true });
    const gateway = passwordIs(CURRENT);
    const app = await page();
    await table();
    const asked = session.meCalls;

    await fill("not-the-password", NEXT);
    await userEvent.click(changeButton());
    // Shown on the current field, in the words of the form.
    const current = field("Current password");
    await waitFor(() => {
      expect(descriptionOf(current)).toBe(WRONG_CURRENT);
    });
    expect(current).toHaveAttribute("aria-invalid", "true");
    expect(within(part("Password")).getAllByRole("alert")).toHaveLength(1);
    expect(screen.queryByText(errors.invalid_credentials.body.error.message)).toBeNull();
    expect(gateway.bodies).toEqual([{ current_password: "not-the-password", new_password: NEXT }]);
    // The three fields are cleared, and the focus is on the first of them.
    expectCleared();
    await waitFor(() => {
      expect(current).toHaveFocus();
    });
    expect(toasts()).toEqual([]);
    // Nothing keeps what was typed.
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    for (const password of ["not-the-password", NEXT]) expectNoSecret(app, password);

    // The user is still signed in: this 401 is not the end of a session.
    await settle();
    expect(href(app)).toBe("/account");
    expect(screen.getByRole("heading", { level: 1, name: "Account" })).toBeInTheDocument();
    expect(screen.queryByText(SESSION_ENDED)).toBeNull();
    expect(app.queryClient.getQueryData(queryKeys.me())).toBeDefined();
    expect(app.queryClient.getQueryData(queryKeys.tokens.list())).toBeDefined();
    expect(session.meCalls).toBe(asked);
    expect(screen.getByRole("table", { name: "Access tokens" })).toBeInTheDocument();

    // A second submit right after it sends nothing: the form is empty.
    await userEvent.click(changeButton());
    await userEvent.type(current, "{Enter}");
    fireEvent.submit(within(part("Password")).getByRole("form", { name: "Change password" }));
    await settle();
    expect(gateway.bodies).toHaveLength(1);
    expect(descriptionOf(current)).toBe(WRONG_CURRENT);

    // The error goes when the field is typed in, and the right password is taken.
    await fill(CURRENT, NEXT);
    expect(descriptionOf(current)).toBe("");
    expect(current).not.toHaveAttribute("aria-invalid");
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    expect(gateway.bodies).toHaveLength(2);
  });

  test("password mismatch sends nothing", async () => {
    const posts = counted("post", "/api/auth/password", noContent);
    await page();
    await fill(CURRENT, NEXT, "next-staple-correct-8");
    await userEvent.click(changeButton());
    const confirmation = field("Confirm new password");
    await waitFor(() => {
      expect(descriptionOf(confirmation)).toBe(DIFFER);
    });
    expect(confirmation).toHaveAttribute("aria-invalid", "true");
    await waitFor(() => {
      expect(confirmation).toHaveFocus();
    });
    await settle();
    expect(posts.calls).toBe(0);
    expect(toasts()).toEqual([]);
    // Nothing was sent, so nothing was refused: what was typed stays, to be corrected.
    expect(field("Current password")).toHaveValue(CURRENT);
    expect(field("New password")).toHaveValue(NEXT);
    expect(confirmation).toHaveValue("next-staple-correct-8");

    // Corrected, it is sent.
    await userEvent.clear(confirmation);
    expect(descriptionOf(confirmation)).toBe("");
    await userEvent.type(confirmation, NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    expect(posts.bodies).toEqual([{ current_password: CURRENT, new_password: NEXT }]);
    expectCleared();
  });

  test.each([
    ["nothing is typed", "", "", ""],
    ["the current password is missing", "", NEXT, NEXT],
    ["the new password is missing", CURRENT, "", ""],
    ["the confirmation is missing", CURRENT, NEXT, ""],
  ])("a form in which %s is not sent", async (_, current, next, confirmation) => {
    const posts = counted("post", "/api/auth/password", noContent);
    await page();
    await fill(current, next, confirmation);
    await userEvent.click(changeButton());
    await userEvent.type(field("New password"), "{Enter}");
    fireEvent.submit(within(part("Password")).getByRole("form", { name: "Change password" }));
    await settle();
    // Every attempt counts against the limit of the gateway: none is made for nothing.
    expect(posts.calls).toBe(0);
    expect(toasts()).toEqual([]);
  });

  test("too many attempts", async () => {
    const posts = counted("post", "/api/auth/password", () => refuse(errors.too_many_attempts));
    const app = await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    const current = field("Current password");
    // The text of the sign-in page, not the one of the gateway.
    await waitFor(() => {
      expect(descriptionOf(current)).toBe(TOO_MANY);
    });
    expect(screen.queryByText(errors.too_many_attempts.body.error.message)).toBeNull();
    expectCleared();
    await waitFor(() => {
      expect(current).toHaveFocus();
    });
    expect(posts.calls).toBe(1);
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe("/account");
    for (const password of [CURRENT, NEXT]) expectNoSecret(app, password);
  });

  test("a new password the gateway does not take shows on its field; the fields are cleared", async () => {
    const gateway = passwordIs(CURRENT);
    await page();
    await fill(CURRENT, "short", "short");
    await userEvent.click(changeButton());
    const next = field("New password");
    await waitFor(() => {
      expect(descriptionOf(next)).toBe(`${fieldMessages.password} 12 characters or more`);
    });
    expect(next).toHaveAttribute("aria-invalid", "true");
    expect(field("Current password")).not.toHaveAttribute("aria-invalid");
    expect(gateway.bodies).toHaveLength(1);
    expectCleared();
    // All three are empty: the user starts again at the first.
    await waitFor(() => {
      expect(field("Current password")).toHaveFocus();
    });
    expect(toasts()).toEqual([]);
  });

  test("a refusal that is about no field shows at the top of the form; the fields are cleared", async () => {
    override("post", "/api/auth/password", () => refuse(errors.internal_error));
    const app = await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    expect(await within(part("Password")).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    for (const one of fields()) expect(one).not.toHaveAttribute("aria-invalid");
    expectCleared();
    expect(toasts()).toEqual([]);
    for (const password of [CURRENT, NEXT]) expectNoSecret(app, password);
  });

  test("a gateway that cannot be reached: the form says so, and the fields are cleared", async () => {
    override("post", "/api/auth/password", networkFailure);
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    expect(await within(part("Password")).findByRole("alert")).toHaveTextContent(
      "Could not reach the gateway.",
    );
    expectCleared();
  });

  test("the session has ended when the password is changed: signed out, and the form says nothing", async () => {
    startGateway({ signedIn: true });
    const posts = counted("post", "/api/auth/password", unauthenticated);
    const app = await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    await settle();
    expect(posts.calls).toBe(1);
    expect(screen.getByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    // No error of a field, none of a form, and not the answer of the gateway.
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(WRONG_CURRENT)).toBeNull();
    expect(screen.queryByText(errors.unauthenticated.body.error.message)).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    for (const password of [CURRENT, NEXT]) expectNoSecret(app, password);
  });

  test("a change whose answer came for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("post", "/api/auth/password", async () => {
      await door.opened;
      return noContent();
    });
    const app = await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    await within(part("Password")).findByRole("button", { name: "Changing the password" });
    await aCallFindsTheSessionEnded("/api/providers");
    door.open();
    await settle();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
    for (const password of [CURRENT, NEXT]) expectNoSecret(app, password);
  });

  test("nothing is written to the console about a password", async () => {
    const written = listenToConsole();
    passwordIs(CURRENT);
    await page();
    await fill("not-the-password", NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(descriptionOf(field("Current password"))).toBe(WRONG_CURRENT);
    });
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    for (const password of ["not-the-password", CURRENT, NEXT]) {
      expect(written()).not.toContain(password);
    }
  });
});

describe("the access tokens", () => {
  test("the list: name, what is shown of the token, expiry, last use and status", async () => {
    await page();
    const tokens = await table();
    expect(within(tokens).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Name",
      "Token",
      "Expires",
      "Last used",
      "Status",
      "Actions",
    ]);
    for (const token of fixtures.tokenList) {
      const display = within(rowOf(token.name)).getByText(token.display);
      expect(display.className.split(/\s+/)).toContain("font-mono");
    }
    // The status is worked out from what the API gives: it has no field for it.
    expect(fixtures.tokenList.map((token) => cellsOf(token.name)[4])).toEqual([
      "active",
      "active",
      "revoked",
      "expired",
    ]);
    for (const token of fixtures.tokenList) {
      const status = cellsOf(token.name)[4] ?? "";
      expect(within(rowOf(token.name)).getByText(status)).toHaveAttribute("data-slot", "badge");
    }
    // Times are times; what never was says so.
    expect(cellsOf(active.name)[2]).toBe("Never");
    expect(cellsOf(neverUsed.name)[3]).toBe("Never");
    expect(within(rowOf(neverUsed.name)).getByText(/2027|2026/).tagName).toBe("TIME");
    expect(rowOf(active.name).querySelector("time")).toHaveAttribute(
      "title",
      `${active.last_used_at} UTC`,
    );
    // What is revoked cannot be revoked again; the rest can.
    expect(within(rowOf(revoked.name)).queryByRole("button")).toBeNull();
    for (const token of [active, neverUsed, expired]) {
      expect(within(rowOf(token.name)).getByRole("button", { name: "Revoke" })).toBeInTheDocument();
    }
    // A token itself is nowhere: the list has only what the gateway shows of it.
    expect(shown()).not.toContain(SECRET);
    expect(shown()).not.toMatch(/uf-at-[0-9a-f]{8}/);
  });

  test("loading shows skeleton rows", async () => {
    const door = gate();
    override("get", "/api/tokens", async () => {
      await door.opened;
      return ok("get", "/api/tokens", 200, { tokens: fixtures.tokenList });
    });
    await page();
    const tokens = screen.getByRole("table", { name: "Access tokens" });
    expect(tokens).toHaveAttribute("aria-busy", "true");
    expect(tokens.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    // The rest of the page does not wait for them.
    expect(said("Name")).toHaveTextContent(maya.name);
    expect(changeButton()).toBeEnabled();
    act(() => {
      door.open();
    });
    await table();
    expect(tokens.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0);
  });

  test("an empty list says so", async () => {
    tokensAre([]);
    await page();
    expect(await screen.findByRole("heading", { name: "No access tokens" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    expect(
      within(part("Access tokens")).getByRole("button", { name: "Create token" }),
    ).toBeInTheDocument();
  });

  test("a failed list call shows the error with Retry, and Retry asks again; the rest of the page stays", async () => {
    const failing = counted("get", "/api/tokens", () => refuse(errors.internal_error));
    await page();
    const tokens = part("Access tokens");
    expect(await within(tokens).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.queryByRole("table")).toBeNull();
    expect(within(tokens).queryByRole("button", { name: "Create token" })).toBeNull();
    expect(toasts()).toEqual([]);
    expect(failing.calls).toBe(1);
    expect(said("Email")).toHaveTextContent(maya.email);
    expect(changeButton()).toBeInTheDocument();

    const again = tokensAre(fixtures.tokenList);
    await userEvent.click(within(tokens).getByRole("button", { name: "Retry" }));
    await table();
    expect(again.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(within(tokens).getByRole("button", { name: "Create token" })).toBeInTheDocument();
  });

  test("a gateway that cannot be reached is an error with Retry", async () => {
    override("get", "/api/tokens", networkFailure);
    await page();
    expect(await within(part("Access tokens")).findByRole("alert")).toHaveTextContent(
      "Could not reach the gateway.",
    );
    expect(within(part("Access tokens")).getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("the list call answers 403: the part says that it is not available, not an error and not a toast", async () => {
    const refused = counted("get", "/api/tokens", () => refuse(errors.forbidden));
    await page();
    const tokens = part("Access tokens");
    expect(await within(tokens).findByText(PART_NOT_AVAILABLE)).toBeInTheDocument();
    expect(refused.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(errors.forbidden.body.error.message)).toBeNull();
    expect(within(tokens).queryByRole("button")).toBeNull();
    expect(screen.queryByRole("table")).toBeNull();
    expect(toasts()).toEqual([]);
    // The profile and the password are the user's own whatever the tokens are.
    expect(screen.getByRole("heading", { level: 1, name: "Account" })).toBeInTheDocument();
    expect(said("Email")).toHaveTextContent(maya.email);
    expect(changeButton()).toBeInTheDocument();
    expectOneMain();
  });

  test("at width 390 the rows are cards with their labels and actions", async () => {
    await page({ width: 390 });
    const cards = await screen.findByRole("list", { name: "Access tokens" });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    expect(screen.queryByRole("table")).toBeNull();
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(fixtures.tokenList.length);
    const [first] = items;
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Name",
      "Token",
      "Expires",
      "Last used",
      "Status",
    ]);
    expect(first).toHaveTextContent(active.name);
    const revoke = within(first).getByRole("button", { name: "Revoke" });
    expect(revoke.className.split(/\s+/)).toContain("min-h-11");
  });

  test("the list sorts by name", async () => {
    await page();
    await table();
    const names = () =>
      within(screen.getByRole("table", { name: "Access tokens" }))
        .getAllByRole("row")
        .slice(1)
        .map((row) => within(row).getAllByRole("cell")[0]?.textContent);
    expect(names()).toEqual(fixtures.tokenList.map((token) => token.name));
    await userEvent.click(screen.getByRole("button", { name: "Name" }));
    expect(names()).toEqual(fixtures.tokenList.map((token) => token.name).sort());
  });
});

describe("creating a token", () => {
  test("token is shown once", async () => {
    const written = listenToConsole();
    const state = keeps();
    const app = await page();
    await table();
    expect(state.lists).toBe(1);
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);

    const secret = await secretDialog();
    expect(screen.queryByRole("dialog", { name: "Create token" })).toBeNull();
    expect(secret).toHaveTextContent(SHOWN_ONCE);
    expect(within(secret).getByLabelText("Your new access token")).toHaveValue(SECRET);
    // How to use it: the header with a placeholder.
    const usage = within(secret).getByRole("group", { name: "How to use the token" });
    expect(usage).toHaveTextContent("Use it as Authorization: Bearer <token> with the admin API.");
    expect(usage.querySelector("code")).toHaveTextContent("Authorization: Bearer <token>");
    expect(usage.textContent).not.toContain(SECRET);
    expect(usage.textContent).not.toContain(SECRET.slice(-8));
    expect(secret.textContent).not.toContain(SECRET);
    // While it is shown, the token is in the state of the page only.
    expect(cached(app.queryClient)).not.toContain(SECRET);
    expect(JSON.stringify(app.router.state)).not.toContain(SECRET);
    expect(href(app)).toBe("/account");
    expect(toasts()).toEqual([]);

    await closeSecret();
    expectNoSecret(app, SECRET);
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    expect(toasts().join()).not.toContain(SECRET);
    // The list was asked for again, and has the token.
    expect(await screen.findByRole("cell", { name: "deploy" })).toBeInTheDocument();
    expect(state.lists).toBe(2);
    expect(state.created).toEqual([{ name: "deploy" }]);
    // Nothing was written to the console about it.
    expect(written()).not.toContain(SECRET);
    expect(written()).not.toContain(SECRET.slice(6));
  });

  test("the labels of the form name controls, and the group of expiries is named once", async () => {
    await page();
    await table();
    const dialog = await openCreate();
    expectLabelsNameControls(dialog);
    const expires = within(dialog).getByRole("radiogroup", { name: "Expires" });
    expect(within(expires).getAllByRole("radio")).toEqual(
      ["Never", "In 30 days", "In 90 days", "On a date"].map((name) =>
        within(expires).getByRole("radio", { name }),
      ),
    );
    expect(within(expires).getByRole("radio", { name: "Never" })).toBeChecked();
    expect(descriptionOf(expires)).toBe("A token expires at the end of its day, in UTC.");
    expect(within(dialog).queryByLabelText("Expiry date")).toBeNull();
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
      await page();
      await table();
      const dialog = await openCreate();
      await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
      const expires = within(dialog).getByRole("radiogroup", { name: "Expires" });
      if (choice !== "Never") {
        await userEvent.click(within(expires).getByRole("radio", { name: choice }));
      }
      if (date !== undefined) {
        const day = within(dialog).getByLabelText("Expiry date");
        expect(day).toHaveAttribute("type", "date");
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
      expect(body).toEqual({ name: "deploy" });
      expect(Object.keys(body as object)).not.toContain("expires_at");
    });

    test("a date is sent as the end of that day", async () => {
      expect(await created("On a date", "2027-01-31")).toEqual({
        name: "deploy",
        expires_at: "2027-01-31 23:59:59",
      });
    });

    test("30 days are counted from today in UTC", async () => {
      expect(await created("In 30 days")).toEqual({
        name: "deploy",
        expires_at: "2026-10-31 23:59:59",
      });
    });

    test("90 days are counted from today in UTC", async () => {
      expect(await created("In 90 days")).toEqual({
        name: "deploy",
        expires_at: "2026-12-30 23:59:59",
      });
    });
  });

  test("without a date nothing is sent, and the field says so", async () => {
    const state = keeps();
    await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
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

  test("the errors of the fields show on their fields, and the dialog keeps what was typed", async () => {
    const posts = counted("post", "/api/tokens", () =>
      refuse(
        validationFailed({ name: fieldMessages.name, expires_at: fieldMessages.expiresAtPast }),
      ),
    );
    await page();
    await table();
    const dialog = await openCreate();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, "x");
    await send(dialog);
    await waitFor(() => {
      expect(descriptionOf(name)).toBe(fieldMessages.name);
    });
    expect(name).toHaveAttribute("aria-invalid", "true");
    const expires = within(dialog).getByRole("radiogroup", { name: "Expires" });
    expect(descriptionOf(expires)).toContain(fieldMessages.expiresAtPast);
    expect(name).toHaveValue("x");
    expect(posts.calls).toBe(1);
    expect(toasts()).toEqual([]);
    expect(screen.queryByRole("dialog", { name: "Your new access token" })).toBeNull();
  });

  test("a refusal that is about no field shows at the top of the form", async () => {
    override("post", "/api/tokens", () => refuse(errors.internal_error));
    await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(within(dialog).getByLabelText("Name")).not.toHaveAttribute("aria-invalid");
  });

  test("a dialog that is opened again is empty and shows no old error", async () => {
    override("post", "/api/tokens", () => refuse(errors.internal_error));
    const app = await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    await within(dialog).findByRole("alert");
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    const again = await openCreate();
    expect(within(again).getByLabelText("Name")).toHaveValue("");
    expect(within(again).queryByRole("alert")).toBeNull();
    expect(within(again).getByRole("radio", { name: "Never" })).toBeChecked();
  });

  describe("while the token is created the dialog stays", () => {
    async function sending(answer: () => Response) {
      const door = gate();
      const posts = counted("post", "/api/tokens", async () => {
        await door.opened;
        return answer();
      });
      const app = await page();
      await table();
      const dialog = await openCreate();
      const name = within(dialog).getByLabelText("Name");
      await userEvent.type(name, "deploy");
      await send(dialog);
      const running = await within(dialog).findByRole("button", { name: "Creating the token" });
      expect(running).toBeDisabled();
      return { app, dialog, name, posts, door };
    }

    test("Escape, a click beside it, Cancel and a second submit do nothing; one request, and the token is shown once", async () => {
      const { app, dialog, name, posts, door } = await sending(() =>
        ok("post", "/api/tokens", 201, { token: active, secret: SECRET }),
      );
      const open = () => screen.queryByRole("dialog", { name: "Create token" });

      await userEvent.keyboard("{Escape}");
      expect(open()).toBe(dialog);
      await userEvent.click(overlay());
      expect(open()).toBe(dialog);
      expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
      expect(within(dialog).queryByRole("button", { name: "Close" })).toBeNull();
      await userEvent.type(name, "{Enter}");
      fireEvent.submit(within(dialog).getByRole("form", { name: "Create token" }));
      await settle();
      expect(open()).toBe(dialog);
      expect(name).toHaveValue("deploy");
      expect(posts.calls).toBe(1);

      act(() => {
        door.open();
      });
      const secret = await secretDialog();
      expect(within(secret).getByLabelText("Your new access token")).toHaveValue(SECRET);
      expect(open()).toBeNull();
      await closeSecret();
      await settle();
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(posts.calls).toBe(1);
      expectNoSecret(app, SECRET);
      await waitFor(() => {
        expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
      });
    });

    test("after a refusal it can be left again", async () => {
      const { dialog, posts, door } = await sending(() => refuse(errors.forbidden));
      act(() => {
        door.open();
      });
      expect(await within(dialog).findByRole("alert")).toHaveTextContent(
        errors.forbidden.body.error.message,
      );
      await waitFor(() => {
        expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
      });
      expect(within(dialog).getByRole("button", { name: "Close" })).toBeInTheDocument();
      await userEvent.keyboard("{Escape}");
      await closed();
      expect(posts.calls).toBe(1);
    });
  });

  test("the session has ended when the token is created: signed out, and the form says nothing", async () => {
    startGateway({ signedIn: true });
    const posts = counted("post", "/api/tokens", unauthenticated);
    const app = await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    await settle();
    expect(posts.calls).toBe(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(errors.unauthenticated.body.error.message)).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
  });

  test("a token whose answer came for a session that is over is not shown", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("post", "/api/tokens", async () => {
      await door.opened;
      return ok("post", "/api/tokens", 201, { token: active, secret: SECRET });
    });
    const app = await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    await within(dialog).findByRole("button", { name: "Creating the token" });
    await aCallFindsTheSessionEnded("/api/providers");
    door.open();
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expectNoSecret(app, SECRET);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
  });

  test("the session ends while the new token is shown", async () => {
    startGateway({ signedIn: true });
    const app = await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
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
      expect(href(app)).toBe("/sign-in?next=%2Faccount");
    });
    expectNoSecret(app, SECRET);
    expect(toasts()).toEqual([]);
  });

  test("at width 390 the controls of the dialog are high enough to touch", async () => {
    await page({ width: 390 });
    await screen.findByRole("list", { name: "Access tokens" });
    const dialog = await openCreate();
    for (const control of [
      within(dialog).getByLabelText("Name"),
      within(dialog).getByRole("button", { name: "Create token" }),
      within(dialog).getByRole("button", { name: "Cancel" }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
    expect(dialog.className).toContain("max-w-[calc(100%-2rem)]");
  });
});

describe("revoking a token", () => {
  test("token revoke", async () => {
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
    expect(toasts()).toEqual(["Token revoked."]);
    // The list was asked for again: the pill of the token changes where it is.
    await waitFor(() => {
      expect(cellsOf(active.name)[4]).toBe("revoked");
    });
    expect(state.lists).toBe(2);
    expect(within(rowOf(active.name)).getByText("revoked")).toHaveAttribute("data-slot", "badge");
    // What is revoked cannot be revoked again.
    expect(within(rowOf(active.name)).queryByRole("button")).toBeNull();
    // The others are as they were.
    expect(cellsOf(neverUsed.name)[4]).toBe("active");
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
    expect(cellsOf(active.name)[4]).toBe("active");
    expect(state.lists).toBe(1);
  });

  test("a refusal stays in the dialog", async () => {
    const state = keeps();
    const removes = counted("delete", "/api/tokens/{id}", () => refuse(errors.not_found));
    await page();
    await table();
    const dialog = await askToRevoke(active);
    await confirm(dialog);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.not_found.body.error.message,
    );
    expect(removes.calls).toBe(1);
    expect(toasts()).toEqual([]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    expect(cellsOf(active.name)[4]).toBe("active");
    expect(state.revoked).toEqual([]);
  });

  test("a revocation whose answer came for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("delete", "/api/tokens/{id}", async () => {
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
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
  });
});
