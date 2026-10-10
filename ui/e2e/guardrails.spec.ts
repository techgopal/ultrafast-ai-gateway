import type { Page } from "@playwright/test";
import {
  expect,
  goTo,
  heading,
  itemOf,
  listOf,
  openNavigation,
  signInFromStart,
  test,
  type GatewayApi,
} from "./fixtures";
import { startMockProvider, type MockProvider } from "./mock-provider";

let mock: MockProvider;
const MAIL = "ada@example.com";

test.beforeEach(async () => {
  mock = await startMockProvider(["e2e-model"]);
  mock.answer = `Please write to ${MAIL} about it.`;
});

test.afterEach(async () => {
  await mock.close();
});

async function setup(api: GatewayApi) {
  const provider = await api.addProvider("alpha", mock.baseUrl, mock.apiKey);
  await api.sync(provider);
  await api.open("alpha", "e2e-model");
}

async function chat(page: Page, text: string) {
  await page.getByRole("textbox", { name: "Message" }).fill(text);
  await page.getByRole("button", { name: "Send" }).click();
}

async function choosePlaygroundModel(page: Page) {
  await page.getByRole("combobox", { name: "Model or route" }).click();
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();
}

async function openNewGuardrail(page: Page) {
  await goTo(page, "Guardrails");
  await page.getByRole("link", { name: "Add guardrail" }).click();
  await expect(heading(page, "New guardrail")).toBeVisible();
}

async function choose(page: Page, select: ReturnType<Page["getByRole"]>, option: string) {
  await select.click();
  await page.getByRole("option", { name: option, exact: true }).click();
}

test("a guardrail that redacts emails in answers, for every call: it is tried, used by the playground, and the log says Redacted", async ({
  page,
  admin,
  apiAs,
}) => {
  const api = await apiAs(admin);
  await setup(api);
  await signInFromStart(page, admin);

  // Make it: a PII rule on the output only, for every call.
  await openNewGuardrail(page);
  await page.getByLabel("Name", { exact: true }).fill("mask-emails");
  const rule = page.getByRole("group", { name: "Rule 1" });
  await choose(page, rule.getByRole("combobox", { name: "Applies to" }), "Output only");
  await page.getByRole("switch", { name: "Applies to every call" }).click();

  // Try it before it is saved: nothing is stored or logged.
  await page.getByLabel("Text to check").fill(`Please write to ${MAIL} about it.`);
  await choose(page, page.getByRole("combobox", { name: "Check as" }), "Output of a call");
  await page.getByRole("button", { name: "Check the text" }).click();
  const after = page.getByRole("group", { name: "Text after the guardrail" });
  await expect(after).toContainText("Please write to [REDACTED:EMAIL] about it.");
  await expect(after.locator("mark")).toHaveText("[REDACTED:EMAIL]");
  await expect(page.getByRole("list", { name: "What was found" })).toContainText("Redacted: EMAIL 1.");

  await page.getByRole("button", { name: "Create guardrail" }).click();
  await expect(heading(page, "Guardrails")).toBeVisible();
  const row = itemOf(page, "Guardrails", "mask-emails");
  await expect(row).toContainText("Every call");
  await expect(row).toContainText("Enabled");

  // The playground is a call like any other: the address never reaches the screen.
  await goTo(page, "Playground");
  await choosePlaygroundModel(page);
  await chat(page, "Who should I write to?");
  const thread = page.getByRole("list", { name: "Conversation" });
  await expect(thread).toContainText("Please write to [REDACTED:EMAIL] about it.");
  await expect(thread).not.toContainText(MAIL);
  expect(mock.calls).toHaveLength(1);

  // The log: a badge, a filter, and the call's page with names and counts.
  await expect.poll(async () => (await api.logs()).length, { timeout: 15_000 }).toBe(1);
  await goTo(page, "Logs");
  const calls = listOf(page, "Request logs");
  await expect(calls).toContainText("Redacted");
  await page.getByRole("combobox", { name: "Guardrails" }).click();
  await page.getByRole("option", { name: "Redacted", exact: true }).click();
  await expect(calls).toContainText("Redacted");
  await page.getByRole("combobox", { name: "Guardrails" }).click();
  await page.getByRole("option", { name: "Blocked", exact: true }).click();
  await expect(page.getByRole("heading", { name: "No calls" })).toBeVisible();
  await page.getByRole("combobox", { name: "Guardrails" }).click();
  await page.getByRole("option", { name: "Redacted", exact: true }).click();
  await page.getByRole("link").filter({ has: page.locator("time") }).first().click();
  await expect(heading(page, "Call")).toBeVisible();
  await expect(page.getByRole("heading", { level: 2, name: "Guardrails" })).toBeVisible();
  await expect(page.getByText("Checked with mask-emails.")).toBeVisible();
  await expect(page.getByText("Redacted: EMAIL 1.")).toBeVisible();
  await expect(page.locator("body")).not.toContainText(MAIL);

  const [logged] = await api.logs();
  expect(logged?.guardrails).toMatchObject({ action: "redacted" });
});

test("a guardrail that blocks a word in messages: the playground says which one, the provider is not called, and the log says Blocked", async ({
  page,
  admin,
  apiAs,
  rules,
}) => {
  const api = await apiAs(admin);
  await setup(api);
  await signInFromStart(page, admin);

  await openNewGuardrail(page);
  await page.getByLabel("Name", { exact: true }).fill("no-swordfish");
  const rule = page.getByRole("group", { name: "Rule 1" });
  await rule.getByLabel("Rule id").fill("word");
  await choose(page, rule.getByRole("combobox", { name: "Matches" }), "Keywords");
  await rule.getByLabel("Words", { exact: true }).fill("swordfish");
  await choose(page, rule.getByRole("combobox", { name: "Action" }), "Block");
  await choose(page, rule.getByRole("combobox", { name: "Applies to" }), "Input only");
  await page.getByRole("switch", { name: "Applies to every call" }).click();
  await page.getByRole("button", { name: "Create guardrail" }).click();
  await expect(heading(page, "Guardrails")).toBeVisible();
  await expect(itemOf(page, "Guardrails", "no-swordfish")).toContainText("1 rule");

  rules.expectRefusal(400, /\/api\/playground\/chat$/);
  await goTo(page, "Playground");
  await choosePlaygroundModel(page);
  await chat(page, "Tell me about the Swordfish.");
  await expect(page.getByRole("alert")).toContainText("Blocked by guardrail 'no-swordfish'.");
  // The message is back in the box, and the provider heard nothing.
  await expect(page.getByRole("textbox", { name: "Message" })).toHaveValue("Tell me about the Swordfish.");
  expect(mock.calls).toHaveLength(0);

  await expect.poll(async () => (await api.logs()).length, { timeout: 15_000 }).toBe(1);
  await goTo(page, "Logs");
  await page.getByRole("combobox", { name: "Guardrails" }).click();
  await page.getByRole("option", { name: "Blocked", exact: true }).click();
  await expect(listOf(page, "Request logs")).toContainText("Blocked");
  await expect(listOf(page, "Request logs")).toContainText("400");
  expect((await api.logs())[0]?.guardrails).toMatchObject({ action: "blocked" });
});

test("a guardrail is attached to a key from the keys page, in the order chosen, and a member sees nothing of the page", async ({
  page,
  admin,
  apiAs,
  newContext,
}) => {
  const api = await apiAs(admin);
  await setup(api);
  await signInFromStart(page, admin);
  for (const name of ["first-rules", "second-rules"]) {
    await openNewGuardrail(page);
    await page.getByLabel("Name", { exact: true }).fill(name);
    await page.getByRole("button", { name: "Create guardrail" }).click();
    await expect(itemOf(page, "Guardrails", name)).toBeVisible();
  }

  await goTo(page, "Virtual keys");
  await page.getByRole("button", { name: "Create key" }).click();
  const dialog = page.getByRole("dialog", { name: "Create key" });
  await dialog.getByLabel("Name", { exact: true }).fill("ci");
  for (const name of ["second-rules", "first-rules"]) {
    await choose(page, dialog.getByRole("combobox", { name: "Add a guardrail" }), name);
  }
  await expect(dialog.getByRole("list", { name: "Chosen guardrails" }).getByRole("listitem")).toHaveText([
    "1.second-rules",
    "2.first-rules",
  ]);
  await dialog.getByRole("button", { name: "Create key" }).click();
  await page.getByRole("dialog", { name: "Your new key" }).getByRole("button", { name: "Done" }).click();
  await page.getByRole("alertdialog").getByRole("button", { name: "Close" }).click();
  await expect(itemOf(page, "Virtual keys", "ci").getByRole("group", { name: "Guardrails" })).toHaveText(
    "second-rulesfirst-rules",
  );

  // A member does not see the page, and its link is not offered.
  const mia = await api.activeUser("Mia");
  const other = await (await newContext()).newPage();
  await signInFromStart(other, mia);
  const nav = await openNavigation(other);
  await expect(nav.getByRole("link", { name: "Virtual keys" })).toBeVisible();
  await expect(nav.getByRole("link", { name: "Guardrails" })).toHaveCount(0);
  await other.goto("/guardrails");
  await expect(other.getByText("This page is not available to your account.")).toBeVisible();
});
