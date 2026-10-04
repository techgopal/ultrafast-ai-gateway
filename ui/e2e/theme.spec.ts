import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import {
  closeNavigation,
  expect,
  goTo,
  heading,
  openNavigation,
  signInFromStart,
  test,
} from "./fixtures";

/**
 * Loads a page with the app's script answered empty, so that only
 * `theme.js`, which the page loads first in its head, runs. What the page
 * shows then is its first paint: the theme is known before the app is.
 */
async function firstPaint(page: Page, path: string) {
  await page.route("**/assets/*.js", (route) =>
    route.fulfill({ status: 200, contentType: "text/javascript", body: "" }),
  );
  await page.goto(path);
  const seen = await page.evaluate(() => ({
    dark: document.documentElement.classList.contains("dark"),
    colorScheme: document.documentElement.style.colorScheme,
    appRan: (document.getElementById("root")?.childElementCount ?? 0) > 0,
  }));
  await page.unroute("**/assets/*.js");
  expect(seen.appRan, "the app's script did not run").toBe(false);
  return seen;
}

async function isDark(page: Page): Promise<boolean> {
  return page.evaluate(() => document.documentElement.classList.contains("dark"));
}

async function contrastProblems(page: Page) {
  const result = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  return result.violations.flatMap((violation) =>
    violation.nodes.map((node) => `${node.target.join(" ")}: ${node.failureSummary ?? ""}`),
  );
}

for (const scheme of ["light", "dark"] as const) {
  test(`the first paint follows the device setting: ${scheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    const seen = await firstPaint(page, "/sign-in");
    expect(seen.dark).toBe(scheme === "dark");
    expect(seen.colorScheme).toBe(scheme);

    // The app, once it runs, keeps what the first paint showed.
    await page.reload();
    await expect(heading(page, "Sign in")).toBeVisible();
    expect(await isDark(page)).toBe(scheme === "dark");
  });
}

test("Dark, once chosen, lasts over a reload, whatever the device says", async ({ page, admin }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await signInFromStart(page, admin);
  expect(await isDark(page)).toBe(false);

  const nav = await openNavigation(page);
  const dark = nav.getByRole("group", { name: "Theme" }).getByRole("button", { name: "Dark" });
  await dark.click();
  await expect(dark).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => isDark(page)).toBe(true);
  expect(await page.evaluate(() => localStorage.getItem("uf-theme"))).toBe("dark");

  const seen = await firstPaint(page, "/");
  expect(seen.dark).toBe(true);
  await page.reload();
  await expect(heading(page, "Overview")).toBeVisible();
  expect(await isDark(page)).toBe(true);
  const navAgain = await openNavigation(page);
  await expect(
    navAgain.getByRole("group", { name: "Theme" }).getByRole("button", { name: "Dark" }),
  ).toHaveAttribute("aria-pressed", "true");
  await closeNavigation(page);
});

for (const scheme of ["light", "dark"] as const) {
  test(`the text of overview and keys has enough contrast: ${scheme}`, async ({
    page,
    admin,
    apiAs,
  }) => {
    const api = await apiAs(admin);
    await api.send("POST", "/api/providers", {
      name: "upstream",
      kind: "openai",
      base_url: "https://api.example.test/v1",
    });
    await api.send("POST", "/api/keys", { name: "contrast check" });
    await page.emulateMedia({ colorScheme: scheme });

    await signInFromStart(page, admin);
    expect(await isDark(page)).toBe(scheme === "dark");
    await expect(page.getByRole("main")).toContainText("with a credential");
    expect(await contrastProblems(page)).toEqual([]);

    await goTo(page, "Virtual keys");
    await expect(page.getByRole("main")).toContainText("contrast check");
    expect(await contrastProblems(page)).toEqual([]);
  });
}
