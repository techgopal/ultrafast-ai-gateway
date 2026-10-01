// The steps the flows share, and the forms of what the gateway gives out.
import { expect, type Locator, type Page } from "@playwright/test";
import type { Account } from "./gateway";

/**
 * Types a secret into a field. It sets the value as typing does and tells the
 * page by an `input` event; unlike `fill`, its step in the report does not
 * show the text.
 */
export async function typeSecret(field: Locator, text: string): Promise<void> {
  await expect(field).toBeEditable();
  await field.evaluate((element, value) => {
    if (!(element instanceof HTMLInputElement)) throw new Error("The field is not an input.");
    element.focus();
    // The setter of the prototype: React watches the one of the element, and
    // takes a value set through it for no change.
    // eslint-disable-next-line @typescript-eslint/unbound-method -- it is called on the input below.
    const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    if (setValue === undefined) throw new Error("An input has no value setter.");
    setValue.call(element, value);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  }, text);
}

/** Fills the sign-in form and sends it. */
export async function signIn(page: Page, account: Account): Promise<void> {
  const form = page.getByRole("form", { name: "Sign in" });
  await typeSecret(form.getByLabel("Email", { exact: true }), account.email);
  await typeSecret(form.getByLabel("Password", { exact: true }), account.password);
  await form.getByRole("button", { name: "Sign in" }).click();
}

/** The main heading of the page. */
export function heading(page: Page, name: string): Locator {
  return page.getByRole("heading", { level: 1, name, exact: true });
}

/** Opens the sign-in page and signs in; ends on the Overview. */
export async function signInFromStart(page: Page, account: Account): Promise<void> {
  await page.goto("/sign-in");
  await signIn(page, account);
  await expect(heading(page, "Overview")).toBeVisible();
}

/** The menu button of a narrow screen, which opens the drawer. */
export function menuButton(page: Page): Locator {
  return page.getByRole("button", { name: "Open menu" });
}

/**
 * Whether the page is below the width at which the console's sidebar is a
 * drawer (768 px). Decided by the viewport and not by what shows: the
 * console's first frame after a load is the wide one.
 */
export function isNarrow(page: Page): boolean {
  return (page.viewportSize()?.width ?? 1280) < 768;
}

/** Opens the drawer on a narrow screen; on a wide one the navigation is always there. */
export async function openNavigation(page: Page): Promise<Locator> {
  const nav = page.getByRole("navigation", { name: "Main" });
  if (isNarrow(page)) await menuButton(page).click();
  await expect(nav).toBeVisible();
  return nav;
}

/** Closes the drawer of a narrow screen. */
export async function closeNavigation(page: Page): Promise<void> {
  if (!isNarrow(page)) return;
  const drawer = page.getByRole("dialog", { name: "Sidebar" });
  await expect(drawer).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
}

/** Goes to a page through the navigation, as a user does. */
export async function goTo(page: Page, label: string, title: string = label): Promise<void> {
  const nav = await openNavigation(page);
  await nav.getByRole("link", { name: label, exact: true }).click();
  await expect(heading(page, title)).toBeVisible();
}

/** Signs out through the button of the navigation. */
export async function signOut(page: Page): Promise<void> {
  const nav = await openNavigation(page);
  await nav.getByRole("button", { name: "Sign out" }).click();
  await expect(heading(page, "Sign in")).toBeVisible();
}

/** The row of a table, or the card that stands for it on a narrow screen. */
export function itemOf(page: Page, list: string, text: string): Locator {
  return page
    .getByRole("table", { name: list })
    .getByRole("row")
    .filter({ hasText: text })
    .or(page.getByRole("list", { name: list }).getByRole("listitem").filter({ hasText: text }));
}

/** Whether the value has the form, said without showing the value. */
export function expectForm(value: string, form: RegExp, what: string): void {
  expect(form.test(value), `${what} has the form ${String(form)}`).toBe(true);
}

/**
 * Whether the text is anywhere the page keeps something: the document and
 * the values of its fields, the address, the storage of the browser and the
 * cookies a script can read. Said without showing the text.
 */
export async function expectNowhere(page: Page, text: string, what: string): Promise<void> {
  const found = await page.evaluate((secret) => {
    const fields = [...document.querySelectorAll("input, textarea")].map((field) =>
      field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement ? field.value : "",
    );
    const stored = (storage: Storage) =>
      Object.keys(storage).flatMap((key) => [key, storage.getItem(key) ?? ""]);
    const places: Record<string, string[]> = {
      document: [document.documentElement.outerHTML],
      fields,
      address: [window.location.href, document.referrer],
      history: [JSON.stringify(window.history.state)],
      localStorage: stored(window.localStorage),
      sessionStorage: stored(window.sessionStorage),
      cookies: [document.cookie],
    };
    return Object.entries(places)
      .filter(([, values]) => values.some((value) => value.includes(secret)))
      .map(([place]) => place);
  }, text);
  expect(found, `${what} is kept nowhere`).toEqual([]);
}

// The forms of the secrets and texts of the gateway, as `src/test/fixtures.test.ts` pins them.
export const KEY_SECRET = /^uf-sk-[0-9a-f]{64}$/;
export const KEY_DISPLAY = /^uf-sk-…[0-9a-f]{4}$/;
export const TOKEN_SECRET = /^uf-at-[0-9a-f]{64}$/;
export const TOKEN_DISPLAY = /^uf-at-…[0-9a-f]{4}$/;
export const INVITE_PATH = /^\/accept-invite\?token=uf-inv-[0-9a-f]{64}$/;
