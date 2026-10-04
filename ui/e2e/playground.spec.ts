import type { Page } from "@playwright/test";
import {
  expect,
  goTo,
  signInFromStart,
  test,
  type Account,
  type GatewayApi,
} from "./fixtures";
import { startMockProvider, type MockProvider } from "./mock-provider";

let mock: MockProvider;

test.beforeEach(async () => {
  mock = await startMockProvider(["e2e-model", "e2e-other"]);
  mock.usage = { prompt: 3, completion: 7 };
});

test.afterEach(async () => {
  await mock.close();
});

/** A provider with two models, e2e-model open to everyone and priced. */
async function setup(api: GatewayApi) {
  const provider = await api.addProvider("alpha", mock.baseUrl, mock.apiKey);
  await api.sync(provider);
  const model = await api.open("alpha", "e2e-model");
  // 3 tokens in at $10 and 7 out at $20 per million: 170 millionths of a dollar.
  await api.setPrice(model, 10_000_000, 20_000_000);
  return model;
}

function box(page: Page) {
  return page.getByRole("textbox", { name: "Message" });
}

function picker(page: Page) {
  return page.getByRole("combobox", { name: "Model or route" });
}

test("an admin chats: the answer streams in, tokens and cost are shown, and the call is logged as a playground call of the user", async ({
  page,
  admin,
  apiAs,
}) => {
  const api = await apiAs(admin);
  await setup(api);
  await signInFromStart(page, admin);
  await goTo(page, "Playground");

  await picker(page).click();
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();
  await box(page).fill("Say hello");
  await page.getByRole("button", { name: "Send" }).click();

  const thread = page.getByRole("list", { name: "Conversation" });
  await expect(thread).toContainText("Say hello");
  await expect(thread).toContainText(mock.answer);
  await expect(page.getByText("Tokens: 3 in, 7 out. Cost: $0.00017.")).toBeVisible();
  // Send is back, and the box is empty for the next message.
  await expect(page.getByRole("button", { name: "Send" })).toBeDisabled();
  await expect(box(page)).toHaveValue("");

  // The provider was called once with its own key, and the gateway logged
  // the call: a stream, to the user, with no key, as a playground call.
  expect(mock.calls).toEqual([{ authorized: true, model: "e2e-model" }]);
  await expect.poll(async () => (await api.logs()).length, { timeout: 15_000 }).toBe(1);
  const [row] = await api.logs();
  expect(row).toMatchObject({
    endpoint: "playground",
    key_name: null,
    status: 200,
    stream: true,
    cost_micros: 170,
    user_email: admin.email,
    requested: "alpha/e2e-model",
  });

  // Nothing of the conversation is kept: opened again, the page is empty.
  await goTo(page, "Logs");
  await goTo(page, "Playground");
  await expect(page.getByText("Nothing has been said yet")).toBeVisible();
  await expect(page.getByText(mock.answer)).toHaveCount(0);
});

test("Copy as curl gives the call for /v1 with a place for the key, and Stop ends a call", async ({
  page,
  admin,
  apiAs,
}) => {
  await setup(await apiAs(admin));
  await signInFromStart(page, admin);
  await goTo(page, "Playground");
  await picker(page).click();
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();
  await box(page).fill("Hello there");
  await page.getByRole("button", { name: "Copy as curl" }).click();
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(copied).toContain(`curl ${new URL(page.url()).origin}/v1/chat/completions`);
  expect(copied).toContain("-H 'Authorization: Bearer <your key>'");
  expect(copied).toContain('"model":"alpha/e2e-model"');
  expect(copied).toContain('"content":"Hello there"');
  expect(copied).not.toMatch(/uf_session|csrf/i);

  // A slow provider: Stop ends the wait and gives the message back.
  mock.mode = { delayMs: 5_000 };
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByText("Waiting for the answer")).toBeVisible();
  await page.getByRole("button", { name: "Stop" }).click();
  await expect(page.getByRole("button", { name: "Send" })).toBeVisible();
  await expect(box(page)).toHaveValue("Hello there");
  await expect(page.getByRole("alert")).toHaveCount(0);
});

async function member(api: GatewayApi): Promise<Account & { id: number }> {
  return api.activeUser("Mia");
}

test("a member is offered only what they may call, and a model they may no longer call is refused as their key would be", async ({
  page,
  admin,
  apiAs,
  rules,
}) => {
  const api = await apiAs(admin);
  const open = await setup(api);
  const mia = await member(api);
  const other = (await api.modelNamed("alpha", "e2e-other")).id;
  await api.enable(other);
  await api.grant(other, { team_ids: [] });

  await signInFromStart(page, mia);
  await goTo(page, "Playground");
  await picker(page).click();
  // Only the model that is open to everyone: the other one is granted to nobody.
  await expect(page.getByRole("option")).toHaveText(["alpha/e2e-model"]);
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();

  // The grant goes while the page is open: the call is refused as /v1 refuses it.
  await api.grant(open, { team_ids: [] });
  rules.expectRefusal(403, /^\/api\/playground\/chat$/);
  await box(page).fill("Am I allowed?");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByRole("alert")).toContainText(
    "You do not have access to model 'alpha/e2e-model'.",
  );
  await expect(box(page)).toHaveValue("Am I allowed?");
  expect(mock.calls).toEqual([]);
  // The refusal was logged too, to the member.
  await expect.poll(async () => (await api.logs()).length, { timeout: 15_000 }).toBe(1);
  expect((await api.logs())[0]).toMatchObject({
    endpoint: "playground",
    status: 403,
    key_name: null,
    user_email: mia.email,
  });
});

test("the limits of the user apply to the playground: the second call of a user limited to one a minute is refused with the wait", async ({
  page,
  admin,
  apiAs,
  rules,
}) => {
  const api = await apiAs(admin);
  await setup(api);
  const mia = await member(api);
  await api.setLimit({ scope: "user", scope_id: mia.id, requests_per_minute: 1 });

  await signInFromStart(page, mia);
  await goTo(page, "Playground");
  await picker(page).click();
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();
  await box(page).fill("First");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByRole("list", { name: "Conversation" })).toContainText(mock.answer);

  rules.expectRefusal(429, /^\/api\/playground\/chat$/);
  await box(page).fill("Second");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByRole("alert")).toContainText(
    `rate limit 'requests per minute' of user '${mia.email}' reached`,
  );
  await expect(page.getByRole("alert")).toContainText(/Try again in \d+ (?:seconds?|minutes?)\./);
  expect(mock.calls).toHaveLength(1);
});
