import {
  chat,
  expect,
  goTo,
  itemOf,
  signInFromStart,
  test,
} from "./fixtures";
import { startMockProvider, type MockProvider } from "./mock-provider";

let mock: MockProvider;

test.beforeEach(async () => {
  mock = await startMockProvider(["e2e-model"]);
});

test.afterEach(async () => {
  await mock.close();
});

test("a cached route answers the same call of a team from memory, never another team's", async ({
  page,
  admin,
  apiAs,
  request,
}) => {
  const api = await apiAs(admin);
  const provider = await api.addProvider("alpha", mock.baseUrl, mock.apiKey);
  await api.sync(provider);
  const model = await api.open("alpha", "e2e-model");
  const routeId = await api.createRoute({
    name: "main",
    primaries: [{ model_id: model, weight: 1 }],
    everyone: true,
  });
  await api.updateRoute(routeId, {
    name: "main",
    primaries: [{ model_id: model, weight: 1 }],
    everyone: true,
    cache_enabled: true,
  });
  const teamA = await api.createTeam("Team A");
  const teamB = await api.createTeam("Team B");
  const ann = await api.activeUser("Ann");
  const bob = await api.activeUser("Bob");
  await api.putMember(teamA, ann.id, "member");
  await api.putMember(teamB, bob.id, "member");
  const annKey = await (await apiAs(ann)).createKey("ann-key");
  const bobKey = await (await apiAs(bob)).createKey("bob-key");

  expect((await chat(request, annKey, "main", "Same question")).status()).toBe(200);
  expect(mock.calls).toHaveLength(1);
  // Answered from memory: no second provider call.
  const second = await chat(request, annKey, "main", "Same question");
  expect(second.status()).toBe(200);
  expect(mock.calls).toHaveLength(1);
  // Another team asks the same: the provider is called.
  expect((await chat(request, bobKey, "main", "Same question")).status()).toBe(200);
  expect(mock.calls).toHaveLength(2);

  await expect
    .poll(async () => (await api.logs()).length, { timeout: 15_000 })
    .toBe(3);
  await signInFromStart(page, admin);
  await goTo(page, "Logs");
  const annRows = itemOf(page, "Request logs", "ann-key");
  await expect(annRows).toHaveCount(2);
  await expect(annRows.filter({ hasText: "Cached" })).toHaveCount(1);
  await expect(itemOf(page, "Request logs", "bob-key")).not.toContainText("Cached");
});
