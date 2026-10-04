import {
  expect,
  expectForm,
  expectNowhere,
  goTo,
  heading,
  INVITE_PATH,
  newAccount,
  openNavigation,
  signIn,
  signInFromStart,
  signOut,
  test,
  typeSecret,
} from "./fixtures";

test("an invited user sets a password from the link and signs in", async ({
  page,
  admin,
  gateway,
}) => {
  const invited = newAccount("sam");
  await signInFromStart(page, admin);

  await goTo(page, "Users");
  await page.getByRole("button", { name: "Invite user" }).click();
  const invite = page.getByRole("dialog", { name: "Invite user" });
  await invite.getByLabel("Name", { exact: true }).fill("Sam Reed");
  await typeSecret(invite.getByLabel("Email", { exact: true }), invited.email);
  await expect(invite.getByRole("radio", { name: "Member" })).toBeChecked();
  await invite.getByRole("button", { name: "Create invite link" }).click();

  // The link, copied from the dialog.
  const shown = page.getByRole("dialog", { name: "Invite link" });
  await expect(shown).toBeVisible();
  await shown.getByRole("button", { name: "Copy" }).click();
  await expect(shown.getByRole("status")).toHaveText("Copied");
  const link = await page.evaluate(() => navigator.clipboard.readText());
  expect(link === (await shown.getByRole("textbox", { name: "Invite link" }).inputValue())).toBe(
    true,
  );
  // The gateway gives a path of the console; the dialog shows it on this origin.
  const url = new URL(link);
  expect(url.origin).toBe(gateway.origin);
  // The token is in the fragment, which the browser never sends to a server.
  expect(url.search).toBe("");
  expectForm(url.pathname + url.hash, INVITE_PATH, "the invite link");
  await shown.getByRole("button", { name: "Done" }).click();
  await page
    .getByRole("alertdialog", { name: "Close this dialog?" })
    .getByRole("button", { name: "Close" })
    .click();
  await expect(shown).toBeHidden();
  await expect(page.getByRole("main")).toContainText("Sam Reed");
  const token = new URLSearchParams(url.hash.slice(1)).get("token") ?? "";
  expect(token).toMatch(/^uf-inv-/);
  await expectNowhere(page, token, "the invite token, once its dialog is closed,");

  await signOut(page);

  // The link is opened as a visitor does. It is opened by the page and not
  // by `goto`, whose step in the report would show the token.
  await page.evaluate((address) => {
    window.location.assign(address);
  }, link);
  await expect(heading(page, "Accept your invite")).toBeVisible();
  const now = new URL(page.url());
  expect(now.pathname).toBe("/accept-invite");
  expect(now.search, "the token is not in the address bar").toBe("");
  expect(now.hash).toBe("");
  await expectNowhere(page, token, "the invite token, on the page that accepts it,");

  const accept = page.getByRole("form", { name: "Accept your invite" });
  await typeSecret(accept.getByLabel("Password", { exact: true }), invited.password);
  await typeSecret(accept.getByLabel("Confirm password", { exact: true }), invited.password);
  await accept.getByRole("button", { name: "Set password" }).click();

  await expect(heading(page, "Sign in")).toBeVisible();
  await expect(page.getByText("Your password is set. Sign in to continue.")).toBeVisible();
  await signIn(page, invited);
  await expect(heading(page, "Overview")).toBeVisible();
  const nav = await openNavigation(page);
  await expect(nav.getByText("Sam Reed", { exact: true })).toBeVisible();
  await expect(nav.getByText("Member", { exact: true })).toBeVisible();
});
