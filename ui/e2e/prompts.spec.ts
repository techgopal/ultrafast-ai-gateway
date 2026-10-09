import type { Page } from "@playwright/test";
import {
  chat,
  expect,
  goTo,
  itemOf,
  signInFromStart,
  test,
  type GatewayApi,
} from "./fixtures";
import { startMockProvider, type MockProvider } from "./mock-provider";

let mock: MockProvider;

test.beforeEach(async () => {
  mock = await startMockProvider(["e2e-model"]);
});

test.afterEach(async () => {
  await mock.close();
});

async function setup(api: GatewayApi) {
  const provider = await api.addProvider("alpha", mock.baseUrl, mock.apiKey);
  await api.sync(provider);
  await api.open("alpha", "e2e-model");
}

async function choose(page: Page, select: string, option: string) {
  await page.getByRole("combobox", { name: select }).click();
  await page.getByRole("option", { name: option, exact: true }).click();
}

test("an admin makes a template and a second version, compares them, uses it in the playground, and the call is logged with its name and version", async ({
  page,
  admin,
  apiAs,
}) => {
  const api = await apiAs(admin);
  await setup(api);
  await signInFromStart(page, admin);
  await goTo(page, "Prompts");

  await page.getByRole("link", { name: "New template" }).click();
  await page.getByLabel("Name", { exact: true }).fill("greeter");
  await page.getByLabel("Message 1", { exact: true }).fill("Say hello to {{name}} in one word.");
  await expect(page.getByRole("status", { name: "Variables" })).toHaveText("Variables: name");
  await page.getByRole("button", { name: "Create template" }).click();

  // The template opens at version 1.
  await expect(page.getByRole("heading", { level: 1, name: "greeter" })).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Version" })).toHaveText("Version 1 (latest)");
  await expect(page.getByRole("list", { name: "Messages of version 1" })).toContainText(
    "Say hello to {{name}} in one word.",
  );

  // On a phone the toast covers the bottom of the screen until it goes.
  await expect(page.getByText("Template created.")).toBeHidden({ timeout: 15_000 });

  // Version 2 changes the message.
  await page.getByLabel("Message 1", { exact: true }).fill("Greet {{name}} warmly.");
  await page.getByRole("button", { name: "Save as version 2" }).click();
  await expect(page.getByRole("combobox", { name: "Version" })).toHaveText("Version 2 (latest)");
  await expect(page.getByRole("button", { name: "Save as version 3" })).toBeVisible();
  await expect(page.getByText("Version saved.")).toBeHidden({ timeout: 15_000 });

  // The difference between the two is told in words.
  await choose(page, "Compare with", "Version 1");
  const changes = page.getByRole("region", { name: "Changes from version 1 to version 2" });
  await expect(changes).toContainText("Removed: Say hello to {{name}} in one word.");
  await expect(changes).toContainText("Added: Greet {{name}} warmly.");

  // Open in the playground: the template and its version are chosen.
  await page.getByRole("link", { name: "Open in Playground" }).click();
  await expect(page).toHaveURL(/\/playground\?prompt=greeter&version=2$/);
  await expect(page.getByRole("combobox", { name: "Prompt template" })).toHaveText("greeter");
  await expect(page.getByRole("combobox", { name: "Template version" })).toHaveText("Version 2");
  await page.getByLabel("Variable: name", { exact: true }).fill("Ada");
  await choose(page, "Model or route", "alpha/e2e-model");
  // The template is the whole message: Send is ready with none of your own.
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByRole("list", { name: "Conversation" })).toContainText(mock.answer);

  // The provider got the rendered messages.
  expect(mock.texts).toEqual([["Greet Ada warmly."]]);
  expect(mock.roles).toEqual([["user"]]);
  await expect.poll(async () => (await api.logs()).length, { timeout: 15_000 }).toBe(1);
  const [row] = await api.logs();
  expect(row).toMatchObject({ endpoint: "playground", prompt: "greeter@2", status: 200 });

  // The logs show it in the prompt column.
  await goTo(page, "Logs");
  await expect(itemOf(page, "Request logs", "greeter@2")).toHaveCount(1);
});

test("the logs are filtered by endpoint", async ({ page, admin, apiAs, request }) => {
  const api = await apiAs(admin);
  await setup(api);
  const key = await api.createKey("k1");
  expect((await chat(request, key, "alpha/e2e-model")).status()).toBe(200);
  await signInFromStart(page, admin);
  await goTo(page, "Playground");
  await choose(page, "Model or route", "alpha/e2e-model");
  await page.getByRole("textbox", { name: "Message" }).fill("Hi");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByRole("list", { name: "Conversation" })).toContainText(mock.answer);
  await expect.poll(async () => (await api.logs()).length, { timeout: 15_000 }).toBe(2);

  await goTo(page, "Logs");
  await expect(itemOf(page, "Request logs", "Chat completions")).toHaveCount(1);
  await expect(itemOf(page, "Request logs", "Playground")).toHaveCount(1);
  await choose(page, "Endpoint", "Chat completions");
  await expect(itemOf(page, "Request logs", "Playground")).toHaveCount(0);
  await expect(itemOf(page, "Request logs", "Chat completions")).toHaveCount(1);
  await choose(page, "Endpoint", "Images");
  await expect(page.getByRole("heading", { name: "No calls" })).toBeVisible();
  await choose(page, "Endpoint", "All endpoints");
  await expect(itemOf(page, "Request logs", "Playground")).toHaveCount(1);
});
