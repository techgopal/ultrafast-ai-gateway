// The rules every browser test is held to: the test fails on a console
// error, a Content Security Policy violation, a request to another origin, a
// request of the app outside `/api` or an answer 5xx. `rules.spec.ts` checks
// that they see what breaks them.
import type { BrowserContext, ConsoleMessage } from "@playwright/test";

/**
 * The refusals of the gateway that the console meets on purpose and handles:
 * 401 (nobody signed in: `/api/auth/me` before sign-in, a wrong password, a
 * session that ended), 403 (what the account may not do), 409 (a conflict a
 * dialog says, such as the last admin or a taken email) and 422 (fields that
 * are not valid). Not 404 or 405: from the API they say that the console asked
 * for a path or a method that is not there.
 */
const API_REFUSAL =
  /^Failed to load resource: the server responded with a status of (?:401|403|409|422)\b/;

/**
 * Collects what breaks the rules of the browser tests, from every context it
 * watches. A test fails when it collected anything.
 *
 * One kind of console error is not a problem: Chromium logs every answer
 * 4xx of a `fetch` as "Failed to load resource", also when the console asked
 * for it on purpose and handles it. It is let through for the refusals above,
 * when the answer is of the gateway's own `/api`; the flows check what the
 * console makes of it.
 */
export class BrowserRules {
  readonly problems: string[] = [];
  /** The refusals this test expects on purpose, each until it is seen. */
  private readonly expected: { status: number; path: RegExp; seen: boolean }[] = [];

  /** Origins a test lets the browser go to (never the app's own requests). */
  private readonly sites = new Set<string>();

  constructor(private readonly origin: string) {}

  /**
   * The test sends the browser to another site on purpose (an identity
   * provider that the test runs). The browser may go there by a navigation
   * of the page; a request of the app itself (`fetch`, `xhr`, ...) to it is
   * still a problem.
   */
  allowSite(origin: string): void {
    this.sites.add(origin);
  }

  /**
   * This test provokes an answer `status` of the gateway's `/api` at a path
   * that matches `path`, and the console handles it: the console error
   * Chromium logs for it is let through, for this test only and for no other
   * path or status. The test fails when it never happened (`unmet`).
   */
  expectRefusal(status: number, path: RegExp): void {
    this.expected.push({ status, path, seen: false });
  }

  /** What the test expected with `expectRefusal` and did not get. */
  unmet(): string[] {
    return this.expected
      .filter((one) => !one.seen)
      .map((one) => `expected refusal ${String(one.status)} at ${String(one.path)} never happened`);
  }

  private isApiRefusal(message: ConsoleMessage): boolean {
    let url: URL;
    try {
      url = new URL(message.location().url);
    } catch {
      return false;
    }
    if (url.origin !== this.origin || !url.pathname.startsWith("/api/")) return false;
    const text = message.text();
    const wanted = this.expected.find(
      (one) =>
        !one.seen &&
        text.startsWith(`Failed to load resource: the server responded with a status of ${String(one.status)}`) &&
        one.path.test(url.pathname),
    );
    if (wanted !== undefined) {
      wanted.seen = true;
      return true;
    }
    return API_REFUSAL.test(text);
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
      if (origin !== null && this.sites.has(origin) && route.request().isNavigationRequest()) {
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
