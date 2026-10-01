import { expect, goTo, heading, openNavigation, signInFromStart, test } from "./fixtures";

test("the only admin cannot disable themselves: the dialog says why", async ({ page, admin }) => {
  await signInFromStart(page, admin);
  await goTo(page, "Users");
  await page.getByRole("link", { name: "Admin", exact: true }).click();
  await expect(heading(page, "Admin")).toBeVisible();

  await page.getByRole("group", { name: "Actions" }).getByRole("button", { name: "Disable" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Disable this user?" });
  await expect(dialog).toContainText(
    "You will be signed out and cannot sign in again until another admin enables your account.",
  );
  await dialog.getByRole("button", { name: "Disable" }).click();

  // The message of the API, in the dialog, which stays open with the focus on it.
  const refusal = dialog.getByRole("alert");
  await expect(refusal).toHaveText("At least one active admin is required.");
  await expect(refusal).toBeFocused();
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();

  // The same from the keyboard: the message has the focus, and shows it.
  // (After a pointer the browser shows no ring there, as `:focus-visible` is meant to.)
  const disable = page.getByRole("group", { name: "Actions" }).getByRole("button", { name: "Disable" });
  await disable.focus();
  await page.keyboard.press("Enter");
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Disable" }).focus();
  await page.keyboard.press("Enter");
  await expect(refusal).toBeFocused();
  const ring = await refusal.evaluate((element) => ({
    visible: element.matches(":focus-visible"),
    shadow: getComputedStyle(element).boxShadow,
  }));
  expect(ring.visible).toBe(true);
  expect(ring.shadow).not.toBe("none");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(disable).toBeFocused();

  // Still an active admin, also after a reload.
  const details = page.getByRole("definition");
  await expect(details.filter({ hasText: /^Admin$/ })).toHaveCount(1);
  await expect(details.filter({ hasText: /^active$/ })).toHaveCount(1);
  await page.reload();
  await expect(heading(page, "Admin")).toBeVisible();
  const nav = await openNavigation(page);
  await expect(nav.getByRole("link", { name: "Audit log" })).toBeVisible();
});
