import type { Locator, Page } from "@playwright/test";
import {
  expect,
  goTo,
  heading,
  menuButton,
  signInFromStart,
  test,
  type GatewayApi,
} from "./fixtures";

// A phone, in both projects: the desktop project has no touch, the phone project has.
test.use({ viewport: { width: 390, height: 844 } });

async function scrollsSideways(page: Page): Promise<boolean> {
  return page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
  );
}

/** Waits for what moves to end: a dialog that opens grows into its size. Endless ones are left. */
async function settled(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const ending = document
      .getAnimations()
      .filter((animation) => animation.effect?.getComputedTiming().endTime !== Infinity);
    await Promise.all(ending.map((animation) => animation.finished.catch(() => undefined)));
  });
}

/**
 * The controls in `scope` that are less than 44 x 44 px to touch, whatever
 * they show: a short text makes a narrow link. A control inside its label is
 * touched through the label as well: its target is the label.
 */
async function smallTargets(scope: Locator): Promise<string[]> {
  await settled(scope.page());
  return scope.evaluate((root) => {
    const controls = root.querySelectorAll<HTMLElement>(
      "a[href], button, input, select, textarea, [role=checkbox], [role=radio], [role=combobox]",
    );
    const small: string[] = [];
    for (const control of controls) {
      // Not for the user: hidden, or a copy for the form of the browser.
      if (!control.checkVisibility({ visibilityProperty: true, opacityProperty: true })) continue;
      if (control.closest('[aria-hidden="true"]') !== null) continue;
      const target = control.closest("label") ?? control;
      const { width, height } = target.getBoundingClientRect();
      const text = target.textContent.trim();
      if (height >= 44 && width >= 44) continue;
      const role = control.getAttribute("role") ?? control.tagName.toLowerCase();
      const labels = control instanceof HTMLButtonElement ? [...control.labels] : [];
      const label = labels.map((one) => one.textContent.trim()).join(" ");
      const name = control.getAttribute("aria-label") ?? (label === "" ? text : label).slice(0, 40);
      small.push(`${role} "${name}": ${String(Math.round(width))} x ${String(Math.round(height))}`);
    }
    return small;
  });
}

/** Touches the element near its top right corner: where neither a box nor a text is. */
async function touchEdge(target: Locator): Promise<void> {
  await settled(target.page());
  const box = await target.boundingBox();
  expect(box).not.toBeNull();
  if (box === null) return;
  expect(box.height).toBeGreaterThanOrEqual(44);
  await target.click({ position: { x: box.width - 2, y: 2 } });
}

/** Long texts, which a narrow screen must wrap: a page is no wider for them. */
async function content(api: GatewayApi) {
  const user = await api.activeUser("Christina-Alexandra Montgomery-Weatherby");
  const team = await api.createTeam("Platform infrastructure and developer experience");
  await api.putMember(team, user.id, "lead");
  await api.send("POST", "/api/providers", {
    name: "a-provider-with-a-name-of-forty-letters",
    kind: "openai",
    base_url: "https://a-rather-long-host-name.inference.example.test/api/openai/compatible/v1",
  });
  await api.send("POST", "/api/keys", {
    name: "The key of the nightly batch job that summarises support tickets",
    owner_id: user.id,
    team_id: team,
  });
}

test("every page fits a phone; the drawer opens and closes; rows are cards; the dialog fits", async ({
  page,
  admin,
  apiAs,
}) => {
  await content(await apiAs(admin));

  await page.goto("/sign-in");
  await expect(heading(page, "Sign in")).toBeVisible();
  expect(await scrollsSideways(page), "sign-in").toBe(false);

  await signInFromStart(page, admin);
  expect(await scrollsSideways(page), "overview").toBe(false);

  // The menu button opens the drawer; Escape closes it, and the focus goes back.
  const drawer = page.getByRole("dialog", { name: "Sidebar" });
  await expect(drawer).toHaveCount(0);
  await menuButton(page).click();
  await expect(drawer).toBeVisible();
  await expect(drawer.getByRole("navigation", { name: "Main" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
  await expect(menuButton(page)).toBeFocused();
  // A link of the drawer closes it too.
  await goTo(page, "Users");
  await expect(drawer).toBeHidden();

  // Each page with what it lists, so that it is measured with its content.
  const pages: [string, string][] = [
    ["Users", "Montgomery-Weatherby"],
    ["Teams", "developer experience"],
    ["Virtual keys", "nightly batch job"],
    ["Providers", "a-rather-long-host-name"],
    ["Account", "Access tokens"],
    ["Audit log", "auth.login"],
  ];
  for (const [title, shows] of pages) {
    await goTo(page, title);
    await expect(page.getByRole("main")).toContainText(shows);
    expect(await scrollsSideways(page), title).toBe(false);
  }

  // A row of a table is a card.
  await goTo(page, "Virtual keys");
  await expect(page.getByRole("table")).toHaveCount(0);
  const card = page
    .getByRole("list", { name: "Virtual keys" })
    .getByRole("listitem")
    .filter({ hasText: "nightly batch job" });
  await expect(card).toBeVisible();
  await expect(card.getByRole("term")).toHaveText(["Name", "Key", "Owner", "Team", "Expires", "Status"]);

  // The create-key dialog fits the screen, and its submit button can be reached.
  await page.getByRole("button", { name: "Create key" }).click();
  const dialog = page.getByRole("dialog", { name: "Create key" });
  await expect(dialog.getByLabel("Owner", { exact: true })).toBeVisible();
  const box = await dialog.boundingBox();
  expect(box).not.toBeNull();
  if (box !== null) {
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(390);
    expect(box.y + box.height).toBeLessThanOrEqual(844);
  }
  const submit = dialog.getByRole("button", { name: "Create key" });
  await submit.scrollIntoViewIfNeeded();
  await expect(submit).toBeInViewport({ ratio: 1 });
  await submit.click({ trial: true });
  expect(await scrollsSideways(page), "the dialog").toBe(false);
});

test("every control of the pages and their dialogs is 44 x 44 px to touch", async ({
  page,
  admin,
  apiAs,
}) => {
  await content(await apiAs(admin));
  // What is too small, everywhere: one list, so that a failure names every place.
  const found: string[] = [];
  const measure = async (scope: Locator, where: string) => {
    for (const small of await smallTargets(scope)) found.push(`${where}: ${small}`);
  };
  await page.goto("/sign-in");
  await expect(heading(page, "Sign in")).toBeVisible();
  await measure(page.locator("body"), "sign-in");
  await signInFromStart(page, admin);
  await measure(page.locator("body"), "overview");
  await menuButton(page).click();
  const drawer = page.getByRole("dialog", { name: "Sidebar" });
  await expect(drawer).toBeVisible();
  await measure(drawer, "the drawer");
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();

  /** Opens a dialog with a button of the page, measures it, and closes it with Escape. */
  async function measureDialog(opener: string, name: string = opener, waitFor?: string) {
    await page.getByRole("button", { name: opener, exact: true }).first().click();
    const dialog = page.getByRole("dialog", { name });
    await expect(dialog).toBeVisible();
    if (waitFor !== undefined) await expect(dialog.getByLabel(waitFor, { exact: true })).toBeVisible();
    await measure(dialog, `the dialog ${name}`);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
  }

  const pages: [string, string][] = [
    ["Users", "Montgomery-Weatherby"],
    ["Teams", "developer experience"],
    ["Virtual keys", "nightly batch job"],
    ["Providers", "a-rather-long-host-name"],
    ["Account", "Access tokens"],
    ["Audit log", "auth.login"],
  ];
  for (const [title, shows] of pages) {
    await goTo(page, title);
    await expect(page.getByRole("main")).toContainText(shows);
    await measure(page.locator("body"), title);
    if (title === "Users") await measureDialog("Invite user");
    if (title === "Teams") await measureDialog("New team");
    if (title === "Virtual keys") await measureDialog("Create key", "Create key", "Owner");
    if (title === "Providers") {
      await measureDialog("Add provider");
      await measureDialog("Edit", "Edit provider");
    }
    if (title === "Account") await measureDialog("Create token");
  }

  // The page of a team, with the dialog that adds a member from the list of users.
  await goTo(page, "Teams");
  await page.getByRole("link", { name: "Platform infrastructure and developer experience" }).click();
  await expect(heading(page, "Platform infrastructure and developer experience")).toBeVisible();
  await measure(page.locator("body"), "a team");
  await measureDialog("Add member");

  // Every place is named before a touch is tried.
  expect(found).toEqual([]);

  // A touch at the edge of a label, away from its text and its box, chooses.
  await goTo(page, "Virtual keys");
  const showRevoked = page.locator("label").filter({ hasText: "Show revoked" });
  await touchEdge(showRevoked);
  await expect(page.getByRole("checkbox", { name: "Show revoked" })).toBeChecked();
  await page.getByRole("button", { name: "Create key", exact: true }).click();
  const create = page.getByRole("dialog", { name: "Create key" });
  await expect(create.getByLabel("Owner", { exact: true })).toBeVisible();
  const inDays = create.getByRole("radio", { name: "In 30 days" });
  await expect(inDays).not.toBeChecked();
  await touchEdge(create.locator("label").filter({ hasText: "In 30 days" }));
  await expect(inDays).toBeChecked();
  await page.keyboard.press("Escape");
  await expect(create).toBeHidden();
});
