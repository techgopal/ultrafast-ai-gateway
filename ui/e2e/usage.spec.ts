import {
  chat,
  expect,
  goTo,
  heading,
  itemOf,
  signInFromStart,
  test,
} from "./fixtures";
import { startMockProvider, type MockProvider } from "./mock-provider";

let mock: MockProvider;

test.beforeEach(async () => {
  mock = await startMockProvider(["e2e-model"]);
  // 100 000 tokens in at $10 per million and 200 000 out at $20: $5.00 a call.
  mock.usage = { prompt: 100_000, completion: 200_000 };
});

test.afterEach(async () => {
  await mock.close();
});

test("priced calls show in the logs with their cost and attempts, in the Overview tiles, and a member sees only their own", async ({
  page,
  newContext,
  admin,
  apiAs,
  request,
}) => {
  const api = await apiAs(admin);
  const provider = await api.addProvider("alpha", mock.baseUrl, mock.apiKey);
  await api.sync(provider);
  const model = await api.open("alpha", "e2e-model");
  await api.setPrice(model, 10_000_000, 20_000_000);
  await api.createRoute({
    name: "main",
    primaries: [{ model_id: model, weight: 1 }],
    everyone: true,
  });
  const mia = await api.activeUser("Mia");
  const adminKey = await api.createKey("admin-key");
  const miaKey = await (await apiAs(mia)).createKey("mia-key");

  for (const key of [adminKey, adminKey, miaKey]) {
    expect((await chat(request, key, "main")).status()).toBe(200);
  }
  // The log writer batches for up to a second: wait for the records.
  await expect.poll(async () => (await api.logs()).length, { timeout: 15_000 }).toBe(3);
  await expect
    .poll(
      async () =>
        (
          (await api.get("/api/usage?group=day")) as {
            total: { requests: number; cost_micros: number };
          }
        ).total.cost_micros,
      { timeout: 15_000 },
    )
    .toBe(15_000_000);

  await signInFromStart(page, admin);

  // Overview: the tiles of the last 30 days.
  const tile = (title: string) => page.getByRole("group", { name: title, exact: true });
  await expect(tile("Requests")).toContainText("3");
  await expect(tile("Spend")).toContainText("$15.00");
  await expect(tile("Tokens")).toContainText("300,000 in");
  await expect(tile("Tokens")).toContainText("600,000 out");

  // Logs: each call with its cost.
  await goTo(page, "Logs");
  await expect(itemOf(page, "Request logs", "mia-key")).toContainText("$5.00");
  await expect(itemOf(page, "Request logs", "alpha/e2e-model")).toHaveCount(3);
  await expect(itemOf(page, "Request logs", "admin-key")).toHaveCount(2);

  // The detail of a call shows its routing attempt.
  await itemOf(page, "Request logs", "mia-key").getByRole("link").first().click();
  await expect(heading(page, "Call")).toBeVisible();
  await expect(page.getByText("Asked for", { exact: true })).toBeVisible();
  await expect(itemOf(page, "Routing attempts", "alpha")).toContainText("e2e-model");
  await expect(itemOf(page, "Routing attempts", "alpha")).toContainText("Answered");

  // A member sees only their own calls.
  const context = await newContext();
  const member = await context.newPage();
  await signInFromStart(member, mia);
  await goTo(member, "Logs");
  await expect(itemOf(member, "Request logs", "mia-key")).toHaveCount(1);
  await expect(itemOf(member, "Request logs", "admin-key")).toHaveCount(0);
});
