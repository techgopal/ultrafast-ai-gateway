import {
  expect,
  expectForm,
  expectNowhere,
  goTo,
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

test("a provider, a key, a call through the gateway, and the key revoked", async ({
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
  expect(key === (await shown.getByRole("textbox", { name: "Your new key" }).inputValue())).toBe(
    true,
  );
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
      data: { model: "mock/e2e-model", messages: [{ role: "user", content: "Hello" }] },
    });
  const answered = await call();
  expect(answered.status()).toBe(200);
  const body = (await answered.json()) as { choices: { message: { content: string } }[] };
  expect(body.choices[0]?.message.content).toBe(mock.answer);
  expect(mock.calls).toEqual([{ authorized: true, model: "e2e-model" }]);

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
  expect(mock.calls).toHaveLength(1);

  // The audit log has what was done, in the gateway's own action names and
  // summaries (the forms the unit fixtures pin: src/test/fixtures.test.ts).
  await goTo(page, "Audit log");
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
