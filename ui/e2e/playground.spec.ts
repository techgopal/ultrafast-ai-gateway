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

test("a tool call is shown, its result is sent back, and the model answers with text", async ({
  page,
  admin,
  apiAs,
}) => {
  await setup(await apiAs(admin));
  await signInFromStart(page, admin);
  await goTo(page, "Playground");
  await picker(page).click();
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();

  await page.getByRole("button", { name: "Tools" }).click();
  await page.getByRole("textbox", { name: "Tools" }).fill(
    JSON.stringify([
      {
        type: "function",
        function: {
          name: "weather",
          description: "The weather in a city.",
          parameters: { type: "object", properties: { city: { type: "string" } } },
        },
      },
    ]),
  );
  await box(page).fill("What is the weather in Oslo?");
  await page.getByRole("button", { name: "Send" }).click();

  // The call arrives in pieces and is shown whole, with its arguments indented.
  const call = page.getByRole("group", { name: "Tool call weather" });
  await expect(call).toBeVisible();
  await expect(call.locator("pre")).toHaveText('{\n  "city": "Oslo"\n}');
  await expect(page.getByRole("button", { name: "Send", exact: true })).toBeDisabled();
  expect(mock.roles).toEqual([["user"]]);

  await call.getByLabel("Tool result").fill("3 degrees and clear");
  await page.getByRole("button", { name: "Send results" }).click();
  await expect(page.getByRole("list", { name: "Conversation" })).toContainText(mock.answer);
  await expect(page.getByRole("button", { name: "Send results" })).toHaveCount(0);
  // The second request carried the call and its result.
  expect(mock.roles).toEqual([["user"], ["user", "assistant", "tool"]]);
});

test("an image is attached, sent as a part and shown in the thread", async ({ page, admin, apiAs }) => {
  await setup(await apiAs(admin));
  await signInFromStart(page, admin);
  await goTo(page, "Playground");
  await picker(page).click();
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();
  // A one pixel PNG.
  const pixel = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
    "base64",
  );
  await page.getByLabel("Attach image").setInputFiles({ name: "pixel.png", mimeType: "image/png", buffer: pixel });
  await expect(page.getByRole("img", { name: "pixel.png" })).toBeVisible();
  await box(page).fill("What is this?");
  await page.getByRole("button", { name: "Send" }).click();
  const thread = page.getByRole("list", { name: "Conversation" });
  await expect(thread).toContainText(mock.answer);
  await expect(thread.getByRole("img", { name: "pixel.png" })).toBeVisible();
});

test("an admin generates images: the prompt, size and number reach the provider, the images are shown, and the call is logged and priced from its usage", async ({
  page,
  admin,
  apiAs,
}) => {
  const api = await apiAs(admin);
  await setup(api);
  await signInFromStart(page, admin);
  await goTo(page, "Playground");

  await page.getByRole("button", { name: "Images" }).click();
  await picker(page).click();
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();
  await page.getByRole("textbox", { name: "Prompt" }).fill("A red fox");
  await page.getByRole("textbox", { name: "Number of images" }).fill("2");
  await page.getByRole("button", { name: "Generate" }).click();

  const shown = page.getByRole("list", { name: "Generated images" }).getByRole("img");
  await expect(shown).toHaveCount(2);
  // The pictures decode: they are real PNGs of the mock.
  await expect
    .poll(() => shown.first().evaluate((img: HTMLImageElement) => img.naturalWidth))
    .toBe(1);
  await expect(page.getByText("Tokens: 3 in, 7 out. Cost: $0.00017.")).toBeVisible();

  expect(mock.imageCalls).toHaveLength(1);
  expect(mock.imageCalls[0]).toMatchObject({
    authorized: true,
    body: { model: "e2e-model", prompt: "A red fox", n: 2, size: "1024x1024" },
  });
  await expect.poll(async () => (await api.logs()).length, { timeout: 15_000 }).toBe(1);
  const [row] = await api.logs();
  expect(row).toMatchObject({
    endpoint: "playground",
    status: 200,
    stream: false,
    cost_micros: 170,
    user_email: admin.email,
  });
});

test("an admin transcribes a file and plays a speech: the file reaches the provider as a form, the audio plays from a blob, and both calls are logged", async ({
  page,
  admin,
  apiAs,
}) => {
  const api = await apiAs(admin);
  await setup(api);
  await signInFromStart(page, admin);
  await goTo(page, "Playground");
  await page.getByRole("button", { name: "Audio" }).click();

  // Transcribe: a small file is uploaded and its transcript is shown.
  await page.getByRole("combobox", { name: "Transcription model or route" }).click();
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();
  await page.getByLabel("Audio file").setInputFiles({
    name: "talk.mp3",
    mimeType: "audio/mpeg",
    buffer: Buffer.alloc(5000, 7),
  });
  await page.getByRole("textbox", { name: "Language" }).fill("en");
  await page.getByRole("button", { name: "Transcribe" }).click();
  await expect(page.getByRole("region", { name: "Transcript" })).toContainText("Hello from the mock recording.");

  // Speak: the audio the provider made plays from a blob URL (the policy allows blob media).
  await page.getByRole("combobox", { name: "Speech model or route" }).click();
  await page.getByRole("option", { name: "alpha/e2e-model" }).click();
  await page.getByRole("textbox", { name: "Text to speak" }).fill("Good morning");
  await page.getByRole("button", { name: "Speak" }).click();
  const player = page.getByLabel("Speech", { exact: true });
  await expect(player).toBeVisible();
  await expect(player).toHaveAttribute("src", /^blob:/);
  await expect
    .poll(() => player.evaluate((audio: HTMLAudioElement) => audio.duration), { timeout: 15_000 })
    .toBeGreaterThan(0);

  expect(mock.audioCalls).toHaveLength(2);
  const [stt, tts] = mock.audioCalls;
  expect(stt).toMatchObject({
    path: "/v1/audio/transcriptions",
    authorized: true,
    fileBytes: 5000,
    fields: expect.arrayContaining(["model", "language"]),
  });
  expect(stt?.contentType).toMatch(/^multipart\/form-data; boundary=/);
  expect(tts).toMatchObject({
    path: "/v1/audio/speech",
    authorized: true,
    body: { model: "e2e-model", input: "Good morning", voice: "alloy" },
  });
  await expect.poll(async () => (await api.logs()).length, { timeout: 15_000 }).toBe(2);
  const rows = await api.logs();
  expect(rows.map((row) => [row.endpoint, row.status]).sort()).toEqual([
    ["playground", 200],
    ["playground", 200],
  ]);
  expect(rows.every((row) => row.user_email === admin.email)).toBe(true);
});
