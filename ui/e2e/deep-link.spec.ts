import { expect, heading, signIn, test } from "./fixtures";

test("a deep link survives the sign-in, and a page of a team survives a reload", async ({
  page,
  admin,
  apiAs,
}) => {
  const api = await apiAs(admin);
  const teamId = await api.createTeam("Platform");

  await page.goto("/keys");
  await expect(heading(page, "Sign in")).toBeVisible();
  await expect(page).toHaveURL(/\/sign-in\?next=%2Fkeys$/);
  await signIn(page, admin);
  await expect(heading(page, "Virtual keys")).toBeVisible();
  await expect(page).toHaveURL(/\/keys$/);

  await page.goto(`/teams/${String(teamId)}`);
  await expect(heading(page, "Platform")).toBeVisible();
  await page.reload();
  await expect(heading(page, "Platform")).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`/teams/${String(teamId)}$`));
});
