// What every browser test has: a gateway of its own, and a browser held to
// the rules of `rules.ts`. The API from the test (`api.ts`) and the steps the
// flows share (`steps.ts`) are exported from here as well, so that a spec
// imports from one place.
//
// Test accounts are made here for each test: random, and gone with the
// gateway of the test. They are typed into the page by `typeSecret`, whose
// step does not show the text, and the gateway logs warnings only; but what a
// failure writes (its message, the snapshot of the page in `test-results`)
// may show them. That output is not uploaded anywhere (playwright.config.ts).
import {
  expect,
  test as base,
  type APIRequestContext,
  type BrowserContext,
  type BrowserContextOptions,
} from "@playwright/test";
import { GatewayApi, newAccount } from "./api";
import { startGateway, type Account, type Gateway } from "./gateway";
import { BrowserRules } from "./rules";

export { expect };
export type { Account };
export { GatewayApi, newAccount, chat } from "./api";
export { BrowserRules } from "./rules";
export * from "./steps";


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
    if (rules.unmet().length > 0) {
      // The console line arrives a moment after the answer.
      await new Promise((done) => setTimeout(done, 300));
    }
    rules.problems.push(...rules.unmet());
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
