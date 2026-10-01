import type { Response } from "@playwright/test";
import { expect, heading, test } from "./fixtures";

// crates/gateway/src/web.rs: the policy of the console, with a new nonce for
// every page (the amendment to Task 2 of the console plan).
const POLICY = new RegExp(
  "^default-src 'none'; script-src 'self'; style-src 'self' 'nonce-([A-Za-z0-9+/]{22,})'; " +
    "style-src-attr 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; " +
    "form-action 'self'; base-uri 'none'; frame-ancestors 'none'; manifest-src 'self'$",
);

const PAGE_HEADERS = {
  "x-content-type-options": "nosniff",
  "referrer-policy": "no-referrer",
  "x-frame-options": "DENY",
  "cross-origin-opener-policy": "same-origin",
  "permissions-policy": "camera=(), microphone=(), geolocation=()",
  // The page carries its own nonce, so it is never stored.
  "cache-control": "no-store",
};

const IMMUTABLE = "public, max-age=31536000, immutable";

/** The nonce of the policy of a page, after checking every header of the page. */
function checkPage(response: Response | null): string {
  expect(response?.status()).toBe(200);
  const headers = response?.headers() ?? {};
  expect(headers["content-type"]).toMatch(/^text\/html/);
  for (const [name, value] of Object.entries(PAGE_HEADERS)) {
    expect(headers[name], name).toBe(value);
  }
  expect(headers.etag, "a page is never the same twice: no ETag").toBeUndefined();
  const policy = headers["content-security-policy"] ?? "";
  const nonce = POLICY.exec(policy)?.[1];
  expect(nonce, `the policy is the console's: ${policy}`).toBeDefined();
  return nonce ?? "";
}

test("the page has the policy and the security headers; files are cached and sent in gzip", async ({
  page,
}) => {
  const files: Response[] = [];
  page.on("response", (response) => {
    const { pathname } = new URL(response.url());
    if (pathname.startsWith("/assets/") || pathname === "/theme.js") files.push(response);
  });

  const nonce = checkPage(await page.goto("/sign-in"));
  await expect(heading(page, "Sign in")).toBeVisible();
  // The page carries the nonce of its own policy, for the styles the libraries add.
  await expect(page.locator('meta[name="csp-nonce"]')).toHaveAttribute("content", nonce);

  // A deep link is the same page with a new nonce.
  const again = checkPage(await page.goto("/keys"));
  expect(again).not.toBe(nonce);

  const assets = files.filter((response) => new URL(response.url()).pathname.startsWith("/assets/"));
  const kinds = assets.map((response) => new URL(response.url()).pathname.split(".").pop());
  expect(kinds).toContain("js");
  expect(kinds).toContain("css");
  for (const response of assets) {
    const { pathname } = new URL(response.url());
    const headers = response.headers();
    expect(response.status(), pathname).toBe(200);
    expect(headers["cache-control"], pathname).toBe(IMMUTABLE);
    expect(headers["x-content-type-options"], pathname).toBe("nosniff");
    expect(headers.etag, pathname).toBeDefined();
    expect(headers["content-type"], pathname).toMatch(
      pathname.endsWith(".js") ? /^(text|application)\/javascript/ : /^text\/css/,
    );
    // The browser takes gzip, and gets it.
    expect(headers["content-encoding"], pathname).toBe("gzip");
    expect(headers.vary, pathname).toBe("accept-encoding");
  }

  // Asked for without gzip, the script comes as it is, with a tag of its own.
  const script = assets.find((response) => new URL(response.url()).pathname.endsWith(".js"));
  expect(script).toBeDefined();
  if (script !== undefined) {
    const plain = await page.request.get(script.url(), {
      headers: { "accept-encoding": "identity" },
    });
    expect(plain.status()).toBe(200);
    expect(plain.headers()["content-encoding"]).toBeUndefined();
    expect(plain.headers().vary).toBe("accept-encoding");
    expect(plain.headers().etag).not.toBe(script.headers().etag);
    expect(plain.headers()["cache-control"]).toBe(IMMUTABLE);
    expect((await plain.body()).equals(await script.body())).toBe(true);
  }

  // The theme script is at the root of the build: it is checked each time, not kept.
  const theme = files.find((response) => new URL(response.url()).pathname === "/theme.js");
  expect(theme?.status()).toBe(200);
  expect(theme?.headers()["cache-control"]).toBe("no-cache");
  expect(theme?.headers().etag).toBeDefined();
});
