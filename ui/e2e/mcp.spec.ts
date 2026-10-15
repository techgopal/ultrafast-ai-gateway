import { expect, goTo, itemOf, signInFromStart, signOut, test } from "./fixtures";
import { startMockMcp, type MockMcp } from "./mock-mcp";

let mock: MockMcp;

test.beforeEach(async () => {
  mock = await startMockMcp();
});

test.afterEach(async () => {
  await mock.close();
});

test("an admin registers and syncs a server, turns a tool on and grants it; a lead limits a key to it; a member sees it", async ({
  page,
  admin,
  apiAs,
}) => {
  const api = await apiAs(admin);
  const team = await api.createTeam("Alpha");
  const lead = await api.activeUser("Lea");
  const member = await api.activeUser("Mia");
  await api.putMember(team, lead.id, "lead");
  await api.putMember(team, member.id, "member");

  // The admin adds the server, pointing at the mock.
  await signInFromStart(page, admin);
  await goTo(page, "MCP tools");
  await page.getByRole("button", { name: "Add server" }).click();
  const add = page.getByRole("dialog", { name: "Add MCP server" });
  await add.getByLabel("Name", { exact: true }).fill("mock");
  await add.getByLabel("Address", { exact: true }).fill(mock.url);
  await add.getByRole("button", { name: "Add server" }).click();
  await expect(add).toBeHidden();
  await expect(page.getByText("Server added. Sync it to read its tools.")).toBeVisible();
  const server = itemOf(page, "MCP servers", "mock");
  await expect(server).toContainText("Not synced yet");

  // The sync reads every page of the tools; they start off.
  await server.getByRole("button", { name: "Sync" }).click();
  await expect(page.getByText("Synced: added 3, updated 0, removed 0, skipped 0.")).toBeVisible();
  expect(mock.methods.filter((method) => method === "tools/list")).toHaveLength(2);
  const echo = itemOf(page, "Tools of mock", "echo");
  await expect(echo).toContainText("Off");
  await expect(echo).toContainText("No one");

  // One tool is switched on, and granted to the team.
  await echo.getByRole("switch", { name: "echo on" }).click();
  await expect(echo).toContainText("On");
  await expect(echo).toContainText("On, but nobody has access yet.");
  await echo.getByRole("button", { name: "Edit access" }).click();
  const access = page.getByRole("dialog", { name: "Edit access" });
  await access.getByRole("checkbox", { name: "Alpha" }).check();
  await access.getByRole("button", { name: "Save access" }).click();
  await expect(access).toBeHidden();
  await expect(page.getByText("Access updated.")).toBeVisible();
  await expect(echo).toContainText("1 team");
  await expect(itemOf(page, "Tools of mock", "add")).toContainText("Off");

  // The lead makes a key limited to the tool.
  await signOut(page);
  await signInFromStart(page, lead);
  await goTo(page, "Virtual keys");
  await page.getByRole("button", { name: "Create key" }).click();
  const create = page.getByRole("dialog", { name: "Create key" });
  await create.getByLabel("Name", { exact: true }).fill("agent");
  await create.getByRole("radio", { name: "Only these" }).check();
  const tools = create.getByRole("group", { name: "MCP tools" });
  // Only the tool that is on and granted is offered.
  await expect(tools.getByRole("checkbox")).toHaveCount(1);
  await tools.getByRole("checkbox", { name: "mcp:mock/echo" }).check();
  await create.getByRole("button", { name: "Create key" }).click();
  const shown = page.getByRole("dialog", { name: "Your new key" });
  await expect(shown).toBeVisible();
  await shown.getByRole("button", { name: "Done" }).click();
  await page
    .getByRole("alertdialog", { name: "Close this dialog?" })
    .getByRole("button", { name: "Close" })
    .click();
  await expect(shown).toBeHidden();
  const keys = (await (await apiAs(lead)).get("/api/keys")) as {
    keys: { name: string; allowed: string[] | null }[];
  };
  expect(keys.keys.find((key) => key.name === "agent")?.allowed).toEqual(["mcp:mock/echo"]);

  // The member sees the tool they may call, and nothing to change.
  await signOut(page);
  await signInFromStart(page, member);
  await goTo(page, "MCP tools");
  const list = page.getByRole("list", { name: "Tools of mock" });
  await expect(list.getByRole("listitem")).toHaveCount(1);
  await expect(list).toContainText("mcp:mock/echo");
  await expect(page.getByRole("switch")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Sync" })).toHaveCount(0);
});
