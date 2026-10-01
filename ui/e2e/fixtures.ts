// What every browser test has: a gateway of its own, a browser that fails
// the test on a console error, a Content Security Policy violation, a request
// to another origin, a request of the app outside `/api` or an answer 5xx,
// and the steps the flows share.
//
// Test accounts are made here for each test and are never printed: they are
// typed into the page by `typeSecret`, whose step in the report does not show
// the text, and the gateway logs warnings only.
import {
  expect,
  test as base,
  type APIRequestContext,
  type BrowserContext,
  type BrowserContextOptions,
  type ConsoleMessage,
  type Locator,
  type Page,
} from "@playwright/test";
import { randomBytes } from "node:crypto";
import { startGateway, type Account, type Gateway } from "./gateway";

export { expect };
export type { Account };

/** A new account: an email and a password of 24 characters, both random. */
export function newAccount(kind: string): Account {
  return {
    email: `${kind}-${randomBytes(6).toString("hex")}@example.test`,
    password: randomBytes(18).toString("base64url"),
  };
}

// ------------------------------------------------------------ the browser rules

const API_REFUSAL = /^Failed to load resource: the server responded with a status of 4\d\d\b/;

/**
 * Collects what breaks the rules of the browser tests, from every context it
 * watches. A test fails when it collected anything.
 *
 * One kind of console error is not a problem: Chromium logs every answer
 * 4xx of a `fetch` as "Failed to load resource", also when the console asked
 * for it on purpose and handles it (a 401 of `/api/auth/me` before sign-in, a
 * refusal that a dialog shows). It is let through when the answer is of the
 * gateway's own `/api`; the flows check what the console makes of it.
 */
export class BrowserRules {
  readonly problems: string[] = [];

  constructor(private readonly origin: string) {}

  private isApiRefusal(message: ConsoleMessage): boolean {
    if (!API_REFUSAL.test(message.text())) return false;
    try {
      const url = new URL(message.location().url);
      return url.origin === this.origin && url.pathname.startsWith("/api/");
    } catch {
      return false;
    }
  }

  async watch(context: BrowserContext): Promise<void> {
    // Every request goes through here; one to another origin is not sent.
    await context.route("**/*", async (route) => {
      const url = route.request().url();
      let origin: string | null = null;
      try {
        origin = new URL(url).origin;
      } catch {
        // Not a URL with an origin: not the gateway's.
      }
      if (origin === this.origin) {
        await route.fallback();
        return;
      }
      this.problems.push(`request to another origin: ${url}`);
      await route.abort("blockedbyclient");
    });
    await context.exposeBinding("__ufPolicyViolation", (_source, text: unknown) => {
      this.problems.push(`Content Security Policy violation: ${String(text)}`);
    });
    await context.addInitScript(() => {
      document.addEventListener("securitypolicyviolation", (event) => {
        const report = Reflect.get(window, "__ufPolicyViolation") as
          | ((text: string) => Promise<void>)
          | undefined;
        if (report === undefined) return;
        const where = `${event.sourceFile}:${String(event.lineNumber)}`;
        void report(`${event.effectiveDirective} blocked ${event.blockedURI || "inline"} at ${where}`);
      });
    });
    context.on("console", (message) => {
      if (message.type() !== "error" || this.isApiRefusal(message)) return;
      this.problems.push(`console error: ${message.text()}`);
    });
    context.on("weberror", (error) => {
      this.problems.push(`uncaught error: ${error.error().message}`);
    });
    // The app asks its own origin, and only paths under `/api`.
    context.on("request", (request) => {
      if (!["fetch", "xhr", "eventsource", "websocket"].includes(request.resourceType())) return;
      const url = new URL(request.url());
      if (url.origin === this.origin && !url.pathname.startsWith("/api/")) {
        this.problems.push(`request of the app outside /api: ${url.pathname}`);
      }
    });
    context.on("response", (response) => {
      if (response.status() >= 500) {
        this.problems.push(`answer ${String(response.status())}: ${response.url()}`);
      }
    });
  }
}

// ------------------------------------------------------------ the API, from the test

/** A signed-in session of the admin API, for making what a flow needs. */
export class GatewayApi {
  private constructor(
    private readonly request: APIRequestContext,
    private readonly csrf: string,
  ) {}

  static async signIn(request: APIRequestContext, account: Account): Promise<GatewayApi> {
    const answer = await request.post("/api/auth/login", { data: account });
    expect(answer.status(), "the test signs in through the API").toBe(200);
    const body = (await answer.json()) as { csrf_token: string };
    return new GatewayApi(request, body.csrf_token);
  }

  async send(method: "POST" | "PUT" | "PATCH" | "DELETE", path: string, data?: unknown) {
    const answer = await this.request.fetch(path, {
      method,
      headers: { "x-csrf-token": this.csrf },
      ...(data === undefined ? {} : { data }),
    });
    expect(answer.status(), `${method} ${path}`).toBeLessThan(300);
    return answer.status() === 204 ? null : ((await answer.json()) as unknown);
  }

  async get(path: string): Promise<unknown> {
    const answer = await this.request.get(path);
    expect(answer.status(), `GET ${path}`).toBe(200);
    return (await answer.json()) as unknown;
  }

  /** Invites a user and returns their id and the invite link (a path of the console). */
  async invite(name: string, email: string, role: "admin" | "member") {
    const made = (await this.send("POST", "/api/users", { name, email, role })) as {
      user: { id: number };
      invite_link: string;
    };
    return { id: made.user.id, link: made.invite_link };
  }

  /** A user who accepted their invite: they can sign in. */
  async activeUser(name: string, role: "admin" | "member" = "member") {
    const account = newAccount(name.toLowerCase().replace(/[^a-z0-9]+/g, "-"));
    const { id, link } = await this.invite(name, account.email, role);
    const token = new URL(link, "http://console").searchParams.get("token");
    const answer = await this.request.post("/api/auth/accept-invite", {
      data: { token, password: account.password },
    });
    expect(answer.status(), "the invite is accepted").toBe(204);
    return { id, name, ...account };
  }

  async createTeam(name: string): Promise<number> {
    const made = (await this.send("POST", "/api/teams", { name })) as { id: number };
    return made.id;
  }

  async putMember(teamId: number, userId: number, role: "lead" | "member") {
    await this.send("PUT", `/api/teams/${String(teamId)}/members/${String(userId)}`, { role });
  }
}

// ------------------------------------------------------------ fixtures

interface Fixtures {
  /** Whether the gateway starts with an admin. Default true; the setup flow sets false. */
  withAdmin: boolean;
  /** The first admin: the gateway's when `withAdmin`, else the one the test creates. */
  admin: Account;
  gateway: Gateway;
  rules: BrowserRules;
  /** Another browser context, as the test's own and watched by the same rules. */
  newContext: () => Promise<BrowserContext>;
  /** The API, signed in as the account given. */
  apiAs: (account: Account) => Promise<GatewayApi>;
}

// The second parameter of a fixture is Playwright's `use`, named `provide`
// here: the lint rules of React hooks take a function named `use` for a hook.
export const test = base.extend<Fixtures>({
  withAdmin: [true, { option: true }],

  // eslint-disable-next-line no-empty-pattern -- Playwright reads the dependencies from this pattern.
  admin: async ({}, provide) => {
    await provide(newAccount("admin"));
  },

  gateway: async ({ withAdmin, admin }, provide, testInfo) => {
    const gateway = await startGateway(withAdmin ? { admin } : {});
    try {
      await provide(gateway);
    } finally {
      await gateway.stop();
      if (testInfo.status !== testInfo.expectedStatus && gateway.output() !== "") {
        await testInfo.attach("gateway output", { body: gateway.output(), contentType: "text/plain" });
      }
    }
  },

  baseURL: async ({ gateway }, provide) => {
    await provide(gateway.origin);
  },

  rules: async ({ gateway }, provide) => {
    const rules = new BrowserRules(gateway.origin);
    await provide(rules);
    expect(rules.problems, "console errors, CSP violations, other origins, 5xx").toEqual([]);
  },

  context: async ({ context, rules }, provide) => {
    await rules.watch(context);
    await provide(context);
  },

  newContext: async (
    { browser, rules, gateway, viewport, hasTouch, isMobile, colorScheme, permissions },
    provide,
  ) => {
    const made: BrowserContext[] = [];
    await provide(async () => {
      const options: BrowserContextOptions = {
        baseURL: gateway.origin,
        viewport,
        colorScheme,
        hasTouch,
        isMobile,
      };
      if (permissions !== undefined) options.permissions = permissions;
      const context = await browser.newContext(options);
      made.push(context);
      await rules.watch(context);
      return context;
    });
    for (const context of made) await context.close();
  },

  apiAs: async ({ playwright, gateway }, provide) => {
    const made: APIRequestContext[] = [];
    await provide(async (account) => {
      const request = await playwright.request.newContext({ baseURL: gateway.origin });
      made.push(request);
      return GatewayApi.signIn(request, account);
    });
    for (const request of made) await request.dispose();
  },
});

// ------------------------------------------------------------ steps

/**
 * Types a secret into a field. It sets the value as typing does and tells the
 * page by an `input` event; unlike `fill`, its step in the report does not
 * show the text.
 */
export async function typeSecret(field: Locator, text: string): Promise<void> {
  await expect(field).toBeEditable();
  await field.evaluate((element, value) => {
    if (!(element instanceof HTMLInputElement)) throw new Error("The field is not an input.");
    element.focus();
    // The setter of the prototype: React watches the one of the element, and
    // takes a value set through it for no change.
    // eslint-disable-next-line @typescript-eslint/unbound-method -- it is called on the input below.
    const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    if (setValue === undefined) throw new Error("An input has no value setter.");
    setValue.call(element, value);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  }, text);
}

/** Fills the sign-in form and sends it. */
export async function signIn(page: Page, account: Account): Promise<void> {
  const form = page.getByRole("form", { name: "Sign in" });
  await typeSecret(form.getByLabel("Email", { exact: true }), account.email);
  await typeSecret(form.getByLabel("Password", { exact: true }), account.password);
  await form.getByRole("button", { name: "Sign in" }).click();
}

/** The main heading of the page. */
export function heading(page: Page, name: string): Locator {
  return page.getByRole("heading", { level: 1, name, exact: true });
}

/** Opens the sign-in page and signs in; ends on the Overview. */
export async function signInFromStart(page: Page, account: Account): Promise<void> {
  await page.goto("/sign-in");
  await signIn(page, account);
  await expect(heading(page, "Overview")).toBeVisible();
}

/** The menu button of a narrow screen, which opens the drawer. */
export function menuButton(page: Page): Locator {
  return page.getByRole("button", { name: "Open menu" });
}

/**
 * Whether the page is below the width at which the console's sidebar is a
 * drawer (768 px). Decided by the viewport and not by what shows: the
 * console's first frame after a load is the wide one.
 */
export function isNarrow(page: Page): boolean {
  return (page.viewportSize()?.width ?? 1280) < 768;
}

/** Opens the drawer on a narrow screen; on a wide one the navigation is always there. */
export async function openNavigation(page: Page): Promise<Locator> {
  const nav = page.getByRole("navigation", { name: "Main" });
  if (isNarrow(page)) await menuButton(page).click();
  await expect(nav).toBeVisible();
  return nav;
}

/** Closes the drawer of a narrow screen. */
export async function closeNavigation(page: Page): Promise<void> {
  if (!isNarrow(page)) return;
  const drawer = page.getByRole("dialog", { name: "Sidebar" });
  await expect(drawer).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
}

/** Goes to a page through the navigation, as a user does. */
export async function goTo(page: Page, label: string, title: string = label): Promise<void> {
  const nav = await openNavigation(page);
  await nav.getByRole("link", { name: label, exact: true }).click();
  await expect(heading(page, title)).toBeVisible();
}

/** Signs out through the button of the navigation. */
export async function signOut(page: Page): Promise<void> {
  const nav = await openNavigation(page);
  await nav.getByRole("button", { name: "Sign out" }).click();
  await expect(heading(page, "Sign in")).toBeVisible();
}

/** The row of a table, or the card that stands for it on a narrow screen. */
export function itemOf(page: Page, list: string, text: string): Locator {
  return page
    .getByRole("table", { name: list })
    .getByRole("row")
    .filter({ hasText: text })
    .or(page.getByRole("list", { name: list }).getByRole("listitem").filter({ hasText: text }));
}

/** Whether the value has the form, said without showing the value. */
export function expectForm(value: string, form: RegExp, what: string): void {
  expect(form.test(value), `${what} has the form ${String(form)}`).toBe(true);
}

/**
 * Whether the text is anywhere the page keeps something: the document and
 * the values of its fields, the address, the storage of the browser and the
 * cookies a script can read. Said without showing the text.
 */
export async function expectNowhere(page: Page, text: string, what: string): Promise<void> {
  const found = await page.evaluate((secret) => {
    const fields = [...document.querySelectorAll("input, textarea")].map((field) =>
      field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement ? field.value : "",
    );
    const stored = (storage: Storage) =>
      Object.keys(storage).flatMap((key) => [key, storage.getItem(key) ?? ""]);
    const places: Record<string, string[]> = {
      document: [document.documentElement.outerHTML],
      fields,
      address: [window.location.href, document.referrer],
      history: [JSON.stringify(window.history.state)],
      localStorage: stored(window.localStorage),
      sessionStorage: stored(window.sessionStorage),
      cookies: [document.cookie],
    };
    return Object.entries(places)
      .filter(([, values]) => values.some((value) => value.includes(secret)))
      .map(([place]) => place);
  }, text);
  expect(found, `${what} is kept nowhere`).toEqual([]);
}

// The forms of the secrets and texts of the gateway, as `src/test/fixtures.test.ts` pins them.
export const KEY_SECRET = /^uf-sk-[0-9a-f]{64}$/;
export const KEY_DISPLAY = /^uf-sk-…[0-9a-f]{4}$/;
export const TOKEN_SECRET = /^uf-at-[0-9a-f]{64}$/;
export const TOKEN_DISPLAY = /^uf-at-…[0-9a-f]{4}$/;
export const INVITE_PATH = /^\/accept-invite\?token=uf-inv-[0-9a-f]{64}$/;
