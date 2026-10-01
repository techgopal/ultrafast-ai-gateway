import AxeBuilder from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";
import { expect, goTo, heading, test, typeSecret } from "./fixtures";

/** What axe finds wrong with the page as it is, one line per element. No rule is turned off. */
async function axeProblems(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).analyze();
  // It looked at the page: the rules that apply to it passed or failed.
  expect(result.passes.length).toBeGreaterThan(10);
  return result.violations.flatMap((violation) =>
    violation.nodes.map((node) => `${violation.id} at ${node.target.join(" ")}`),
  );
}

/**
 * Marks the controls in `scope` that Tab should reach, in the order of the
 * document, and remembers how each control looks without the focus. Returns
 * how many stops there are. The control that has the focus is left for the
 * moment this takes, and has it again after.
 *
 * A stop can be a group that passes the focus on to one of its controls: a
 * radio group of Radix is one stop, and gives the focus to its checked radio.
 */
async function markTabStops(scope: Locator): Promise<number> {
  return scope.evaluate(async (root) => {
    const candidates = [
      ...root.querySelectorAll<HTMLElement>(
        "a[href], button, input, select, textarea, [tabindex], [contenteditable]",
      ),
    ];
    const stops = candidates.filter(
      (element) =>
        element.tabIndex >= 0 &&
        !element.matches(":disabled") &&
        element.closest("[inert]") === null &&
        element.checkVisibility({ visibilityProperty: true }),
    );
    const active = document.activeElement;
    if (active instanceof HTMLElement) active.blur();
    // A ring that fades out is still there: its look is the one after the fade.
    const fading = document.getAnimations().filter((animation) => animation instanceof CSSTransition);
    await Promise.all(fading.map((animation) => animation.finished.catch(() => undefined)));
    const looks = new Map<Element, string>();
    for (const element of candidates) {
      const style = getComputedStyle(element);
      looks.set(element, `${style.outlineStyle} ${style.outlineWidth} ${style.boxShadow}`);
    }
    if (active instanceof HTMLElement) active.focus({ preventScroll: true });
    Reflect.set(window, "__ufTabStops", { stops, looks });
    return stops.length;
  });
}

interface Marked {
  stops: Element[];
  looks: Map<Element, string>;
}

/** Which of the marked stops has the focus, itself or in it: its place, or -1. */
async function focusedStop(page: Page): Promise<number> {
  return page.evaluate(() => {
    const { stops } = Reflect.get(window, "__ufTabStops") as Marked;
    const active = document.activeElement;
    if (active === null) return -1;
    return stops.findIndex((stop) => stop === active || stop.contains(active));
  });
}

/** Whether the control with the focus looks different from how it looks without it. */
async function focusShows(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const { looks } = Reflect.get(window, "__ufTabStops") as Marked;
    const active = document.activeElement;
    if (active === null) return false;
    const style = getComputedStyle(active);
    const look = `${style.outlineStyle} ${style.outlineWidth} ${style.boxShadow}`;
    return looks.has(active) && look !== looks.get(active);
  });
}

/**
 * Presses Tab `count` times from where the focus is now, and returns the
 * place of the stop that has the focus after each press (-1: none of them).
 * Each stop that gets the focus must show it: it looks different than
 * without it.
 */
async function tabThrough(page: Page, scope: Locator, count: number): Promise<number[]> {
  const order: number[] = [];
  for (let press = 0; press < count; press += 1) {
    await page.keyboard.press("Tab");
    const place = await focusedStop(page);
    order.push(place);
    if (place >= 0) {
      await expect
        .poll(() => focusShows(page), { message: `the focus shows on stop ${String(place)}` })
        .toBe(true);
    }
  }
  // Tab has not closed what the stops are in.
  expect(await scope.count()).toBe(1);
  return order;
}

const inOrder = (count: number) => Array.from({ length: count }, (_, index) => index);

/**
 * Tab reaches every control of the page once, in the order of the document,
 * and leaves the page after the last one. Where the first Tab starts depends
 * on where the focus was last (the browser remembers it), so the cycle is
 * read from wherever it starts: the page's controls, then outside the page.
 */
async function checkTabOrder(page: Page): Promise<void> {
  const body = page.locator("body");
  const count = await markTabStops(body);
  expect(count).toBeGreaterThan(0);
  const order = await tabThrough(page, body, count + 1);
  const outside = order.indexOf(-1);
  const cycle = [...order.slice(outside + 1), ...order.slice(0, outside + 1)];
  expect(cycle).toEqual([...inOrder(count), -1]);
}

for (const scheme of ["light", "dark"] as const) {
  test(`keyboard and automated checks: ${scheme}`, async ({ page, admin, apiAs }) => {
    const api = await apiAs(admin);
    await api.activeUser("Mia");
    await api.send("POST", "/api/keys", { name: "a first key" });
    await page.emulateMedia({ colorScheme: scheme });

    // Sign-in.
    await page.goto("/sign-in");
    await expect(heading(page, "Sign in")).toBeVisible();
    await checkTabOrder(page);
    expect(await axeProblems(page)).toEqual([]);

    // Sent with Enter from the password field.
    const form = page.getByRole("form", { name: "Sign in" });
    await typeSecret(form.getByLabel("Email", { exact: true }), admin.email);
    await typeSecret(form.getByLabel("Password", { exact: true }), admin.password);
    await page.keyboard.press("Enter");
    await expect(heading(page, "Overview")).toBeVisible();
    await expect(page.getByRole("main")).toContainText("Get started");
    await checkTabOrder(page);
    expect(await axeProblems(page)).toEqual([]);

    await goTo(page, "Users");
    await expect(page.getByRole("main")).toContainText("Mia");
    await checkTabOrder(page);
    expect(await axeProblems(page)).toEqual([]);

    await goTo(page, "Virtual keys");
    await expect(page.getByRole("main")).toContainText("a first key");
    await checkTabOrder(page);
    expect(await axeProblems(page)).toEqual([]);

    // The create-key dialog, opened from the keyboard: the focus stays in it.
    const opener = page.getByRole("button", { name: "Create key" });
    await opener.focus();
    await page.keyboard.press("Enter");
    const dialog = page.getByRole("dialog", { name: "Create key" });
    await expect(dialog.getByLabel("Owner", { exact: true })).toBeVisible();
    await expect(dialog.getByLabel("Name", { exact: true })).toBeFocused();
    const count = await markTabStops(dialog);
    expect(count).toBeGreaterThan(3);
    // The first control has the focus; Tab goes through the others and comes back to it.
    expect(await focusedStop(page)).toBe(0);
    const forward = await tabThrough(page, dialog, count);
    expect(forward).toEqual([...inOrder(count).slice(1), 0]);
    await page.keyboard.press("Shift+Tab");
    expect(await focusedStop(page)).toBe(count - 1);
    expect(await axeProblems(page)).toEqual([]);

    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(opener).toBeFocused();
  });
}
