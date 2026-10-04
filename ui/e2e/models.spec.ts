import {
  expect,
  goTo,
  signInFromStart,
  test,
  type GatewayApi,
} from "./fixtures";
import { startMockProvider, type MockProvider } from "./mock-provider";

let mock: MockProvider;

test.beforeEach(async () => {
  mock = await startMockProvider([
    "e2e-open",
    "e2e-team",
    "e2e-off",
    "e2e-bare",
  ]);
});

test.afterEach(async () => {
  await mock.close();
});

/**
 * Four models of one provider: `e2e-open` for everyone, `e2e-team` enabled
 * and granted to nobody yet, `e2e-off` granted to everyone but disabled, and
 * `e2e-bare` neither. Returns the id of `e2e-team`.
 */
async function catalog(api: GatewayApi): Promise<number> {
  const provider = await api.addProvider("mock", mock.baseUrl, mock.apiKey);
  expect(await api.sync(provider)).toHaveLength(4);
  await api.open("mock", "e2e-open");
  const off = await api.modelNamed("mock", "e2e-off");
  await api.grant(off.id, { everyone: true });
  const team = await api.modelNamed("mock", "e2e-team");
  await api.enable(team.id);
  return team.id;
}

test("a member's key calls only what is enabled and granted to them", async ({
  page,
  admin,
  apiAs,
  request,
}) => {
  const api = await apiAs(admin);
  const teamModel = await catalog(api);
  const member = await api.activeUser("Mia");
  const team = await api.createTeam("Alpha");
  await api.putMember(team, member.id, "member");

  const key = await (await apiAs(member)).createKey("member key");
  const chat = (model: string) =>
    request.post("/v1/chat/completions", {
      headers: { authorization: `Bearer ${key}` },
      data: { model, messages: [{ role: "user", content: "Hello" }] },
    });
  const listed = async () => {
    const answer = await request.get("/v1/models", {
      headers: { authorization: `Bearer ${key}` },
    });
    expect(answer.status()).toBe(200);
    const body = (await answer.json()) as { data: { id: string }[] };
    return body.data.map((model) => model.id).sort();
  };

  // Granted to everyone: it works. Disabled, or enabled and given to nobody: refused.
  expect((await chat("mock/e2e-open")).status()).toBe(200);
  expect((await chat("mock/e2e-off")).status()).toBe(403);
  expect((await chat("mock/e2e-team")).status()).toBe(403);
  expect((await chat("mock/e2e-bare")).status()).toBe(403);
  expect(mock.calls.map((call) => call.model)).toEqual(["e2e-open"]);
  expect(await listed()).toEqual(["mock/e2e-open"]);

  await signInFromStart(page, member);
  await goTo(page, "Models");
  const usable = page.getByRole("list", { name: "Models you can use" });
  await expect(usable.getByRole("listitem")).toHaveText([/^mock\/e2e-openInput Not set · Output Not set per 1M tokensCopy$/]);

  // Granted to the member's team, the model can be called, and is listed everywhere.
  await api.grant(teamModel, { team_ids: [team] });
  expect((await chat("mock/e2e-team")).status()).toBe(200);
  expect(mock.calls.map((call) => call.model)).toEqual([
    "e2e-open",
    "e2e-team",
  ]);
  expect(await listed()).toEqual(["mock/e2e-open", "mock/e2e-team"]);
  await page.reload();
  await expect(usable.getByRole("listitem")).toHaveText([
    /^mock\/e2e-openInput Not set · Output Not set per 1M tokensCopy$/,
    /^mock\/e2e-teamInput Not set · Output Not set per 1M tokensCopy$/,
  ]);
  await expect(page.getByRole("switch")).toHaveCount(0);

  // Disabled again, it is refused and gone from both lists.
  await api.enable(teamModel, false);
  expect((await chat("mock/e2e-team")).status()).toBe(403);
  expect(await listed()).toEqual(["mock/e2e-open"]);
});
