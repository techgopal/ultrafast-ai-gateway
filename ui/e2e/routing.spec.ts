import {
  expect,
  goTo,
  heading,
  itemOf,
  signInFromStart,
  test,
  type GatewayApi,
} from "./fixtures";
import { startMockProvider, type MockProvider } from "./mock-provider";

let alpha: MockProvider;
let beta: MockProvider;

test.beforeEach(async () => {
  alpha = await startMockProvider(["e2e-model"]);
  beta = await startMockProvider(["e2e-model"]);
});

test.afterEach(async () => {
  await alpha.close();
  await beta.close();
});

/** Two providers, one open model each. Returns the ids of the models. */
async function twoProviders(api: GatewayApi) {
  for (const [name, mock] of [
    ["alpha", alpha],
    ["beta", beta],
  ] as const) {
    const id = await api.addProvider(name, mock.baseUrl, mock.apiKey);
    await api.sync(id);
  }
  return {
    a: await api.open("alpha", "e2e-model"),
    b: await api.open("beta", "e2e-model"),
  };
}

test("a failing primary is covered by the fallback, shows as Failing, and the route is for its teams", async ({
  page,
  admin,
  apiAs,
  request,
}) => {
  const api = await apiAs(admin);
  const { a, b } = await twoProviders(api);
  const team = await api.createTeam("Alpha");
  const inside = await api.activeUser("Ida");
  const outside = await api.activeUser("Otto");
  await api.putMember(team, inside.id, "member");
  await api.createRoute({
    name: "main",
    primaries: [{ model_id: a, weight: 1 }],
    fallbacks: [b],
    breaker_failures: 2,
    team_ids: [team],
  });

  const call = async (key: string) =>
    request.post("/v1/chat/completions", {
      headers: { authorization: `Bearer ${key}` },
      data: { model: "main", messages: [{ role: "user", content: "Hello" }] },
    });
  const insideKey = await (await apiAs(inside)).createKey("inside");
  const outsideKey = await (await apiAs(outside)).createKey("outside");

  // Healthy: the primary answers.
  const first = await call(insideKey);
  expect(first.status()).toBe(200);
  expect(
    ((await first.json()) as { choices: { message: { content: string } }[] })
      .choices[0]?.message.content,
  ).toBe(alpha.answer);
  expect(alpha.calls).toHaveLength(1);
  expect(beta.calls).toHaveLength(0);

  // A member of no team of the route is refused, and nothing is called.
  const refused = await call(outsideKey);
  expect(refused.status()).toBe(403);
  expect(await refused.json()).toMatchObject({
    error: {
      type: "permission_error",
      message: "You do not have access to model 'main'.",
    },
  });
  expect(alpha.calls).toHaveLength(1);
  expect(beta.calls).toHaveLength(0);

  // The primary answers 500: the call succeeds from the fallback.
  alpha.mode = { status: 500 };
  for (const round of [1, 2]) {
    const answered = await call(insideKey);
    expect(answered.status(), `call ${String(round)}`).toBe(200);
    const body = (await answered.json()) as {
      choices: { message: { content: string } }[];
    };
    expect(body.choices[0]?.message.content).toBe(beta.answer);
  }
  expect(alpha.calls).toHaveLength(3);
  expect(beta.calls).toHaveLength(2);

  // Two failures opened the breaker: the primary is skipped.
  const skipped = await call(insideKey);
  expect(skipped.status()).toBe(200);
  expect(alpha.calls).toHaveLength(3);
  expect(beta.calls).toHaveLength(3);

  // The console shows the primary as Failing and the fallback as Healthy.
  await signInFromStart(page, admin);
  await goTo(page, "Routing");
  await expect(itemOf(page, "Target health", "alpha/e2e-model")).toContainText(
    "Failing",
  );
  await expect(itemOf(page, "Target health", "alpha/e2e-model")).toContainText(
    "Status 500",
  );
  await expect(itemOf(page, "Target health", "beta/e2e-model")).toContainText(
    "Healthy",
  );
});

test("a primary that is too slow to answer is covered by the fallback", async ({
  admin,
  apiAs,
  request,
}) => {
  const api = await apiAs(admin);
  const { a, b } = await twoProviders(api);
  await api.createRoute({
    name: "slow",
    primaries: [{ model_id: a, weight: 1 }],
    fallbacks: [b],
    first_token_timeout_ms: 1000,
    everyone: true,
  });
  const key = await api.createKey("slow");
  alpha.mode = { delayMs: 2500 };
  const answered = await request.post("/v1/chat/completions", {
    headers: { authorization: `Bearer ${key}` },
    data: { model: "slow", messages: [{ role: "user", content: "Hello" }] },
  });
  expect(answered.status()).toBe(200);
  expect(
    ((await answered.json()) as { choices: { message: { content: string } }[] })
      .choices[0]?.message.content,
  ).toBe(beta.answer);
  expect(beta.calls).toHaveLength(1);
});

test("an admin makes a route in the console, changes it, and the gateway serves what was saved", async ({
  page,
  admin,
  apiAs,
  request,
}) => {
  const api = await apiAs(admin);
  await twoProviders(api);
  await signInFromStart(page, admin);
  await goTo(page, "Routing");

  await page.getByRole("link", { name: "Add route" }).click();
  await expect(heading(page, "New route")).toBeVisible();
  const form = page.getByRole("form", { name: "Route" });
  await form.getByLabel("Name", { exact: true }).fill("console-route");
  await form.getByRole("button", { name: "Add primary target" }).click();
  await form
    .getByRole("combobox", { name: "Model of primary target 1" })
    .click();
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();
  await form.getByRole("button", { name: "Add fallback" }).click();
  await form.getByRole("combobox", { name: "Model of fallback 1" }).click();
  await page.getByRole("option", { name: "beta/e2e-model" }).click();
  await form.getByRole("radio", { name: "All teams" }).check();
  await form.getByRole("button", { name: "Create route" }).click();
  await expect(page.getByText("Route created.")).toBeVisible();
  await expect(heading(page, "Routing")).toBeVisible();
  const row = itemOf(page, "Routes", "console-route");
  await expect(row).toContainText("Ready");

  // What the console saved is what the API holds.
  const saved = async () =>
    (
      (await api.get("/api/routes")) as {
        routes: {
          name: string;
          retries: number;
          everyone: boolean;
          fallbacks: unknown[];
        }[];
      }
    ).routes.find((route) => route.name === "console-route");
  expect(await saved()).toMatchObject({ retries: 2, everyone: true });
  expect((await saved())?.fallbacks).toHaveLength(1);

  // Edited in the console, the change is saved and the route answers by it.
  await row.getByRole("link", { name: "Edit" }).click();
  await expect(heading(page, "Edit route")).toBeVisible();
  const edit = page.getByRole("form", { name: "Route" });
  await expect(
    edit.getByRole("combobox", { name: "Model of primary target 1" }),
  ).toContainText("alpha/e2e-model");
  await edit.getByRole("button", { name: "Advanced" }).click();
  await edit.getByLabel("Retries", { exact: true }).fill("0");
  await edit.getByRole("button", { name: "Save route" }).click();
  await expect(page.getByText("Route saved.")).toBeVisible();
  await expect(heading(page, "Routing")).toBeVisible();
  expect(await saved()).toMatchObject({ retries: 0 });

  const key = await api.createKey("console route");
  alpha.mode = { status: 500 };
  const answered = await request.post("/v1/chat/completions", {
    headers: { authorization: `Bearer ${key}` },
    data: {
      model: "console-route",
      messages: [{ role: "user", content: "Hello" }],
    },
  });
  expect(answered.status()).toBe(200);
  expect(alpha.calls).toHaveLength(1);
  expect(beta.calls).toHaveLength(1);

  // Deleted in the console, the route is gone and its name unknown.
  await expect(heading(page, "Routing")).toBeVisible();
  await itemOf(page, "Routes", "console-route")
    .getByRole("button", { name: "Delete" })
    .click();
  await page
    .getByRole("alertdialog", { name: "Delete console-route?" })
    .getByRole("button", { name: "Delete" })
    .click();
  await expect(itemOf(page, "Routes", "console-route")).toBeHidden();
  const gone = await request.post("/v1/chat/completions", {
    headers: { authorization: `Bearer ${key}` },
    data: {
      model: "console-route",
      messages: [{ role: "user", content: "Hello" }],
    },
  });
  expect(gone.status()).toBe(404);
});

test("a stream that breaks after its first chunk is not moved to the fallback", async ({
  admin,
  apiAs,
  request,
}) => {
  const api = await apiAs(admin);
  const { a, b } = await twoProviders(api);
  await api.createRoute({
    name: "stream",
    primaries: [{ model_id: a, weight: 1 }],
    fallbacks: [b],
    everyone: true,
  });
  const key = await api.createKey("stream");
  alpha.mode = { breakAfterFirstChunk: true };
  const answered = await request.post("/v1/chat/completions", {
    headers: { authorization: `Bearer ${key}` },
    data: {
      model: "stream",
      stream: true,
      messages: [{ role: "user", content: "Hello" }],
    },
  });
  // The answer began, so it is the primary's: the stream ends in an error event, and nothing is retried.
  expect(answered.status()).toBe(200);
  const text = await answered.text();
  expect(text).toContain("Hel");
  expect(text).toContain("The connection to the provider was lost.");
  expect(alpha.calls).toHaveLength(1);
  expect(beta.calls).toHaveLength(0);
});
