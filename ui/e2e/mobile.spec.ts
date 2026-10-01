import type { Page } from "@playwright/test";
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
