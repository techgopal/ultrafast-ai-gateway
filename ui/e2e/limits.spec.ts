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
  // 100 000 tokens in at $10 per million and 200 000 out at $20: $5.00 a call.
  mock.usage = { prompt: 100_000, completion: 200_000 };
});

test.afterEach(async () => {
  await mock.close();
});

async function setup(api: GatewayApi) {
  const provider = await api.addProvider("alpha", mock.baseUrl, mock.apiKey);
  await api.sync(provider);
  const model = await api.open("alpha", "e2e-model");
  await api.setPrice(model, 10_000_000, 20_000_000);
  await api.createRoute({
    name: "main",
    primaries: [{ model_id: model, weight: 1 }],
    everyone: true,
  });
}

test("a key limit of two requests a minute refuses the third call, naming the limit", async ({
  admin,
  apiAs,
  request,
}) => {
  const api = await apiAs(admin);
  await setup(api);
  const key = await api.createKey("limited");
  await api.setLimit({
    scope: "key",
    scope_id: await api.keyId("limited"),
    requests_per_minute: 2,
  });

  expect((await chat(request, key, "main")).status()).toBe(200);
  expect((await chat(request, key, "main")).status()).toBe(200);
  const third = await chat(request, key, "main");
  expect(third.status()).toBe(429);
  expect(third.headers()["retry-after"]).toMatch(/^[1-9]\d*$/);
  expect(await third.json()).toMatchObject({
    error: {
      type: "rate_limit_error",
      message: "rate limit 'requests per minute' of key 'limited' reached",
    },
  });
  // The refused call never reached the provider.
  expect(mock.calls).toHaveLength(2);
});

test("a spent block budget refuses calls with budget_exceeded, and the Budgets page shows what was spent", async ({
  page,
  admin,
  apiAs,
  request,
}) => {
  const api = await apiAs(admin);
  await setup(api);
  const key = await api.createKey("spender");
  const keyId = await api.keyId("spender");

  expect((await chat(request, key, "main")).status()).toBe(200);
  await expect.poll(async () => (await api.logs())[0]?.cost_micros, { timeout: 15_000 }).toBe(5_000_000);

  // The budget is set after the spend; it is counted when the writer prices a record.
  await api.setBudget({
    scope: "key",
    scope_id: keyId,
    amount_micros: 1_000_000,
    period: "daily",
    action: "block",
  });
  // Spend that came after the budget counts: call until the gateway refuses.
  let refused: Awaited<ReturnType<typeof chat>> | undefined;
  await expect
    .poll(
      async () => {
        const answer = await chat(request, key, "main");
        if (answer.status() === 429) refused = answer;
        return answer.status();
      },
      { timeout: 20_000, intervals: [500] },
    )
    .toBe(429);
  expect(refused?.headers()["retry-after"]).toMatch(/^[1-9]\d*$/);
  expect(await refused?.json()).toMatchObject({
    error: {
      type: "rate_limit_error",
      code: "budget_exceeded",
      message: "budget 'daily $1.00' of key 'spender' reached",
    },
  });

  await signInFromStart(page, admin);
  await goTo(page, "Budgets and limits");
  const row = itemOf(page, "Budgets", "spender");
  await expect(row).toContainText("$1.00");
  await expect(row).toContainText("Block");
  await expect(row.getByRole("progressbar", { name: "Spent this period" })).toBeVisible();
  const budget = (await api.budgets()).find((one) => one.scope === "key" && one.scope_id === keyId);
  const spent = budget?.spent_micros ?? 0;
  expect(spent).toBeGreaterThanOrEqual(1_000_000);
  await expect(row).toContainText(/\$\d+\.\d\d \((?:[1-9]\d{2,})%\)/);
});
