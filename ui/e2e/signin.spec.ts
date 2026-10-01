import { expect, expectNowhere, heading, signIn, signOut, test } from "./fixtures";

test("sign in, stay signed in over a reload, sign out", async ({ page, admin, context }) => {
  await page.goto("/");
  await expect(heading(page, "Sign in")).toBeVisible();

  // A wrong password: the message, and still the sign-in page.
  await signIn(page, { email: admin.email, password: `${admin.password}-wrong` });
  await expect(page.getByRole("alert")).toHaveText("Email or password is incorrect.");
  await expect(heading(page, "Sign in")).toBeVisible();
  // The password field is emptied after the attempt.
  await expect(page.getByLabel("Password", { exact: true })).toHaveValue("");

  await signIn(page, admin);
  await expect(heading(page, "Overview")).toBeVisible();

  await page.reload();
  await expect(heading(page, "Overview")).toBeVisible();

  // The page keeps neither the password nor the session: the cookie is
  // HttpOnly, and the CSRF token is held in memory only.
  await expectNowhere(page, admin.password, "the password");
  const [cookie, ...others] = await context.cookies();
  expect(others).toEqual([]);
  expect(cookie?.httpOnly).toBe(true);
  expect(cookie?.sameSite).toBe("Strict");
  await expectNowhere(page, cookie?.value ?? "", "the session cookie");
  const csrf = await page.evaluate(async () => {
    const me = (await (await fetch("/api/auth/me")).json()) as { csrf_token: string };
    return me.csrf_token;
  });
  expect(csrf.length).toBeGreaterThan(0);
  await expectNowhere(page, csrf, "the CSRF token");

  await signOut(page);
  // Who signed out is not sent back to where they were.
  await expect(page).toHaveURL(/\/sign-in$/);

  await page.goto("/keys");
  await expect(heading(page, "Sign in")).toBeVisible();
  await expect(page).toHaveURL(/\/sign-in\?next=%2Fkeys$/);
});
