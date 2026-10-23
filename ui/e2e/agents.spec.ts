import {
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
  mock.usage = { prompt: 1_000, completion: 500 };
});

test.afterEach(async () => {
  await mock.close();
});

test("the calls of one coding-agent session are found by its id and summed in the session view", async ({
  page,
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
  const key = await api.createKey("agent-key");

  // Codex names its session in `session-id`: two calls of one, one of another.
  for (const session of ["sess-a", "sess-a", "sess-b"]) {
    const answer = await request.post("/v1/chat/completions", {
      headers: { authorization: `Bearer ${key}`, "session-id": session, originator: "codex_exec" },
      data: { model: "main", messages: [{ role: "user", content: "Hello" }] },
    });
    expect(answer.status()).toBe(200);
  }
  await expect.poll(async () => (await api.logs()).length, { timeout: 15_000 }).toBe(3);

  await signInFromStart(page, admin);
  await goTo(page, "Logs");
  await expect(itemOf(page, "Request logs", "agent-key")).toHaveCount(3);

  // Filter by session.
  const filter = page.getByRole("textbox", { name: "Agent session" });
  await filter.fill("sess-a");
  await filter.press("Enter");
  await expect(itemOf(page, "Request logs", "agent-key")).toHaveCount(2);
  await expect(itemOf(page, "Request logs", "sess-b")).toHaveCount(0);

  // The session view: its calls and what they came to together.
  await itemOf(page, "Request logs", "sess-a").first().getByRole("link", { name: "sess-a" }).click();
  await expect(heading(page, "Session sess-a")).toBeVisible();
  await expect(itemOf(page, "Session calls", "alpha/e2e-model")).toHaveCount(2);
  const totals = page.getByLabel("Totals");
  await expect(totals).toContainText("2,000 / 1,000");
  await expect(totals).toContainText("$0.04");
});
