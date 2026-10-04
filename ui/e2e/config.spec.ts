import { readFile } from "node:fs/promises";
import {
  BrowserRules,
  expect,
  GatewayApi,
  goTo,
  listOf,
  newAccount,
  signInFromStart,
  test,
} from "./fixtures";
import { startGateway } from "./gateway";
import { startMockProvider, type MockProvider } from "./mock-provider";

let mock: MockProvider;

test.beforeEach(async () => {
  mock = await startMockProvider(["e2e-model", "e2e-other"]);
});

test.afterEach(async () => {
  await mock.close();
});

test("the configuration of one gateway is exported, imported into a fresh one through the console, and exports the same there", async ({
  page,
  browser,
  playwright,
  admin,
  apiAs,
}) => {
  // A gateway with a provider that has a credential, models, a route, a team
  // with a limit and a budget, and a setting that is not the default.
  const api = await apiAs(admin);
  const provider = await api.addProvider("alpha", mock.baseUrl, mock.apiKey);
  await api.sync(provider);
  const model = await api.open("alpha", "e2e-model");
  await api.setPrice(model, 150_000, 600_000);
  const other = (await api.modelNamed("alpha", "e2e-other")).id;
  const team = await api.createTeam("Platform");
  await api.enable(other);
  await api.grant(other, { team_ids: [team] });
  await api.createRoute({
    name: "main",
    primaries: [
      { model_id: model, weight: 3 },
      { model_id: other, weight: 1 },
    ],
    everyone: true,
    cache_enabled: true,
    cache_scope: "user",
    cache_ttl_s: 120,
  });
  await api.setLimit({ scope: "team", scope_id: team, requests_per_minute: 30 });
  await api.setBudget({
    scope: "gateway",
    amount_micros: 50_000_000,
    period: "monthly",
    action: "alert",
  });
  await api.send("PATCH", "/api/settings", { log_retention_days: 45, session_hours: 8 });

  // Exported from the console: a download.
  await signInFromStart(page, admin);
  await goTo(page, "Settings");
  const downloading = page.waitForEvent("download");
  await page.getByRole("link", { name: "Download configuration" }).click();
  const download = await downloading;
  expect(download.suggestedFilename()).toMatch(/^ultrafast-config-\d{8}-\d{6}\.json$/);
  const path = await download.path();
  const exported = await readFile(path, "utf8");
  // Nothing of a credential is in it.
  expect(exported).not.toContain(mock.apiKey);
  expect(JSON.parse(exported)).toEqual(await api.get("/api/config/export"));

  // A fresh gateway of its own, with its own admin and a browser held to its rules.
  const freshAdmin = newAccount("fresh-admin");
  const second = await startGateway({ admin: freshAdmin });
  const rules = new BrowserRules(second.origin);
  const context = await browser.newContext({ baseURL: second.origin, viewport: page.viewportSize() });
  const request = await playwright.request.newContext({ baseURL: second.origin });
  try {
    await rules.watch(context);
    const there = await context.newPage();
    const freshApi = await GatewayApi.signIn(request, freshAdmin);
    await signInFromStart(there, freshAdmin);
    await goTo(there, "Settings");
    await there.getByLabel("Configuration file").setInputFiles({
      name: "ultrafast-config.json",
      mimeType: "application/json",
      buffer: Buffer.from(exported),
    });

    // The report of what it would do, before anything is written.
    const table = listOf(there, "What the import would do");
    await expect(table).toBeVisible();
    await expect(table).toContainText("alpha");
    await expect(table).toContainText("alpha/e2e-model");
    await expect(table).toContainText("Platform");
    await expect(there.getByText(/provider 'alpha' is created with no credential/)).toBeVisible();
    expect(((await freshApi.get("/api/models")) as { models: unknown[] }).models).toEqual([]);

    // Applied after a question.
    await there.getByRole("button", { name: "Apply import" }).click();
    const question = there.getByRole("alertdialog", { name: "Apply this import?" });
    await question.getByRole("button", { name: "Apply" }).click();
    await expect(there.getByText("Configuration imported.")).toBeVisible();
    await expect(table).toHaveCount(0);

    // Models, routes, teams, limits, budgets and settings are as they were.
    expect(await freshApi.get("/api/config/export")).toEqual(await api.get("/api/config/export"));
    // A provider made by an import has no credential until an admin sets one.
    const providers = (await freshApi.get("/api/providers")) as {
      providers: { name: string; has_credential: boolean }[];
    };
    expect(providers.providers.map((p) => [p.name, p.has_credential])).toEqual([["alpha", false]]);
    expect(mock.listCalls).toBe(1);

    // The same file again has nothing to do.
    await there.getByLabel("Configuration file").setInputFiles({
      name: "ultrafast-config.json",
      mimeType: "application/json",
      buffer: Buffer.from(exported),
    });
    await expect(there.getByText(/Nothing to change/)).toBeVisible();
    await expect(there.getByRole("button", { name: "Apply import" })).toHaveCount(0);
  } finally {
    await request.dispose();
    await context.close();
    await second.stop();
  }
  expect(rules.problems, "console errors, CSP violations, other origins, 5xx").toEqual([]);
});

test("a file that names what the gateway does not have is refused whole, with where and why, and writes nothing", async ({
  page,
  admin,
  apiAs,
}) => {
  const api = await apiAs(admin);
  await signInFromStart(page, admin);
  await goTo(page, "Settings");
  const file = {
    format: "ultrafast-config",
    version: 1,
    teams: [{ name: "Fine" }],
    budgets: [
      { scope: "team", name: "No such team", amount_micros: 1, period: "daily", action: "block" },
    ],
  };
  await page.getByLabel("Configuration file").setInputFiles({
    name: "bad.json",
    mimeType: "application/json",
    buffer: Buffer.from(JSON.stringify(file)),
  });
  const problems = listOf(page, "Problems in the file");
  await expect(problems).toContainText("budgets[0].name");
  await expect(problems).toContainText("team 'No such team' does not exist");
  await expect(page.getByText(/The file has errors. Nothing was written/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Apply import" })).toHaveCount(0);
  const teams = (await api.get("/api/teams")) as { teams: { name: string }[] };
  expect(teams.teams.map((t) => t.name)).toEqual([]);
});
