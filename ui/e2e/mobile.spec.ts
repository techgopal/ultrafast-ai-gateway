import type { Locator, Page } from "@playwright/test";
import {
  expect,
  goTo,
  goToAuditLog,
  heading,
  menuButton,
  signInFromStart,
  test,
  type GatewayApi,
} from "./fixtures";

// A phone, in both projects: the desktop project has no touch, the phone project has.
test.use({ viewport: { width: 390, height: 844 } });

async function scrollsSideways(page: Page): Promise<boolean> {
  return page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
  );
}

/** Waits for what moves to end: a dialog that opens grows into its size. Endless ones are left. */
async function settled(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const ending = document
      .getAnimations()
      .filter((animation) => animation.effect?.getComputedTiming().endTime !== Infinity);
    await Promise.all(ending.map((animation) => animation.finished.catch(() => undefined)));
  });
}

/**
 * The controls in `scope` that are less than 44 x 44 px to touch, whatever
 * they show: a short text makes a narrow link. A control inside its label is
 * touched through the label as well: its target is the label.
 */
async function smallTargets(scope: Locator): Promise<string[]> {
  await settled(scope.page());
  return scope.evaluate((root) => {
    const controls = root.querySelectorAll<HTMLElement>(
      "a[href], button, input, select, textarea, [role=checkbox], [role=radio], [role=combobox]",
    );
    const small: string[] = [];
    for (const control of controls) {
      // Not for the user: hidden, or a copy for the form of the browser.
      if (!control.checkVisibility({ visibilityProperty: true, opacityProperty: true })) continue;
      if (control.closest('[aria-hidden="true"]') !== null) continue;
      const target = control.closest("label") ?? control;
      const { width, height } = target.getBoundingClientRect();
      const text = target.textContent.trim();
      if (height >= 44 && width >= 44) continue;
      const role = control.getAttribute("role") ?? control.tagName.toLowerCase();
      const labels = control instanceof HTMLButtonElement ? [...control.labels] : [];
      const label = labels.map((one) => one.textContent.trim()).join(" ");
      const name = control.getAttribute("aria-label") ?? (label === "" ? text : label).slice(0, 40);
      small.push(`${role} "${name}": ${String(Math.round(width))} x ${String(Math.round(height))}`);
    }
    return small;
  });
}

/** Touches the element near its top right corner: where neither a box nor a text is. */
async function touchEdge(target: Locator): Promise<void> {
  await settled(target.page());
  const box = await target.boundingBox();
  expect(box).not.toBeNull();
  if (box === null) return;
  expect(box.height).toBeGreaterThanOrEqual(44);
  await target.click({ position: { x: box.width - 2, y: 2 } });
}

/** Long texts, which a narrow screen must wrap: a page is no wider for them. */
async function content(api: GatewayApi) {
  const user = await api.activeUser("Christina-Alexandra Montgomery-Weatherby");
  const team = await api.createTeam("Platform infrastructure and developer experience");
  await api.putMember(team, user.id, "lead");
  const provider = (await api.send("POST", "/api/providers", {
    name: "a-provider-with-a-name-of-forty-letters",
    kind: "openai",
    base_url: "https://a-rather-long-host-name.inference.example.test/api/openai/compatible/v1",
  })) as { id: number };
  const model = (await api.send("POST", "/api/models", {
    provider_id: provider.id,
    name: "a-model-with-a-very-long-name-of-the-newest-generation-v2",
  })) as { id: number };
  await api.enable(model.id);
  await api.createRoute({
    name: "a-route-with-a-long-name-for-the-nightly-summaries",
    primaries: [{ model_id: model.id, weight: 1 }],
    everyone: true,
  });
  await api.send("POST", "/api/keys", {
    name: "The key of the nightly batch job that summarises support tickets",
    owner_id: user.id,
    team_id: team,
  });
  await api.send("POST", "/api/guardrails", {
    name: "a-guardrail-with-a-name-that-is-rather-long-for-a-phone",
    kind: "rules",
    is_default: true,
    rules: [
      { id: "email", matcher: { pii: ["EMAIL"] }, action: "redact", directions: "output" },
      {
        id: "words",
        matcher: { keywords: { words: ["a-keyword-that-is-rather-long-for-a-phone"] } },
        action: "flag",
        directions: "both",
      },
    ],
  });
  await api.send("POST", "/api/prompts", {
    name: "a-prompt-template-with-a-name-that-is-rather-long-for-a-phone",
    description: "A description that is long enough to need more than one line on a narrow screen.",
    model: "a-provider-with-a-name-of-forty-letters/a-model-with-a-very-long-name-of-the-newest-generation-v2",
    messages: [
      { role: "system", content: "You answer in the voice of {{a_variable_with_a_rather_long_name_for_a_phone}}." },
      { role: "user", content: "A line without a break that is rather long for a phone: " + "word ".repeat(40) },
    ],
  });
  const channel = (await api.send("POST", "/api/alerts/channels", {
    name: "a-channel-with-a-name-that-is-rather-long-for-a-phone",
    kind: "webhook",
    url: "https://a-rather-long-host-name.alerts.example.test/hooks/one",
  })) as { channel: { id: number } };
  await api.send("POST", "/api/alerts/rules", {
    name: "a-rule-with-a-name-that-is-also-rather-long-for-a-phone",
    kind: "error_rate",
    params: { scope: "route", subject: "a-route-with-a-long-name-for-the-nightly-summaries", percent: 10 },
    channel_ids: [channel.channel.id],
  });
}

test("every page fits a phone; the drawer opens and closes; rows are cards; the dialog fits", async ({
  page,
  admin,
  apiAs,
}) => {
  await content(await apiAs(admin));

  await page.goto("/sign-in");
  await expect(heading(page, "Sign in")).toBeVisible();
  expect(await scrollsSideways(page), "sign-in").toBe(false);

  await signInFromStart(page, admin);
  expect(await scrollsSideways(page), "overview").toBe(false);

  // The menu button opens the drawer; Escape closes it, and the focus goes back.
  const drawer = page.getByRole("dialog", { name: "Sidebar" });
  await expect(drawer).toHaveCount(0);
  await menuButton(page).click();
  await expect(drawer).toBeVisible();
  await expect(drawer.getByRole("navigation", { name: "Main" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
  await expect(menuButton(page)).toBeFocused();
  // A link of the drawer closes it too.
  await goTo(page, "Users");
  await expect(drawer).toBeHidden();

  // Each page with what it lists, so that it is measured with its content.
  const pages: [string, string][] = [
    ["Users", "Montgomery-Weatherby"],
    ["Teams", "developer experience"],
    ["Virtual keys", "nightly batch job"],
    ["Providers", "a-rather-long-host-name"],
    ["Models", "a-model-with-a-very-long-name"],
    ["Routing", "a-route-with-a-long-name"],
    ["Prompts", "a-prompt-template-with-a-name"],
    ["Alerts", "a-rule-with-a-name-that-is-also-rather-long"],
    ["Guardrails", "a-guardrail-with-a-name"],
    ["Playground", "Nothing has been said yet"],
    ["Account", "Access tokens"],
    ["Settings", "Session lifetime"],
    ["Audit log", "auth.login"],
  ];
  for (const [title, shows] of pages) {
    if (title === "Audit log") await goToAuditLog(page);
    else await goTo(page, title);
    await expect(page.getByRole("main")).toContainText(shows);
    expect(await scrollsSideways(page), title).toBe(false);
    if (title === "Prompts") {
      await page.getByRole("link", { name: "Open", exact: true }).first().click();
      await expect(page.getByRole("list", { name: "Messages of version 1" })).toBeVisible();
      expect(await scrollsSideways(page), "a prompt template").toBe(false);
      await goTo(page, "Prompts");
    }
    if (title === "Guardrails") {
      // The form of a guardrail, with its rules and Try it.
      await page.getByRole("link", { name: "Edit", exact: true }).first().click();
      await expect(heading(page, "Edit guardrail")).toBeVisible();
      await expect(page.getByRole("group", { name: "Rule 2" })).toBeVisible();
      expect(await scrollsSideways(page), "guardrail form").toBe(false);
      await goTo(page, "Guardrails");
    }
    if (title === "Alerts") {
      const views = page.getByRole("navigation", { name: "Alerts sections" });
      await views.getByRole("link", { name: "Channels" }).click();
      await expect(page.getByRole("main")).toContainText("a-channel-with-a-name");
      expect(await scrollsSideways(page), "alert channels").toBe(false);
      await views.getByRole("link", { name: "History" }).click();
      await expect(page.getByRole("table", { name: "Alert history" }).or(page.getByText("No alerts yet"))).toBeVisible();
      expect(await scrollsSideways(page), "alert history").toBe(false);
    }
  }

  // A row of a table is a card.
  await goTo(page, "Virtual keys");
  await expect(page.getByRole("table")).toHaveCount(0);
  const card = page
    .getByRole("list", { name: "Virtual keys" })
    .getByRole("listitem")
    .filter({ hasText: "nightly batch job" });
  await expect(card).toBeVisible();
  await expect(card.getByRole("term")).toHaveText(["Name", "Key", "Owner", "Team", "Models", "Tags", "Guardrails", "Expires", "Status"]);

  // The create-key dialog fits the screen, and its submit button can be reached.
  await page.getByRole("button", { name: "Create key" }).click();
  const dialog = page.getByRole("dialog", { name: "Create key" });
  await expect(dialog.getByLabel("Owner", { exact: true })).toBeVisible();
  const box = await dialog.boundingBox();
  expect(box).not.toBeNull();
  if (box !== null) {
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(390);
    expect(box.y + box.height).toBeLessThanOrEqual(844);
  }
  const submit = dialog.getByRole("button", { name: "Create key" });
  await submit.scrollIntoViewIfNeeded();
  await expect(submit).toBeInViewport({ ratio: 1 });
  await submit.click({ trial: true });
  expect(await scrollsSideways(page), "the dialog").toBe(false);
});

test("every control of the pages and their dialogs is 44 x 44 px to touch", async ({
  page,
  admin,
  apiAs,
}) => {
  await content(await apiAs(admin));
  // What is too small, everywhere: one list, so that a failure names every place.
  const found: string[] = [];
  const measure = async (scope: Locator, where: string) => {
    for (const small of await smallTargets(scope)) found.push(`${where}: ${small}`);
  };
  await page.goto("/sign-in");
  await expect(heading(page, "Sign in")).toBeVisible();
  await measure(page.locator("body"), "sign-in");
  await signInFromStart(page, admin);
  await measure(page.locator("body"), "overview");
  await menuButton(page).click();
  const drawer = page.getByRole("dialog", { name: "Sidebar" });
  await expect(drawer).toBeVisible();
  await measure(drawer, "the drawer");
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();

  /** Opens a dialog with a button of the page, measures it, and closes it with Escape. */
  async function measureDialog(opener: string, name: string = opener, waitFor?: string) {
    await page.getByRole("button", { name: opener, exact: true }).first().click();
    const dialog = page.getByRole("dialog", { name });
    await expect(dialog).toBeVisible();
    if (waitFor !== undefined) await expect(dialog.getByLabel(waitFor, { exact: true })).toBeVisible();
    await measure(dialog, `the dialog ${name}`);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
  }

  const pages: [string, string][] = [
    ["Users", "Montgomery-Weatherby"],
    ["Teams", "developer experience"],
    ["Virtual keys", "nightly batch job"],
    ["Providers", "a-rather-long-host-name"],
    ["Prompts", "a-prompt-template-with-a-name"],
    ["Alerts", "a-rule-with-a-name-that-is-also-rather-long"],
    ["Guardrails", "a-guardrail-with-a-name"],
    ["Playground", "Nothing has been said yet"],
    ["Account", "Access tokens"],
    ["Settings", "Session lifetime"],
    ["Audit log", "auth.login"],
  ];
  for (const [title, shows] of pages) {
    if (title === "Audit log") await goToAuditLog(page);
    else await goTo(page, title);
    await expect(page.getByRole("main")).toContainText(shows);
    await measure(page.locator("body"), title);
    if (title === "Prompts") {
      // One template, with its editor and a new one: a form of messages fits too.
      await page.getByRole("link", { name: "Open", exact: true }).first().click();
      await expect(page.getByRole("list", { name: "Messages of version 1" })).toBeVisible();
      await expect(page.getByRole("button", { name: "Save as version 2" })).toBeVisible();
      await measure(page.locator("body"), "a prompt template");
      await goTo(page, "Prompts");
      await page.getByRole("link", { name: "New template" }).click();
      await expect(heading(page, "New template")).toBeVisible();
      await measure(page.locator("body"), "a new prompt template");
      await goTo(page, "Prompts");
    }
    if (title === "Guardrails") {
      await page.getByRole("link", { name: "Edit", exact: true }).first().click();
      await expect(heading(page, "Edit guardrail")).toBeVisible();
      await expect(page.getByRole("group", { name: "Rule 2" })).toBeVisible();
      await measure(page.locator("body"), "the guardrail form");
      await goTo(page, "Guardrails");
    }
    if (title === "Alerts") {
      await measureDialog("Add rule", "Add rule", "Name");
      const views = page.getByRole("navigation", { name: "Alerts sections" });
      await views.getByRole("link", { name: "Channels" }).click();
      await expect(page.getByRole("main")).toContainText("a-channel-with-a-name");
      await measure(page.locator("body"), "alert channels");
      await measureDialog("Add channel", "Add channel", "Name");
      await views.getByRole("link", { name: "History" }).click();
      await measure(page.locator("body"), "alert history");
    }
    if (title === "Users") await measureDialog("Invite user");
    if (title === "Teams") await measureDialog("New team");
    if (title === "Virtual keys") await measureDialog("Create key", "Create key", "Owner");
    if (title === "Providers") {
      await measureDialog("Add provider");
      await measureDialog("Edit", "Edit provider");
    }
    if (title === "Account") await measureDialog("Create token");
  }

  // The page of a team, with the dialog that adds a member from the list of users.
  await goTo(page, "Teams");
  await page.getByRole("link", { name: "Platform infrastructure and developer experience" }).click();
  await expect(heading(page, "Platform infrastructure and developer experience")).toBeVisible();
  await measure(page.locator("body"), "a team");
  await measureDialog("Add member");

  // Every place is named before a touch is tried.
  expect(found).toEqual([]);

  // A touch at the edge of a label, away from its text and its box, chooses.
  await goTo(page, "Virtual keys");
  const showRevoked = page.locator("label").filter({ hasText: "Show revoked" });
  await touchEdge(showRevoked);
  await expect(page.getByRole("checkbox", { name: "Show revoked" })).toBeChecked();
  await page.getByRole("button", { name: "Create key", exact: true }).click();
  const create = page.getByRole("dialog", { name: "Create key" });
  await expect(create.getByLabel("Owner", { exact: true })).toBeVisible();
  const inDays = create.getByRole("radio", { name: "In 30 days" });
  await expect(inDays).not.toBeChecked();
  await touchEdge(create.locator("label").filter({ hasText: "In 30 days" }));
  await expect(inDays).toBeChecked();
  await page.keyboard.press("Escape");
  await expect(create).toBeHidden();
});
