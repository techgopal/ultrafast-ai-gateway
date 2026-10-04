import {
  expect,
  expectForm,
  expectNowhere,
  goTo,
  goToAuditLog,
  itemOf,
  KEY_DISPLAY,
  KEY_SECRET,
  signInFromStart,
  test,
  typeSecret,
} from "./fixtures";
import { startMockProvider, type MockProvider } from "./mock-provider";

let mock: MockProvider;

test.beforeEach(async () => {
  mock = await startMockProvider();
});

test.afterEach(async () => {
  await mock.close();
});

test("a provider, its models, a key, calls through the gateway, and the key revoked", async ({
  page,
  admin,
  request,
}) => {
  await signInFromStart(page, admin);

  // The provider points at the mock.
  await goTo(page, "Providers");
  await page.getByRole("button", { name: "Add provider" }).click();
  const add = page.getByRole("dialog", { name: "Add provider" });
  await add.getByLabel("Name", { exact: true }).fill("mock");
  await add.getByLabel("Base URL", { exact: true }).fill(mock.baseUrl);
  await typeSecret(add.getByLabel("API key", { exact: true }), mock.apiKey);
  await add.getByRole("button", { name: "Add provider" }).click();
  await expect(add).toBeHidden();
  await expect(page.getByText("Provider added")).toBeVisible();
  const provider = itemOf(page, "Providers", "mock");
  await expect(provider).toContainText(mock.baseUrl);
  await expect(provider).toContainText("Set");
  await expectNowhere(page, mock.apiKey, "the provider's API key");

  // The models of the provider are read; they start disabled. One is enabled
  // and given to everyone: only then can a key call it.
  await goTo(page, "Models");
  await page.getByRole("button", { name: "Sync models" }).click();
  const sync = page.getByRole("dialog", { name: "Sync models" });
  await sync.getByRole("radio", { name: "mock" }).check();
  await sync.getByRole("button", { name: "Sync" }).click();
  await expect(sync).toBeHidden();
  await expect(
    page.getByText("Added 2 models. They start disabled."),
  ).toBeVisible();
  expect(mock.listCalls).toBe(1);
  const model = itemOf(page, "Models", "e2e-model");
  await expect(model).toContainText("Disabled");
  await model.getByRole("switch", { name: "e2e-model" }).click();
  await expect(model).toContainText("Enabled");
  await expect(model).toContainText("Enabled, but nobody has access yet.");
  await model.getByRole("button", { name: "Edit access" }).click();
  const access = page.getByRole("dialog", { name: "Edit access" });
  await access.getByRole("switch", { name: "Everyone" }).click();
  await access.getByRole("button", { name: "Save access" }).click();
  await expect(access).toBeHidden();
  await expect(page.getByText("Access updated.")).toBeVisible();
  await expect(model).not.toContainText("nobody has access yet");

  // A key, copied from the dialog that shows it once.
  await goTo(page, "Virtual keys");
  await page.getByRole("button", { name: "Create key" }).click();
  const create = page.getByRole("dialog", { name: "Create key" });
  await create.getByLabel("Name", { exact: true }).fill("first call");
  await create.getByRole("button", { name: "Create key" }).click();
  const shown = page.getByRole("dialog", { name: "Your new key" });
  await expect(shown).toBeVisible();
  await shown.getByRole("button", { name: "Copy" }).click();
  await expect(shown.getByRole("status")).toHaveText("Copied");
  const key = await page.evaluate(() => navigator.clipboard.readText());
  expectForm(key, KEY_SECRET, "the key");
  expect(
    key ===
      (await shown.getByRole("textbox", { name: "Your new key" }).inputValue()),
  ).toBe(true);
  await shown.getByRole("button", { name: "Done" }).click();
  await page
    .getByRole("alertdialog", { name: "Close this dialog?" })
    .getByRole("button", { name: "Close" })
    .click();
  await expect(shown).toBeHidden();
  await expectNowhere(page, key, "the key, once its dialog is closed,");

  // The list shows the key by its last four characters only.
  const display = `uf-sk-…${key.slice(-4)}`;
  expectForm(display, KEY_DISPLAY, "the display of the key");
  const row = itemOf(page, "Virtual keys", "first call");
  await expect(row).toContainText(display);
  await expect(row).toContainText("active");

  // A call with the key reaches the mock, with the provider's key, and its answer comes back.
  const call = () =>
    request.post("/v1/chat/completions", {
      headers: { authorization: `Bearer ${key}` },
      data: {
        model: "mock/e2e-model",
        messages: [{ role: "user", content: "Hello" }],
      },
    });
  const answered = await call();
  expect(answered.status()).toBe(200);
  const body = (await answered.json()) as {
    choices: { message: { content: string } }[];
  };
  expect(body.choices[0]?.message.content).toBe(mock.answer);
  expect(mock.calls).toEqual([{ authorized: true, model: "e2e-model" }]);

  // The Anthropic form: the key goes in `x-api-key`, and the answer is a message.
  const asked = await request.post("/v1/messages", {
    headers: { "x-api-key": key, "anthropic-version": "2023-06-01" },
    data: {
      model: "mock/e2e-model",
      max_tokens: 64,
      messages: [{ role: "user", content: "Hello" }],
    },
  });
  expect(asked.status()).toBe(200);
  const message = (await asked.json()) as {
    type: string;
    role: string;
    content: { type: string; text: string }[];
    stop_reason: string;
  };
  expect(message.type).toBe("message");
  expect(message.role).toBe("assistant");
  expect(message.content[0]).toEqual({ type: "text", text: mock.answer });
  expect(message.stop_reason).toBe("end_turn");
  expect(mock.calls).toHaveLength(2);

  // A model that is not enabled is refused.
  const refusedModel = await request.post("/v1/chat/completions", {
    headers: { authorization: `Bearer ${key}` },
    data: {
      model: "mock/e2e-other",
      messages: [{ role: "user", content: "Hello" }],
    },
  });
  expect(refusedModel.status()).toBe(403);
  expect(mock.calls).toHaveLength(2);

  // Revoked, the key is refused, and the mock is not called again.
  await row.getByRole("button", { name: "Revoke" }).click();
  const confirm = page.getByRole("alertdialog", { name: "Revoke first call?" });
  await confirm.getByRole("button", { name: "Revoke" }).click();
  await expect(confirm).toBeHidden();
  await expect(page.getByText("Key revoked.")).toBeVisible();
  await expect(row).toBeHidden();
  await page.getByRole("checkbox", { name: "Show revoked" }).click();
  await expect(row).toContainText("revoked");

  const refused = await call();
  expect(refused.status()).toBe(401);
  const refusedMessage = await request.post("/v1/messages", {
    headers: { "x-api-key": key, "anthropic-version": "2023-06-01" },
    data: {
      model: "mock/e2e-model",
      max_tokens: 64,
      messages: [{ role: "user", content: "Hello" }],
    },
  });
  expect(refusedMessage.status()).toBe(401);
  expect(mock.calls).toHaveLength(2);

  // The audit log has what was done, in the gateway's own action names and
  // summaries (the forms the unit fixtures pin: src/test/fixtures.test.ts).
  await goToAuditLog(page);
  const done: [string, string][] = [
    ["setup.create_admin", `Created the first admin ${admin.email}`],
    ["auth.login", `${admin.email} signed in`],
    ["provider.create", "Created provider mock (openai), credential set"],
    ["key.create", `Created key first call (${display}) for ${admin.email}`],
    ["key.revoke", `Revoked key first call (${display})`],
  ];
  for (const [action, summary] of done) {
    const entry = itemOf(page, "Audit log", summary);
    await expect(entry.first(), action).toContainText(action);
    await expect(entry.first(), action).toContainText(admin.email);
  }
});
