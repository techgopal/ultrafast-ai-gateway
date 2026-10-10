import { createHmac, timingSafeEqual } from "node:crypto";
import { createServer, type IncomingHttpHeaders } from "node:http";
import type { AddressInfo } from "node:net";
import type { Page } from "@playwright/test";
import { expect, goTo, heading, isNarrow, openNavigation, signInFromStart, test } from "./fixtures";

interface Delivery {
  headers: IncomingHttpHeaders;
  path: string | undefined;
  body: Buffer;
}

/**
 * A receiver in the test, on a port the system gives out: it keeps what it
 * is sent and answers with the status it is told to.
 */
async function startReceiver() {
  const received: Delivery[] = [];
  const state = { status: 200 };
  const server = createServer((request, response) => {
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer) => chunks.push(chunk));
    request.on("end", () => {
      received.push({
        headers: request.headers,
        path: request.url,
        body: Buffer.concat(chunks),
      });
      response.statusCode = state.status;
      response.end("received");
    });
  });
  await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
  const { port } = server.address() as AddressInfo;
  return {
    url: `http://127.0.0.1:${String(port)}/hook/e2e-token`,
    received,
    answerWith: (status: number) => {
      state.status = status;
    },
    close: () => new Promise<void>((done) => server.close(() => { done(); })),
  };
}

/**
 * Whether `header` (`t=<unix seconds>,v1=<hex>`) is the HMAC-SHA256 of
 * `<t>.<raw body>` with `secret`, as the console tells a receiver to check,
 * and `t` is recent.
 */
function signedWith(secret: string, header: string | string[] | undefined, body: Buffer): boolean {
  if (typeof header !== "string") return false;
  const match = /^t=(\d+),v1=([0-9a-f]{64})$/.exec(header);
  if (match === null) return false;
  const [, t = "", v1 = ""] = match;
  if (Math.abs(Date.now() / 1000 - Number(t)) > 300) return false;
  const expected = createHmac("sha256", secret).update(`${t}.`).update(body).digest("hex");
  return timingSafeEqual(Buffer.from(expected), Buffer.from(v1));
}

/** The rows of a table, or the cards of its list on a narrow screen. */
function entriesOf(page: Page, caption: string) {
  return isNarrow(page)
    ? page.getByRole("list", { name: caption }).locator("> li")
    : page.getByRole("table", { name: caption }).getByRole("row");
}

/** Opens the Alerts page through the navigation, and one of its views. */
async function openAlerts(page: Page, view?: "Channels" | "History") {
  await goTo(page, "Alerts");
  if (view === undefined) return;
  await page.getByRole("navigation", { name: "Alerts sections" }).getByRole("link", { name: view }).click();
}

test("a signed test notification reaches a receiver, and a rotated secret signs the next", async ({
  page,
  admin,
}) => {
  const receiver = await startReceiver();
  try {
    await signInFromStart(page, admin);
    await openAlerts(page, "Channels");
    await expect(page.getByText("No channels")).toBeVisible();

    // Add the channel; its signing secret is shown once.
    await page.getByRole("button", { name: "Add channel" }).click();
    const form = page.getByRole("dialog", { name: "Add channel" });
    await form.getByLabel("Name", { exact: true }).fill("e2e-receiver");
    await form.getByLabel("URL", { exact: true }).fill(receiver.url);
    await form.getByRole("button", { name: "Add channel" }).click();
    const shown = page.getByRole("dialog", { name: "Signing secret" });
    await expect(shown).toContainText("Copy this signing secret now. It is not shown again.");
    const secret = await shown.getByLabel("Signing secret").inputValue();
    expect(secret).toMatch(/^whsec_[0-9a-f]{64}$/);
    // Copy puts it on the clipboard.
    await shown.getByRole("button", { name: "Copy" }).click();
    await expect(shown.getByRole("status")).toHaveText("Copied");
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(secret);
    await shown.getByRole("button", { name: "Done" }).click();
    await page.getByRole("alertdialog").getByRole("button", { name: "Close" }).click();
    await expect(shown).toBeHidden();

    // The list shows the host, never the URL.
    const row = entriesOf(page, "Alert channels").filter({ hasText: "e2e-receiver" });
    await expect(row).toContainText(new URL(receiver.url).origin);
    await expect(page.locator("body")).not.toContainText("e2e-token");

    // Send test: the receiver is told, with a signature it can check.
    await row.getByRole("button", { name: "Send test" }).click();
    await expect(page.getByText("Test to e2e-receiver: the receiver answered 200.")).toBeVisible();
    expect(receiver.received).toHaveLength(1);
    const first = receiver.received[0];
    expect(first?.path).toBe("/hook/e2e-token");
    expect(first?.headers["content-type"]).toContain("application/json");
    expect(signedWith(secret, first?.headers["x-uf-signature"], first?.body ?? Buffer.alloc(0))).toBe(true);
    // Another secret, or another body, does not verify.
    expect(signedWith(`${secret}x`, first?.headers["x-uf-signature"], first?.body ?? Buffer.alloc(0))).toBe(false);
    expect(
      signedWith(secret, first?.headers["x-uf-signature"], Buffer.from(`${first?.body.toString() ?? ""} `)),
    ).toBe(false);
    const payload = JSON.parse(first?.body.toString() ?? "{}") as Record<string, unknown>;
    expect(payload).toMatchObject({
      version: 1,
      state: "test",
      summary: "Test notification from the Ultrafast gateway",
      rule: { id: null, name: "Test", kind: "test" },
    });

    // Rotate the secret: the next delivery is signed with the new one only.
    await row.getByRole("button", { name: "Rotate secret" }).click();
    await page
      .getByRole("alertdialog", { name: "Rotate the secret of e2e-receiver?" })
      .getByRole("button", { name: "Rotate secret" })
      .click();
    const rotated = page.getByRole("dialog", { name: "New signing secret" });
    const next = await rotated.getByLabel("New signing secret").inputValue();
    expect(next).toMatch(/^whsec_[0-9a-f]{64}$/);
    expect(next).not.toBe(secret);
    await rotated.getByRole("button", { name: "Done" }).click();
    await page.getByRole("alertdialog").getByRole("button", { name: "Close" }).click();
    await expect(rotated).toBeHidden();

    await row.getByRole("button", { name: "Send test" }).click();
    await expect.poll(() => receiver.received.length).toBe(2);
    const second = receiver.received[1];
    expect(signedWith(next, second?.headers["x-uf-signature"], second?.body ?? Buffer.alloc(0))).toBe(true);
    expect(signedWith(secret, second?.headers["x-uf-signature"], second?.body ?? Buffer.alloc(0))).toBe(false);

    // A receiver that refuses: the result says so.
    receiver.answerWith(500);
    await row.getByRole("button", { name: "Send test" }).click();
    await expect(
      page.getByText("Test to e2e-receiver failed: the receiver answered 500."),
    ).toBeVisible();
    expect(receiver.received).toHaveLength(3);

    // A rule that tells the channel.
    await page.getByRole("navigation", { name: "Alerts sections" }).getByRole("link", { name: "Rules" }).click();
    await page.getByRole("button", { name: "Add rule" }).click();
    const rule = page.getByRole("dialog", { name: "Add rule" });
    await rule.getByLabel("Name", { exact: true }).fill("Any circuit");
    await rule.getByRole("radio", { name: "Circuit" }).click();
    await rule.getByRole("checkbox", { name: "e2e-receiver" }).click();
    await rule.getByRole("button", { name: "Add rule" }).click();
    const ruleRow = entriesOf(page, "Alert rules").filter({ hasText: "Any circuit" });
    await expect(ruleRow).toContainText("Circuit opens on any target");
    await expect(ruleRow).toContainText("e2e-receiver");
    await expect(ruleRow.getByRole("switch", { name: "Any circuit" })).toBeChecked();

    // The history has the three tests, with what happened to each.
    await page.getByRole("navigation", { name: "Alerts sections" }).getByRole("link", { name: "History" }).click();
    const tests = entriesOf(page, "Alert history").filter({
      hasText: "Test notification from the Ultrafast gateway",
    });
    await expect(tests).toHaveCount(3);
    await expect(page.getByRole("main")).toContainText("e2e-receiver: delivered (200)");
    await expect(page.getByRole("main")).toContainText("e2e-receiver: failed, the receiver answered 500");
    await page.getByRole("combobox", { name: "State" }).click();
    await page.getByRole("option", { name: "Firing" }).click();
    await expect(page.getByText("No events match")).toBeVisible();
  } finally {
    await receiver.close();
  }
});

test("a member has no Alerts in the navigation, and the address says it is not available", async ({
  page,
  admin,
  apiAs,
}) => {
  const member = await (await apiAs(admin)).activeUser("Mia");
  await signInFromStart(page, member);
  const nav = await openNavigation(page);
  await expect(nav.getByRole("link", { name: "Alerts" })).toHaveCount(0);
  await page.goto("/alerts");
  await expect(heading(page, "Not available")).toBeVisible();
  await expect(page.getByText("This page is not available to your account.")).toBeVisible();
});
