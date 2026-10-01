import {
  expect,
  expectForm,
  expectNowhere,
  goTo,
  heading,
  itemOf,
  signIn,
  signInFromStart,
  test,
  TOKEN_DISPLAY,
  TOKEN_SECRET,
  typeSecret,
} from "./fixtures";

// The session of the first page is ended by the gateway: the same user
// changes their password in a second browser, and the gateway ends every
// other session and every access token of the user (`change_password` in
// crates/gateway/src/api/auth.rs).
test("a session the gateway ended sends the next action to sign-in, and back", async ({
  page,
  admin,
  newContext,
  request,
}) => {
  await signInFromStart(page, admin);
  await goTo(page, "Virtual keys");
  await expect(page).toHaveURL(/\/keys$/);

  const other = await (await newContext()).newPage();
  await signInFromStart(other, admin);
  await goTo(other, "Account");

  // An access token of the user, which the password change ends as well.
  await other.getByRole("button", { name: "Create token" }).click();
  const create = other.getByRole("dialog", { name: "Create token" });
  await create.getByLabel("Name", { exact: true }).fill("deploy script");
  await create.getByRole("button", { name: "Create token" }).click();
  const shown = other.getByRole("dialog", { name: "Your new access token" });
  await shown.getByRole("button", { name: "Copy" }).click();
  await expect(shown.getByRole("status")).toHaveText("Copied");
  const token = await other.evaluate(() => navigator.clipboard.readText());
  expectForm(token, TOKEN_SECRET, "the access token");
  await shown.getByRole("button", { name: "Done" }).click();
  await other
    .getByRole("alertdialog", { name: "Close this dialog?" })
    .getByRole("button", { name: "Close" })
    .click();
  await expect(shown).toBeHidden();
  await expectNowhere(other, token, "the access token, once its dialog is closed,");
  const display = `uf-at-…${token.slice(-4)}`;
  expectForm(display, TOKEN_DISPLAY, "the display of the token");
  const row = itemOf(other, "Access tokens", "deploy script");
  await expect(row).toContainText(display);
  await expect(row).toContainText("active");
  const asToken = () =>
    request.get("/api/auth/me", { headers: { authorization: `Bearer ${token}` } });
  expect((await asToken()).status()).toBe(200);

  const newPassword = `${admin.password}-new`;
  const form = other.getByRole("form", { name: "Change password" });
  await typeSecret(form.getByLabel("Current password", { exact: true }), admin.password);
  await typeSecret(form.getByLabel("New password", { exact: true }), newPassword);
  await typeSecret(form.getByLabel("Confirm new password", { exact: true }), newPassword);
  await form.getByRole("button", { name: "Change password" }).click();
  await expect(
    other.getByText("Password changed. Your other sessions and all your access tokens were ended."),
  ).toBeVisible();
  // The console shows the token revoked: the API gives its times, not a status.
  await expect(row).toContainText("revoked");
  expect((await asToken()).status()).toBe(401);

  // The next action of the first page asks the gateway, which answers 401.
  await page.getByRole("button", { name: "Create key" }).click();
  await expect(heading(page, "Sign in")).toBeVisible();
  await expect(page.getByText("Your session ended. Sign in again.")).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page).toHaveURL(/\/sign-in\?next=%2Fkeys$/);

  await signIn(page, { email: admin.email, password: newPassword });
  await expect(heading(page, "Virtual keys")).toBeVisible();
  await expect(page).toHaveURL(/\/keys$/);
});
