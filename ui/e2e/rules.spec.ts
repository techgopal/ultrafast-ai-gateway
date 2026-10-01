// The rules every browser test is held to (see `BrowserRules` in fixtures.ts)
// are checked here against what breaks them, so that a test that passes
// says something.
import { expect, expectNowhere, heading, test } from "./fixtures";

test("the rules of the browser tests see what breaks them", async ({ page, rules, gateway }) => {
  await page.goto("/sign-in");
  await expect(heading(page, "Sign in")).toBeVisible();
  // The page asked `/api/auth/me` before sign-in and was answered 401: that is no problem.
  expect(rules.problems).toEqual([]);

  // `expectNowhere` finds a text where the page keeps it.
  const marker = "uf-sk-0123456789abcdef";
  await expectNowhere(page, marker, "the marker");
  await page.evaluate((text) => {
    sessionStorage.setItem("kept", text);
  }, marker);
  await expect(expectNowhere(page, marker, "the marker")).rejects.toThrow(/sessionStorage/);
  await page.evaluate(() => {
    sessionStorage.clear();
  });
  await page.getByLabel("Email", { exact: true }).fill(marker);
  await expect(expectNowhere(page, marker, "the marker")).rejects.toThrow(/fields/);

  const seen: string[] = [];
  const expectSeen = async (what: RegExp) => {
    await expect
      .poll(() => rules.problems.some((problem) => what.test(problem)), { message: String(what) })
      .toBe(true);
    seen.push(...rules.problems.splice(0));
  };

  // A refusal of the API is let through; a 404 of a file is not.
  await page.evaluate(async () => {
    await fetch("/api/users");
  });
  await page.evaluate(async () => {
    await fetch("/assets/none.js");
  });
  await expectSeen(/^console error: Failed to load resource: .* 404/);
  expect(seen).toContain("request of the app outside /api: /assets/none.js");
  expect(seen.some((problem) => problem.includes("401"))).toBe(false);

  // An error the page logs, and one it throws.
  await page.evaluate(() => {
    console.error("an error of the page");
  });
  await expectSeen(/^console error: an error of the page$/);
  await page.evaluate(() => {
    setTimeout(() => {
      throw new Error("thrown by the page");
    });
  });
  await expectSeen(/^uncaught error: thrown by the page$/);

  // A style element without the nonce breaks the policy.
  await page.evaluate(() => {
    const style = document.createElement("style");
    style.textContent = "body { margin: 0 }";
    document.head.append(style);
  });
  await expectSeen(/^Content Security Policy violation: style-src-elem blocked inline/);

  // An answer 5xx of the gateway.
  await page.route("**/api/broken", (route) => route.fulfill({ status: 500, body: "" }));
  await page.evaluate(async () => {
    await fetch("/api/broken");
  });
  await expectSeen(new RegExp(`^answer 500: ${gateway.origin}/api/broken$`));

  // A request to another origin is not sent. (The policy stops a `fetch`
  // before it goes out; a navigation it does not.)
  const elsewhere = "http://127.0.0.1:9/elsewhere";
  await expect(page.goto(elsewhere)).rejects.toThrow();
  await expectSeen(new RegExp(`^request to another origin: ${elsewhere}$`));

  // What was seen here is what this test made; the test itself passes.
  rules.problems.splice(0);
});
