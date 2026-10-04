import { expect, goTo, goToAuditLog, heading, listOf, signInFromStart, test } from "./fixtures";

test("the session lifetime is changed in Settings and applies to the next sign-in; the proxies and limits are shown, and the audit log is a view of the page", async ({
  page,
  admin,
  apiAs,
  playwright,
  gateway,
}) => {
  const api = await apiAs(admin);
  const mia = await api.activeUser("Mia");
  await signInFromStart(page, admin);
  await goTo(page, "Settings");

  const section = page.getByRole("region", { name: "Sign-in" });
  await expect(section.getByLabel("Session lifetime (hours)")).toHaveValue("12");
  await expect(section).toContainText("None. Forwarding headers are ignored.");
  await expect(section).toContainText(
    "A sign-in is refused after 5 failed attempts for one email, or 20 from one address, within 15 minutes.",
  );
  // Only the lifetime is a field.
  await expect(section.getByRole("textbox")).toHaveCount(1);

  await section.getByLabel("Session lifetime (hours)").fill("2");
  await section.getByRole("button", { name: "Save sign-in settings" }).click();
  await expect(page.getByText("Settings saved.")).toBeVisible();
  await page.reload();
  await expect(page.getByLabel("Session lifetime (hours)")).toHaveValue("2");

  // A sign-in from now on lives two hours; the session of the admin is as it was.
  const request = await playwright.request.newContext({ baseURL: gateway.origin });
  try {
    const answer = await request.post("/api/auth/login", {
      data: { email: mia.email, password: mia.password },
    });
    expect(answer.status()).toBe(200);
    expect(answer.headers()["set-cookie"]).toContain("Max-Age=7200");
  } finally {
    await request.dispose();
  }

  // The retention is still its own form.
  await page.getByLabel("Keep request logs for (days)").fill("10");
  await page.getByRole("button", { name: "Save retention" }).click();
  await expect(page.getByText("Settings saved.")).toBeVisible();
  const settings = (await api.get("/api/settings")) as Record<string, unknown>;
  expect(settings).toMatchObject({ log_retention_days: 10, session_hours: 2 });

  // The audit log is under Settings, with what was just done.
  await goToAuditLog(page);
  await expect(page).toHaveURL(/\/settings#audit$/);
  await expect(listOf(page, "Audit log")).toContainText("Set session lifetime to 2 hours");
  await expect(heading(page, "Settings")).toBeVisible();
  // The old address leads there.
  await page.goto("/audit");
  await expect(page).toHaveURL(/\/settings#audit$/);
  await expect(listOf(page, "Audit log")).toBeVisible();
});
