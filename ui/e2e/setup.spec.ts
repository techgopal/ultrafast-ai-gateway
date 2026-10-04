import { expect, heading, openNavigation, signIn, test, typeSecret } from "./fixtures";

test.use({ withAdmin: false });

test("a gateway with no users shows Setup; the admin it creates signs in", async ({ page, admin }) => {
  await page.goto("/");
  await expect(heading(page, "Set up the gateway")).toBeVisible();
  await expect(page).toHaveURL(/\/setup$/);

  const form = page.getByRole("form", { name: "Set up the gateway" });
  await form.getByLabel("Name", { exact: true }).fill("First Admin");
  await typeSecret(form.getByLabel("Email", { exact: true }), admin.email);
  await typeSecret(form.getByLabel("Password", { exact: true }), admin.password);
  await typeSecret(form.getByLabel("Confirm password", { exact: true }), admin.password);
  await form.getByRole("button", { name: "Create admin account" }).click();

  await expect(heading(page, "Sign in")).toBeVisible();
  await expect(page).toHaveURL(/\/sign-in$/);
  await expect(page.getByRole("status")).toHaveText(
    "The admin account is created. Sign in to continue.",
  );

  await signIn(page, admin);
  await expect(heading(page, "Overview")).toBeVisible();
  await expect(page).toHaveURL(/\/$/);
  // The gateway knows the admin by the name given in the form.
  const nav = await openNavigation(page);
  await expect(nav.getByText("First Admin", { exact: true })).toBeVisible();

  // Setup is done: the page is not offered again.
  await page.goto("/setup");
  await expect(heading(page, "Overview")).toBeVisible();
});
