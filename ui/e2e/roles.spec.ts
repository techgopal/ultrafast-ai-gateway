import type { Page } from "@playwright/test";
import {
  closeNavigation,
  expect,
  goTo,
  heading,
  itemOf,
  openNavigation,
  signInFromStart,
  test,
  type GatewayApi,
} from "./fixtures";

/** A member and a lead, with a team the lead leads and one they are a member of. */
async function people(api: GatewayApi) {
  const member = await api.activeUser("Mia");
  const lead = await api.activeUser("Leo");
  const led = await api.createTeam("Alpha");
  const joined = await api.createTeam("Beta");
  await api.putMember(led, lead.id, "lead");
  await api.putMember(led, member.id, "member");
  await api.putMember(joined, lead.id, "member");
  await api.putMember(joined, member.id, "member");
  await api.send("POST", "/api/providers", {
    name: "upstream",
    kind: "openai",
    base_url: "http://127.0.0.1:9/v1",
  });
  return { member, lead, led, joined };
}

/** The group of buttons of a page of a team or a user. */
function actions(page: Page) {
  return page.getByRole("group", { name: "Actions" });
}

test("a member sees no control of an admin, and no audit log", async ({
  page,
  admin,
  apiAs,
}) => {
  const { member } = await people(await apiAs(admin));
  await signInFromStart(page, member);

  const nav = await openNavigation(page);
  await expect(nav.getByRole("link", { name: "Account" })).toBeVisible();
  await expect(nav.getByRole("link", { name: "Audit log" })).toHaveCount(0);
  await closeNavigation(page);

  await goTo(page, "Users");
  await expect(page.getByRole("main")).toContainText("Mia");
  await expect(page.getByRole("button", { name: "Invite user" })).toHaveCount(
    0,
  );

  await goTo(page, "Teams");
  await expect(page.getByRole("main")).toContainText("Alpha");
  await expect(page.getByRole("button", { name: "New team" })).toHaveCount(0);

  await goTo(page, "Providers");
  await expect(page.getByRole("main")).toContainText("upstream");
  await expect(page.getByRole("button", { name: "Add provider" })).toHaveCount(
    0,
  );
  await expect(page.getByRole("button", { name: "Edit" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Delete" })).toHaveCount(0);

  // Typed into the address bar.
  await page.goto("/audit");
  await expect(heading(page, "Not available")).toBeVisible();
  await expect(
    page.getByText("This page is not available to your account."),
  ).toBeVisible();
});

test("a lead sees the controls of the team they lead, and none of another", async ({
  page,
  admin,
  apiAs,
}) => {
  const { member, lead, led, joined } = await people(await apiAs(admin));
  await signInFromStart(page, lead);

  await goTo(page, "Teams");
  await expect(page.getByRole("button", { name: "New team" })).toHaveCount(0);

  await page.getByRole("link", { name: "Alpha", exact: true }).click();
  await expect(heading(page, "Alpha")).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`/teams/${String(led)}$`));
  await expect(
    actions(page).getByRole("button", { name: "Rename" }),
  ).toBeVisible();
  await expect(
    actions(page).getByRole("button", { name: "Add member" }),
  ).toBeVisible();
  // Deleting a team and making a lead are for an admin.
  await expect(
    actions(page).getByRole("button", { name: "Delete" }),
  ).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Make lead" })).toHaveCount(0);
  // Mia can be removed; the lead himself can only leave.
  await expect(page.getByRole("button", { name: "Remove" })).toHaveCount(1);
  await expect(page.getByRole("button", { name: "Leave team" })).toHaveCount(1);

  // Who is in the team already is not added again: the gateway would make a
  // lead a member. The lead types a member's email, then their own.
  const puts: string[] = [];
  page.on("request", (request) => {
    if (request.method() === "PUT") puts.push(new URL(request.url()).pathname);
  });
  await actions(page).getByRole("button", { name: "Add member" }).click();
  const dialog = page.getByRole("dialog", { name: "Add member" });
  const field = dialog.getByLabel("Email", { exact: true });
  for (const email of [member.email, lead.email]) {
    await field.fill(email);
    await dialog.getByRole("button", { name: "Add member" }).click();
    await expect(field).toHaveAccessibleDescription(/^Already in this team\./);
  }
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  expect(puts).toEqual([]);
  await expect(itemOf(page, "Members", "Leo")).toContainText("Lead");
  await expect(itemOf(page, "Members", "Mia")).toContainText("Member");

  await page.getByRole("link", { name: "Back to teams" }).click();
  await page.getByRole("link", { name: "Beta", exact: true }).click();
  await expect(heading(page, "Beta")).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`/teams/${String(joined)}$`));
  await expect(page.getByRole("main")).toContainText("Mia");
  await expect(actions(page)).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Remove" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Make lead" })).toHaveCount(0);

  // The audit log is for an admin.
  await page.goto("/audit");
  await expect(heading(page, "Not available")).toBeVisible();
});

test("a lead adds a member by email, and neither makes a lead nor removes another lead", async ({
  page,
  admin,
  apiAs,
}) => {
  const api = await apiAs(admin);
  const { lead, led } = await people(api);
  const other = await api.activeUser("Lena");
  await api.putMember(led, other.id, "lead");
  const newcomer = await api.activeUser("Nina");
  await signInFromStart(page, lead);

  await goTo(page, "Teams");
  await page.getByRole("link", { name: "Alpha", exact: true }).click();
  await expect(heading(page, "Alpha")).toBeVisible();
  await expect(itemOf(page, "Members", "Nina")).toHaveCount(0);

  await actions(page).getByRole("button", { name: "Add member" }).click();
  const dialog = page.getByRole("dialog", { name: "Add member" });
  // A lead reads no list of users: the email is typed.
  await expect(dialog.getByLabel("Choose a user")).toHaveCount(0);
  await dialog.getByLabel("Email", { exact: true }).fill(newcomer.email);
  await dialog.getByRole("button", { name: "Add member" }).click();
  await expect(dialog).toBeHidden();
  await expect(itemOf(page, "Members", "Nina")).toContainText("Member");

  // Roles are for an admin; another lead cannot be removed, a member can.
  await expect(page.getByRole("button", { name: "Make lead" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Make member" })).toHaveCount(
    0,
  );
  await expect(itemOf(page, "Members", "Lena")).toContainText("Lead");
  await expect(itemOf(page, "Members", "Lena").getByRole("button")).toHaveCount(
    0,
  );
  await expect(
    itemOf(page, "Members", "Nina").getByRole("button", { name: "Remove" }),
  ).toBeVisible();
  await expect(
    itemOf(page, "Members", "Mia").getByRole("button", { name: "Remove" }),
  ).toBeVisible();
});
