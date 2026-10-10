// Single sign-on against a provider the test runs (mock-idp.ts).
//
// The provider is reached as `localhost` and the gateway as `127.0.0.1`: two
// sites, so the way back to the gateway is a cross-site navigation, with the
// session cookie that is SameSite=Strict. The console must still learn who is
// signed in (its first call to `/api/auth/me` is same-site).
import { startMockIdp, type MockIdp } from "./mock-idp";
import {
  expect,
  expectNowhere,
  goTo,
  heading,
  newAccount,
  signIn,
  signInFromStart,
  signOut,
  test,
  typeSecret,
  type Account,
} from "./fixtures";
import type { Page } from "@playwright/test";

test.use({ publicUrl: true });

const LABEL = "Test IdP";

let idp: MockIdp | undefined;

test.afterEach(async () => {
  await idp?.stop();
  idp = undefined;
});

/** The settings the gateway needs to offer the button, made through the API. */
function settingsFor(provider: MockIdp, more: Record<string, unknown> = {}) {
  return {
    enabled: true,
    label: LABEL,
    issuer: provider.issuer,
    client_id: provider.clientId,
    client_secret: provider.clientSecret,
    link_by_email: true,
    ...more,
  };
}

/** Walks the redirects of one sign-in: the hosts the page's own navigation went to. */
function watchNavigation(page: Page): string[] {
  const seen: string[] = [];
  page.on("request", (request) => {
    if (request.isNavigationRequest()) seen.push(new URL(request.url()).origin);
  });
  return seen;
}

async function me(page: Page) {
  return page.evaluate(async () => {
    const answer = await fetch("/api/auth/me");
    return (await answer.json()) as { user: { email: string; auth_provider: string; status: string } };
  });
}

test("an admin sets up single sign-on in Settings; an invited user then signs in with it, across sites", async ({
  page,
  admin,
  apiAs,
  gateway,
  rules,
  context,
  browserName,
}) => {
  const mia: Account = newAccount("mia");
  idp = await startMockIdp({ sub: "mia-at-idp", email: mia.email, name: "Mia Idp" });
  rules.allowSite(idp.origin);
  expect(new URL(idp.origin).hostname, "the provider is another site than the gateway").not.toBe(
    new URL(gateway.origin).hostname,
  );
  const api = await apiAs(admin);
  // Invited, no password set: the sign-in with SSO links this account by its verified email.
  const invited = await api.invite("Mia Idp", mia.email, "member");

  // The admin configures it in the console.
  await signInFromStart(page, admin);
  await goTo(page, "Settings");
  const section = page.getByRole("region", { name: "Single sign-on (OIDC)" });
  await expect(section.getByText(`${gateway.origin}/api/auth/oidc/callback`)).toBeVisible();
  await section.getByRole("button", { name: "Copy address" }).click();
  // Only Chromium lets a test read the clipboard.
  if (browserName === "chromium") {
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
      `${gateway.origin}/api/auth/oidc/callback`,
    );
  }

  await section.getByLabel("Label on the sign-in button").fill(LABEL);
  await section.getByLabel("Issuer").fill(idp.issuer);
  await section.getByLabel("Client ID").fill(idp.clientId);
  await typeSecret(section.getByLabel("Client secret"), idp.clientSecret);
  await section.getByRole("switch", { name: "Single sign-on" }).click();
  await section.getByRole("button", { name: "Test configuration" }).click();
  await expect(section.getByRole("status")).toContainText(
    `The provider answers: ${idp.issuer}, 1 signing key.`,
  );
  await section.getByRole("button", { name: "Save single sign-on" }).click();
  await expect(page.getByText("Single sign-on settings saved.")).toBeVisible();
  // The secret is write only: the field is empty, and nowhere in the page.
  await expect(section.getByLabel("Replace client secret")).toHaveValue("");
  await expectNowhere(page, idp.clientSecret, "the client secret");
  await page.reload();
  await expect(section.getByLabel("Replace client secret")).toHaveValue("");
  await expect(section.getByLabel("Issuer")).toHaveValue(idp.issuer);
  await expectNowhere(page, idp.clientSecret, "the client secret");

  // Sign out: the sign-in page offers the button above the password form.
  await signOut(page);
  const button = page.getByRole("link", { name: `Sign in with ${LABEL}` });
  await expect(button).toBeVisible();
  const hosts = watchNavigation(page);
  await button.click();
  await expect(heading(page, "Overview")).toBeVisible();

  // The browser went to the provider and came back to the gateway.
  expect(hosts).toContain(idp.origin);
  expect(hosts.at(-1)).toBe(gateway.origin);
  expect(idp.requests).toEqual(
    expect.arrayContaining([
      "GET /.well-known/openid-configuration",
      "GET /authorize",
      "POST /token",
      "GET /jwks",
    ]),
  );
  // The console learned who is signed in from `/api/auth/me`, with the Strict cookie.
  const who = await me(page);
  expect(who.user).toMatchObject({ email: mia.email, auth_provider: "oidc", status: "active" });
  const cookies = await context.cookies();
  expect(cookies).toHaveLength(1);
  expect(cookies[0]).toMatchObject({ httpOnly: true, sameSite: "Strict" });
  await expectNowhere(page, cookies[0]?.value ?? "", "the session cookie");
  await expectNowhere(page, idp.clientSecret, "the client secret");
  expect(gateway.output()).not.toContain(idp.clientSecret);
  expect(invited.id).toBeGreaterThan(0);

  // Signed out, a deep link comes back to the same page after SSO.
  await signOut(page);
  await page.goto("/keys");
  await expect(page).toHaveURL(/\/sign-in\?next=%2Fkeys$/);
  await page.getByRole("link", { name: `Sign in with ${LABEL}` }).click();
  await expect(heading(page, "Virtual keys")).toBeVisible();
  await expect(page).toHaveURL(/\/keys$/);
  await signOut(page);

  // The admin sees how Mia signs in.
  await signIn(page, admin);
  await expect(heading(page, "Overview")).toBeVisible();
  await goTo(page, "Users");
  const row = page
    .getByRole("table", { name: "Users" })
    .getByRole("row")
    .filter({ hasText: mia.email })
    .or(page.getByRole("list", { name: "Users" }).getByRole("listitem").filter({ hasText: mia.email }));
  await expect(row).toContainText("SSO");
  await page.getByRole("combobox", { name: "Sign-in" }).click();
  await page.getByRole("option", { name: "Password" }).click();
  await expect(row).toHaveCount(0);
  await page.getByRole("combobox", { name: "Sign-in" }).click();
  await page.getByRole("option", { name: "SSO" }).click();
  await expect(row).toHaveCount(1);
});

test("a refusal by the provider, and an account that is not allowed, are said on the sign-in page", async ({
  page,
  admin,
  apiAs,
  rules,
}) => {
  idp = await startMockIdp({ sub: "stranger", email: newAccount("nobody").email });
  rules.allowSite(idp.origin);
  const api = await apiAs(admin);
  await api.send("PUT", "/api/settings/oidc", settingsFor(idp));

  await page.goto("/sign-in");
  await page.getByRole("link", { name: `Sign in with ${LABEL}` }).click();
  // Nobody invited this address, and accounts are not made on first sign-in.
  await expect(page.getByRole("alert")).toHaveText(
    "Your account is not allowed to sign in here. Ask an admin to invite you.",
  );
  await expect(heading(page, "Sign in")).toBeVisible();

  idp.failNextAuthorizeWith = "access_denied";
  await page.getByRole("link", { name: `Sign in with ${LABEL}` }).click();
  await expect(page.getByRole("alert")).toHaveText("Your identity provider refused the sign-in.");

  // The password form still works beside it.
  await signIn(page, admin);
  await expect(heading(page, "Overview")).toBeVisible();
});

test("the callback of a signed-in browser arrives without its session cookie", async ({
  page,
  admin,
  apiAs,
  context,
  gateway,
  rules,
}) => {
  idp = await startMockIdp({ sub: "stranger", email: newAccount("nobody").email });
  rules.allowSite(idp.origin);
  const api = await apiAs(admin);
  await api.send("PUT", "/api/settings/oidc", settingsFor(idp));
  await signInFromStart(page, admin);
  const before = await context.cookies();
  expect(before.map((cookie) => cookie.name)).toContain("uf_session");

  // The callback requests the browser makes, with the headers it sent.
  const callback = `${gateway.origin}/api/auth/oidc/callback?code=x&state=y`;
  const sent: Promise<Record<string, string>>[] = [];
  page.on("request", (request) => {
    if (request.url() === callback) sent.push(request.allHeaders());
  });
  // A page of another site sends the browser to the callback: a cross-site
  // navigation, which the browser does without the Strict session cookie.
  await page.goto(`${idp.origin}/elsewhere`);
  await page.evaluate((address) => {
    const link = document.createElement("a");
    link.href = address;
    document.body.append(link);
    link.click();
  }, callback);
  await expect.poll(() => sent.length).toBe(1);
  const headers = (await sent[0]) ?? {};
  expect(headers["cookie"] ?? "", "no session cookie crosses sites").not.toContain("uf_session");
  // It was in the jar all along.
  expect((await context.cookies()).map((cookie) => cookie.name)).toContain("uf_session");
});

test("with accounts made on first sign-in, a new person signs in as a member, and a disabled one is refused", async ({
  page,
  admin,
  apiAs,
  rules,
  context,
}) => {
  const lena = newAccount("lena");
  idp = await startMockIdp({ sub: "lena-sub", email: lena.email, name: "Lena Fresh" });
  rules.allowSite(idp.origin);
  const api = await apiAs(admin);
  const domain = lena.email.split("@")[1] ?? "";
  await api.send("PUT", "/api/settings/oidc", settingsFor(idp, { auto_create: true, allowed_domains: [domain] }));

  await page.goto("/sign-in");
  await page.getByRole("link", { name: `Sign in with ${LABEL}` }).click();
  await expect(heading(page, "Overview")).toBeVisible();
  const who = await me(page);
  expect(who.user).toMatchObject({ email: lena.email, auth_provider: "oidc" });
  await signOut(page);

  // The admin disables her; the next SSO sign-in is refused.
  const users = (await api.get("/api/users")) as { users: { id: number; email: string }[] };
  const id = users.users.find((user) => user.email === lena.email)?.id;
  expect(id).toBeDefined();
  await api.send("PATCH", `/api/users/${String(id)}`, { status: "disabled" });
  await page.getByRole("link", { name: `Sign in with ${LABEL}` }).click();
  await expect(page.getByRole("alert")).toHaveText("Your account is disabled.");
  expect(await context.cookies()).toEqual([]);
});
