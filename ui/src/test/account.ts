// What the tests of the account page share: the page and its three parts,
// and the gateways that keep a name, a password and the access tokens. The
// tests of each part are a file of their own, as the parts of the page are.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect } from "vitest";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { noContent, ok, override, refuse } from "@/test/handlers";
import { counted, rowWithCell } from "@/test/pages";
import { renderWithApp, type AppRenderResult } from "@/test/render";

export const { maya, arjun } = fixtures.users;
export const { active, neverUsed, revoked, expired } = fixtures.tokens;
export const SECRET = fixtures.newTokenSecret;

export const PASSWORD_CHANGED =
  "Password changed. Your other sessions and all your access tokens were ended.";
export const WRONG_CURRENT = "Current password is incorrect.";
export const TOO_MANY = "Too many attempts. Try again in a few minutes.";
export const DIFFER = "The passwords do not match.";
export const SHOWN_ONCE = "Copy this token now. It is not shown again.";
export const REVOKE = "Scripts using this token stop working at once. This cannot be undone.";
export const PART_NOT_AVAILABLE = "Not available to your account.";

/** The password the gateway of these tests knows, and the one it is changed to. */
export const CURRENT = "current-horse-battery";
export const NEXT = "next-staple-correct-7";

type Options = { user?: fixtures.Me; width?: number };

export function page(options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/account", ...options });
}

export function main(): HTMLElement {
  return screen.getByRole("main");
}

export function part(name: "Profile" | "Password" | "Access tokens"): HTMLElement {
  return within(main()).getByRole("region", { name });
}

/** What the profile says, by its labels. */
export function profile(): Map<string, HTMLElement> {
  const found = new Map<string, HTMLElement>();
  for (const label of part("Profile").querySelectorAll("dt")) {
    const value = label.nextElementSibling;
    if (value instanceof HTMLElement) found.set(label.textContent, value);
  }
  return found;
}

export function said(label: string): HTMLElement {
  const value = profile().get(label);
  if (value === undefined) throw new Error(`the profile has no ${label}`);
  return value;
}

/** What the sidebar says of who is signed in. */
export function shellName(): string {
  const signOut = within(screen.getByRole("navigation", { name: "Main" })).getByRole("button", {
    name: "Sign out",
  });
  return signOut.parentElement?.querySelector(".font-medium")?.textContent ?? "";
}

// ---------------------------------------------------------------- the name

/** A gateway that keeps the name of who is signed in, and says it in `me`. */
export function keepsTheName(start: fixtures.Me = fixtures.me.maya) {
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

export async function openName(): Promise<HTMLElement> {
  await userEvent.click(within(part("Profile")).getByRole("button", { name: "Edit name" }));
  return screen.findByRole("dialog", { name: "Edit name" });
}

export async function closed(): Promise<void> {
  await waitFor(() => {
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

export function overlay(): Element {
  const found = document.querySelector('[data-slot="dialog-overlay"]');
  if (found === null) throw new Error("no overlay");
  return found;
}

// ------------------------------------------------------------ the password

export function field(name: "Current password" | "New password" | "Confirm new password"): HTMLElement {
  return within(part("Password")).getByLabelText(name);
}

export function fields(): HTMLElement[] {
  return [field("Current password"), field("New password"), field("Confirm new password")];
}

const PASSWORD_FIELDS = ["Current password", "New password", "Confirm new password"] as const;

/**
 * Fills the three fields of the password form, each at once, as a paste
 * does: how a field takes what is typed is not what the tests that call it
 * look at, and typing every key of three passwords takes long. A test that
 * types for real calls `fillByTyping`.
 */
export async function fill(current: string, next: string, confirm: string = next): Promise<void> {
  for (const [index, text] of [current, next, confirm].entries()) {
    const name = PASSWORD_FIELDS[index];
    if (name === undefined) continue;
    await userEvent.clear(field(name));
    if (text !== "") await userEvent.paste(text);
  }
}

/** Fills the three fields of the password form key by key, as a person types. */
export async function fillByTyping(
  current: string,
  next: string,
  confirm: string = next,
): Promise<void> {
  for (const [index, text] of [current, next, confirm].entries()) {
    const name = PASSWORD_FIELDS[index];
    if (name === undefined) continue;
    await userEvent.clear(field(name));
    if (text !== "") await userEvent.type(field(name), text);
  }
}

export function changeButton(): HTMLElement {
  return within(part("Password")).getByRole("button", { name: "Change password" });
}

export function expectCleared(): void {
  for (const one of fields()) expect(one).toHaveValue("");
}

/** The error at the top of the password form: before its fields, and the error of none of them. */
export function formError(): HTMLElement | null {
  const form = within(part("Password")).getByRole("form", { name: "Change password" });
  const first = form.firstElementChild;
  return first instanceof HTMLElement && first.getAttribute("role") === "alert" ? first : null;
}

/** A gateway that changes the password as `POST /api/auth/password` does. */
export function passwordIs(current: string, tokens?: { tokens: fixtures.Token[] }) {
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
        // Revoked comes first, as the gateway says it.
        status: "revoked",
      }));
    }
    return noContent();
  });
  return state;
}

// -------------------------------------------------------------- the tokens

/** A gateway that keeps the access tokens and changes them as it is told. */
export function keeps(start: readonly fixtures.Token[] = fixtures.tokenList) {
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
      String(token.id) === params.id
          ? { ...token, revoked_at: "2026-09-30 10:00:00", status: "revoked" }
          : token,
    );
    return noContent();
  });
  return state;
}

export function tokensAre(list: readonly fixtures.Token[]) {
  return counted("get", "/api/tokens", () => ok("get", "/api/tokens", 200, { tokens: [...list] }));
}

export async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: "Access tokens" });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

export function rowOf(name: string): HTMLElement {
  return rowWithCell(name);
}

/** The cells of the row of the token, as their texts, read through the elements. */
export function cellsOf(name: string): string[] {
  return [...rowOf(name).querySelectorAll("td")].map((cell) => cell.textContent);
}

export async function openCreate(): Promise<HTMLElement> {
  await userEvent.click(within(part("Access tokens")).getByRole("button", { name: "Create token" }));
  return screen.findByRole("dialog", { name: "Create token" });
}

export function send(dialog: HTMLElement): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name: "Create token" }));
}

export function secretDialog(): Promise<HTMLElement> {
  return screen.findByRole("dialog", { name: "Your new access token" });
}

export async function closeSecret(): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: "Done" }));
  const question = await screen.findByRole("alertdialog");
  await userEvent.click(within(question).getByRole("button", { name: "Close" }));
  await closed();
}

export async function askToRevoke(token: fixtures.Token): Promise<HTMLElement> {
  await userEvent.click(within(rowOf(token.name)).getByRole("button", { name: "Revoke" }));
  return screen.findByRole("alertdialog", { name: `Revoke ${token.name}?` });
}

export function confirm(dialog: HTMLElement): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name: "Revoke" }));
}
