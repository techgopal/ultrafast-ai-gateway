import type { QueryClient } from "@tanstack/react-query";
import { act, renderHook, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryHistory } from "@tanstack/react-router";
import { HttpResponse } from "msw";
import { useState } from "react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { api, onUnauthenticated } from "@/api/client";
import { keysOptions, meOptions, useCreateTeam, useKeys, useTeams } from "@/api/queries";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { PageProblem } from "@/pages/NotAvailable";
import * as fixtures from "@/test/fixtures";
import { gate, PASSWORD, startGateway } from "@/test/gateway";
import { errors } from "@/test/errors";
import { networkFailure, noContent, ok, override, refuse } from "@/test/handlers";
import {
  counted,
  expectOneH1,
  expectOneMain,
  forgetToasts,
  shown as shownWithValues,
  theBrowserIsOffline,
  theWindowGetsTheFocus,
  toasts,
} from "@/test/pages";
import { renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";
import { useSession, useSessionControl, useSignOut } from "./session";

const SESSION_ENDED = "Your session ended. Sign in again.";

function href(app: AppRenderResult): string {
  return app.router.state.location.href;
}

async function signIn(password = PASSWORD, email = "maya@example.test"): Promise<void> {
  const user = userEvent.setup();
  const emailField = screen.getByLabelText("Email");
  await user.clear(emailField);
  await user.type(emailField, email);
  await user.type(screen.getByLabelText("Password"), password);
  await user.click(screen.getByRole("button", { name: "Sign in" }));
}

/** The token a write carries now. */
async function tokenOfAWrite(): Promise<string | null> {
  let sent: string | null = "no request";
  override("post", "/api/teams", ({ request }) => {
    sent = request.headers.get("x-csrf-token");
    return ok("post", "/api/teams", 201, fixtures.teams.growth);
  });
  await api.post("/api/teams", { body: { name: "Growth" } });
  return sent;
}

/** Everything the two caches hold, as one text. */
function cached(client: QueryClient): string {
  const queries = client
    .getQueryCache()
    .getAll()
    .map((query) => ({ key: query.queryKey, state: query.state }));
  const mutations = client
    .getMutationCache()
    .getAll()
    .map((mutation) => ({ key: mutation.options.mutationKey, state: mutation.state }));
  return JSON.stringify({ queries, mutations });
}

/** The page as text, with what the fields hold. */
function shown(): string {
  const values = [...document.querySelectorAll("input, textarea")].map((field) =>
    field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement ? field.value : "",
  );
  return document.documentElement.outerHTML + JSON.stringify(values);
}

/** A call that the gateway answers with 401, as it does when the session ended. */
async function aCallFindsTheSessionEnded(): Promise<void> {
  override("get", "/api/teams", unauthenticated);
  await act(async () => {
    await api.get("/api/teams").catch(() => undefined);
  });
}

function heading(name: string): HTMLElement {
  return screen.getByRole("heading", { name });
}

describe("guards of the routes", () => {
  test("setup needed redirects everything", async () => {
    for (const route of ["/", "/keys", "/sign-in", "/accept-invite", "/nothing-here"]) {
      startGateway({ needsSetup: true });
      const app = await renderWithApp(null, { route });
      await waitFor(() => {
        expect(heading("Set up the gateway")).toBeInTheDocument();
      });
      expect(href(app)).toBe("/setup");
      app.unmount();
    }
  });

  test("setup needed at / never goes by the sign-in page", async () => {
    startGateway({ needsSetup: true });
    const history = createMemoryHistory({ initialEntries: ["/"] });
    const replace = vi.spyOn(history, "replace");
    const app = await renderWithApp(null, { history, user: null });
    await waitFor(() => {
      expect(heading("Set up the gateway")).toBeInTheDocument();
    });
    // Lets any effect that would still send the visitor on run out.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
    const paths = replace.mock.calls.map(([path]) => path);
    expect(paths.filter((path) => path.startsWith("/sign-in"))).toEqual([]);
    expect(paths).toEqual(["/setup"]);
    expect(href(app)).toBe("/setup");
    expect(heading("Set up the gateway")).toBeInTheDocument();
  });

  test("signed out visitor goes to sign-in with next", async () => {
    startGateway();
    const app = await renderWithApp(null, { route: "/keys" });
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fkeys");
    });
    expect(heading("Sign in")).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Main" })).toBeNull();
  });

  test("a signed out visitor who opens /setup on a gateway that is set up goes to sign-in", async () => {
    startGateway();
    let sent = 0;
    override("post", "/api/setup", () => {
      sent += 1;
      return refuse(errors.already_set_up);
    });
    const app = await renderWithApp(null, { route: "/setup" });
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(heading("Sign in")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Set up the gateway" })).toBeNull();
    expect(sent).toBe(0);
  });

  test("a signed out visitor of the overview goes to sign-in without next", async () => {
    startGateway();
    const app = await renderWithApp(null, { route: "/" });
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
  });

  test.each([
    ["two slashes", "//evil.example"],
    ["an absolute address", "https://evil.example"],
    ["a slash and a backslash", "/\\evil"],
    ["a script", "javascript:alert(1)"],
    ["encoded slashes after the slash", "/%2F%2Fevil.example"],
    ["an encoded backslash", "/%5Cevil.example"],
    ["encoded slashes", "%2F%2Fevil.example"],
    ["twice encoded slashes", "%2F%252F%252Fevil.example"],
    ["more than 2000 characters", `%2F${"a".repeat(2001)}`],
    ["a newline", "%2Fkeys%0A"],
    ["a tab", "%2F%09%2Fevil.example"],
    ["a number", "7"],
  ])("unsafe next is ignored: %s", async (_, next) => {
    startGateway();
    const app = await renderWithApp(null, { route: `/sign-in?next=${next}` });
    await signIn();
    await waitFor(() => {
      expect(heading("Overview")).toBeInTheDocument();
    });
    expect(href(app)).toBe("/");
  });

  test("a safe next keeps its query string and hash", async () => {
    startGateway();
    const app = await renderWithApp(null, {
      route: "/sign-in?next=%2Fkeys%3Fstatus%3Dactive%23x",
    });
    await signIn();
    await waitFor(() => {
      expect(heading("Virtual keys")).toBeInTheDocument();
    });
    expect(href(app)).toBe("/keys?status=active#x");
  });

  test.each(["/sign-in", "/setup", "/sign-in?next=%2F%2Fevil.example"])(
    "a signed-in user who opens %s is sent to the overview",
    async (route) => {
      startGateway({ signedIn: true });
      const app = await renderWithApp(null, { route });
      await waitFor(() => {
        expect(href(app)).toBe("/");
      });
      expect(heading("Overview")).toBeInTheDocument();
    },
  );

  test("audit is not available to members", async () => {
    let calls = 0;
    override("get", "/api/audit", () => {
      calls += 1;
      return ok("get", "/api/audit", 200, { entries: fixtures.auditEntries });
    });
    await renderWithApp(null, { route: "/audit", user: fixtures.me.lena });
    expect(screen.getByText("This page is not available to your account.")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Audit log" })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    // The shell is still there.
    expect(screen.getByRole("navigation", { name: "Main" })).toBeInTheDocument();
    expect(calls).toBe(0);
  });

  test("audit is available to admins", async () => {
    await renderWithApp(null, { route: "/audit", user: fixtures.me.maya });
    expect(heading("Audit log")).toBeInTheDocument();
  });

  test("403 from a page renders not available", async () => {
    override("get", "/api/keys", () => refuse(errors.forbidden));
    function Page() {
      const keys = useKeys();
      if (keys.isError) return <PageProblem error={keys.error} />;
      return <p>{keys.isPending ? "Loading keys" : "Keys"}</p>;
    }
    await renderWithApp(<Page />);
    expect(
      await screen.findByText("This page is not available to your account."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(errors.forbidden.body.error.message)).toBeNull();
  });

  test("another failure of a page is shown as an error", async () => {
    override("get", "/api/keys", () => refuse(errors.internal_error));
    function Page() {
      const keys = useKeys();
      return keys.isError ? <PageProblem error={keys.error} /> : <p>Keys</p>;
    }
    await renderWithApp(<Page />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Something went wrong.");
  });

  test("the app says so when the gateway cannot be reached, and tries again", async () => {
    const gateway = startGateway({ signedIn: true });
    override("get", "/api/setup", networkFailure);
    const app = await renderWithApp(null, { route: "/keys" });
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    override("get", "/api/setup", () =>
      ok("get", "/api/setup", 200, { needs_setup: gateway.needsSetup }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(heading("Virtual keys")).toBeInTheDocument();
    });
    expect(href(app)).toBe("/keys");
  });
});

describe("the screens of a failure", () => {
  const NOT_LOADED = "This page could not be loaded";
  const COULD_NOT_LOAD = "The console could not load";

  test("a page that fails as it is shown: the error screen in the shell, with one h1 and not what was thrown", async () => {
    // React and the router report the error of the render to the console.
    const quiet = [
      vi.spyOn(console, "error").mockImplementation(() => undefined),
      vi.spyOn(console, "warn").mockImplementation(() => undefined),
    ];
    try {
      startGateway({ signedIn: true });
      // An answer the page cannot show: it fails while it renders.
      override("get", "/api/teams/{id}", () => HttpResponse.json({ team: null, members: [] }));
      await renderWithApp(null, { route: `/teams/${String(fixtures.teams.platform.id)}` });
      expect(
        await screen.findByRole("heading", { level: 1, name: NOT_LOADED }),
      ).toBeInTheDocument();
      expectOneH1(NOT_LOADED);
      expectOneMain();
      expect(screen.getByRole("alert")).toHaveTextContent("Something went wrong.");
      expect(shownWithValues()).not.toMatch(/Cannot read|null \(reading|is not a function/);
      expect(screen.getByRole("navigation", { name: "Main" })).toBeInTheDocument();
    } finally {
      for (const spy of quiet) spy.mockRestore();
    }
  });

  test("the error screen outside the shell brings its own main", async () => {
    await renderWithApp(<PageProblem error={new TypeError("a detail of the code")} />);
    expectOneMain();
    expectOneH1(NOT_LOADED);
    expect(screen.getByRole("alert")).toHaveTextContent("Something went wrong.");
    expect(shownWithValues()).not.toContain("a detail of the code");
  });

  test("the start screen when the gateway cannot be asked: one h1, the failure, and Try again", async () => {
    startGateway({ signedIn: true });
    override("get", "/api/setup", networkFailure);
    await renderWithApp(null, { route: "/keys" });
    expect(
      await screen.findByRole("heading", { level: 1, name: COULD_NOT_LOAD }),
    ).toBeInTheDocument();
    expectOneH1(COULD_NOT_LOAD);
    expectOneMain();
    expect(screen.getByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  test("the start screen does not show what was thrown", async () => {
    startGateway({ signedIn: true });
    const original = api.get;
    const fails: typeof api.get = (path, ...rest) =>
      path === "/api/setup"
        ? Promise.reject(new TypeError("a detail of the code"))
        : original(path, ...rest);
    const spy = vi.spyOn(api, "get").mockImplementation(fails);
    try {
      await renderWithApp(null, { route: "/keys" });
      expect(
        await screen.findByRole("heading", { level: 1, name: COULD_NOT_LOAD }),
      ).toBeInTheDocument();
      expect(screen.getByRole("alert")).toHaveTextContent("Something went wrong.");
      expect(shownWithValues()).not.toContain("a detail of the code");
      expectOneH1(COULD_NOT_LOAD);
    } finally {
      spy.mockRestore();
    }
  });
});

// The gateway can be on the same machine: the browser's word that there is no
// network does not keep the app from asking it.
describe("while the browser says it is offline", () => {
  test("the app opened then asks who is signed in: the gateway answers, and the page shows", async () => {
    startGateway({ signedIn: true });
    theBrowserIsOffline();
    await renderWithApp(null, { route: "/keys" });
    expect(await screen.findByRole("heading", { name: "Virtual keys", level: 1 })).toBeInTheDocument();
  });

  test("a gateway that cannot be reached is said, with Try again, and not by an endless Loading", async () => {
    theBrowserIsOffline();
    override("get", "/api/setup", networkFailure);
    override("get", "/api/auth/me", networkFailure);
    await renderWithApp(null, { route: "/keys" });
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
    expect(screen.queryByRole("status", { name: "Loading" })).toBeNull();
  });
});

describe("sign-in", () => {
  test("sign-in success", async () => {
    const gateway = startGateway({ me: fixtures.me.arjun });
    const app = await renderWithApp(null, { route: "/keys" });
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fkeys");
    });
    expect(await tokenOfAWrite()).toBeNull();
    await signIn(PASSWORD, "arjun@example.test");
    await waitFor(() => {
      expect(heading("Virtual keys")).toBeInTheDocument();
    });
    expect(href(app)).toBe("/keys");
    expect(gateway.logins).toEqual([{ email: "arjun@example.test", password: PASSWORD }]);
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(within(nav).getByText("Arjun Mehta")).toBeInTheDocument();
    expect(within(nav).getByText("member")).toBeInTheDocument();
    expect(within(nav).queryByRole("link", { name: "Audit log" })).toBeNull();
    expect(await tokenOfAWrite()).toBe(fixtures.csrfToken);
  });

  const failures: [string, () => Response, string][] = [
    [
      "401",
      () => refuse(errors.invalid_credentials),
      "Email or password is incorrect.",
    ],
    [
      "429",
      () => refuse(errors.too_many_attempts),
      "Too many attempts. Try again in a few minutes.",
    ],
    ["a network error", networkFailure, "Could not reach the gateway."],
  ];

  test.each(failures)("sign-in failures: %s", async (_, answer, message) => {
    startGateway();
    override("post", "/api/auth/login", answer);
    const ended = vi.fn();
    const stop = onUnauthenticated(ended);
    const app = await renderWithApp(null, { route: "/sign-in?next=%2Fkeys" });
    await signIn("a-wrong-password-0000");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(message);
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByLabelText("Password")).toHaveValue("");
    expect(screen.getByLabelText("Email")).toHaveValue("maya@example.test");
    expect(screen.getByRole("button", { name: "Sign in" })).toBeEnabled();
    // No field has an error, so the focus is on the message.
    await waitFor(() => {
      expect(alert).toHaveFocus();
    });
    expect(href(app)).toBe("/sign-in?next=%2Fkeys");
    expect(ended).not.toHaveBeenCalled();
    stop();
  });

  const NOT_KEPT =
    "Signed in, but this browser did not keep the session. Open the console over HTTPS, or start the gateway with --insecure-cookies on a trusted network.";

  /**
   * The gateway takes the password, but the browser does not keep the session
   * cookie (a `Secure` cookie over plain HTTP): `me` still answers 401.
   */
  function theBrowserKeepsNoCookie(): void {
    override("post", "/api/auth/login", () =>
      ok("post", "/api/auth/login", 200, {
        user: fixtures.users.maya,
        csrf_token: fixtures.csrfToken,
      }),
    );
    override("get", "/api/auth/me", unauthenticated);
  }

  test("a sign-in whose session the browser did not keep says so, and the form is as after a refusal", async () => {
    startGateway();
    theBrowserKeepsNoCookie();
    const ended = vi.fn();
    const stop = onUnauthenticated(ended);
    const app = await renderWithApp(null, { route: "/sign-in?next=%2Fkeys" });
    await signIn();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(NOT_KEPT);
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByLabelText("Password")).toHaveValue("");
    expect(screen.getByLabelText("Email")).toHaveValue("maya@example.test");
    expect(screen.getByRole("button", { name: "Sign in" })).toBeEnabled();
    await waitFor(() => {
      expect(alert).toHaveFocus();
    });
    expect(href(app)).toBe("/sign-in?next=%2Fkeys");
    expect(heading("Sign in")).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Main" })).toBeNull();
    expect(ended).not.toHaveBeenCalled();
    stop();
  });

  test("after a sign-out, a sign-in whose session the browser did not keep says so as well", async () => {
    startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    theBrowserKeepsNoCookie();
    await signIn();
    expect(await screen.findByRole("alert")).toHaveTextContent(NOT_KEPT);
    expect(screen.getByLabelText("Email")).toHaveValue("maya@example.test");
    expect(href(app)).toBe("/sign-in");
  });

  test("the message of a session that was not kept goes with the next attempt, which goes on when it is kept", async () => {
    startGateway();
    theBrowserKeepsNoCookie();
    const app = await renderWithApp(null, { route: "/sign-in?next=%2Fkeys" });
    await signIn();
    expect(await screen.findByRole("alert")).toHaveTextContent(NOT_KEPT);
    // The console is opened over HTTPS, say: the browser keeps the cookie now.
    startGateway();
    await signIn();
    await waitFor(() => {
      expect(heading("Virtual keys")).toBeInTheDocument();
    });
    expect(href(app)).toBe("/keys");
    expect(screen.queryByText(NOT_KEPT)).toBeNull();
    expect(await tokenOfAWrite()).toBe(fixtures.csrfToken);
  });

  test("the message of a failure goes when the next attempt starts", async () => {
    startGateway();
    await renderWithApp(null, { route: "/sign-in" });
    await signIn("a-wrong-password-0000");
    expect(await screen.findByRole("alert")).toHaveTextContent("Email or password is incorrect.");
    await signIn();
    await waitFor(() => {
      expect(heading("Overview")).toBeInTheDocument();
    });
    expect(screen.queryByText("Email or password is incorrect.")).toBeNull();
  });

  test("submit is disabled while signing in", async () => {
    const gateway = startGateway();
    const answer = gate();
    let calls = 0;
    override("post", "/api/auth/login", async () => {
      calls += 1;
      await answer.opened;
      gateway.signedIn = true;
      return ok("post", "/api/auth/login", 200, {
        user: fixtures.users.maya,
        csrf_token: fixtures.csrfToken,
      });
    });
    await renderWithApp(null, { route: "/sign-in" });
    const button = screen.getByRole("button", { name: "Sign in" });
    expect(button).toBeEnabled();
    await signIn();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Sign(ing)? in/ })).toBeDisabled();
    });
    // A second submit while the first runs sends nothing.
    await userEvent.type(screen.getByLabelText("Email"), "{Enter}");
    expect(calls).toBe(1);
    answer.open();
    await waitFor(() => {
      expect(heading("Overview")).toBeInTheDocument();
    });
    expect(calls).toBe(1);
  });

  test("the fields say what they are to the browser", async () => {
    startGateway();
    await renderWithApp(null, { route: "/sign-in" });
    expect(screen.getByLabelText("Email")).toHaveAttribute("autocomplete", "username");
    expect(screen.getByLabelText("Email")).toHaveAttribute("type", "email");
    expect(screen.getByLabelText("Password")).toHaveAttribute("autocomplete", "current-password");
    expect(screen.getByLabelText("Password")).toHaveAttribute("type", "password");
  });

  test("the sign-in form is one column at width 390 and announces its error", async () => {
    startGateway();
    await renderWithApp(null, { route: "/sign-in", width: 390 });
    const form = screen.getByRole("form", { name: "Sign in" });
    const classes = form.className.split(/\s+/);
    expect(classes).toContain("flex");
    expect(classes).toContain("flex-col");
    expect(form.className).not.toMatch(/flex-row|grid-cols/);
    const controls = [...form.querySelectorAll("input, button")];
    expect(controls.map((c) => c.getAttribute("name") ?? c.textContent)).toEqual([
      "email",
      "password",
      "Sign in",
    ]);
    for (const control of controls) expect(control.className).toMatch(/\bw-full\b/);
    await signIn("a-wrong-password-0000");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Email or password is incorrect.");
  });

  test("the password is nowhere after a failed and after a successful sign-in", async () => {
    startGateway();
    const wrong = "a-wrong-password-0000";
    const app = await renderWithApp(null, { route: "/sign-in" });
    await signIn(wrong);
    await screen.findByRole("alert");
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    expect(cached(app.queryClient)).not.toContain(wrong);
    expect(shown()).not.toContain(wrong);

    await signIn();
    await waitFor(() => {
      expect(heading("Overview")).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    for (const password of [wrong, PASSWORD]) {
      expect(cached(app.queryClient)).not.toContain(password);
      expect(shown()).not.toContain(password);
      expect(JSON.stringify(app.router.state)).not.toContain(password);
    }
    // The scan sees what is there.
    expect(cached(app.queryClient)).toContain("maya@example.test");
  });
});

describe("the session", () => {
  test("reload keeps the session", async () => {
    const gateway = startGateway({ signedIn: true });
    gateway.csrfToken = "the-token-me-gave";
    const visited: string[] = [];
    const history = createMemoryHistory({ initialEntries: ["/keys"] });
    history.subscribe(({ location }) => {
      visited.push(location.href);
    });
    const app = await renderWithApp(null, { history });
    expect(visited.filter((address) => address.startsWith("/sign-in"))).toEqual([]);
    expect(heading("Virtual keys")).toBeInTheDocument();
    expect(href(app)).toBe("/keys");
    expect(gateway.logins).toEqual([]);
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(within(nav).getByText("Maya Okafor")).toBeInTheDocument();
    expect(await tokenOfAWrite()).toBe("the-token-me-gave");
  });

  test("useSession says who is signed in", async () => {
    function Who() {
      const session = useSession();
      return <p>{session.status === "signedIn" ? session.me.user.email : session.status}</p>;
    }
    const first = await renderWithApp(<Who />, { user: fixtures.me.lena });
    expect(await screen.findByText("lena@example.test")).toBeInTheDocument();
    first.unmount();
    await renderWithApp(<Who />, { user: null });
    expect(await screen.findByText("signedOut")).toBeInTheDocument();
  });

  test("session ending mid-use", async () => {
    startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    // The keys page comes with a later task; its data is loaded here.
    await app.queryClient.query(keysOptions());
    expect(app.queryClient.getQueryData(keysOptions().queryKey)).toBeDefined();
    expect(await tokenOfAWrite()).toBe(fixtures.csrfToken);

    await aCallFindsTheSessionEnded();

    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fkeys");
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(heading("Sign in")).toBeInTheDocument();
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    expect(await tokenOfAWrite()).toBeNull();
  });

  test("the session ends twice, and each end is told once", async () => {
    const gateway = startGateway({ signedIn: true });
    const told = vi.fn();
    const stop = onUnauthenticated(told);
    const app = await renderWithApp(null, { route: "/keys" });

    gateway.signedIn = false;
    override("get", "/api/users", unauthenticated);
    await act(async () => {
      await Promise.all([
        api.get("/api/users").catch(() => undefined),
        api.get("/api/users").catch(() => undefined),
      ]);
    });
    expect(told).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fkeys");
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);

    gateway.csrfToken = "the-token-of-the-second-session";
    await signIn();
    await waitFor(() => {
      expect(heading("Virtual keys")).toBeInTheDocument();
    });
    expect(screen.queryByText(SESSION_ENDED)).toBeNull();
    expect(await tokenOfAWrite()).toBe("the-token-of-the-second-session");
    expect(told).toHaveBeenCalledTimes(1);

    gateway.signedIn = false;
    await act(async () => {
      await api.get("/api/users").catch(() => undefined);
    });
    expect(told).toHaveBeenCalledTimes(2);
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fkeys");
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(await tokenOfAWrite()).toBeNull();
    stop();
  });

  test("session ending closes a secret dialog", async () => {
    // The dialog for secrets comes with a later task; this one stands in for it.
    function SecretDialog() {
      const [secret, setSecret] = useState<string | null>(null);
      return (
        <main>
          <Button
            type="button"
            onClick={() => {
              setSecret(fixtures.newKeySecret);
            }}
          >
            Create key
          </Button>
          <Dialog
            open={secret !== null}
            onOpenChange={(open) => {
              if (!open) setSecret(null);
            }}
          >
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Your new key</DialogTitle>
                <DialogDescription>It is shown once.</DialogDescription>
              </DialogHeader>
              <code>{secret}</code>
            </DialogContent>
          </Dialog>
        </main>
      );
    }
    startGateway({ signedIn: true });
    await renderWithApp(<SecretDialog />);
    await userEvent.click(screen.getByRole("button", { name: "Create key" }));
    expect(screen.getByRole("dialog", { name: "Your new key" })).toBeInTheDocument();
    expect(document.body.innerHTML).toContain(fixtures.newKeySecret);

    await aCallFindsTheSessionEnded();

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(shown()).not.toContain(fixtures.newKeySecret);
  });

  test("an answer of a session that has ended changes nothing", async () => {
    const gateway = startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    expect(heading("Virtual keys")).toBeInTheDocument();
    const told = vi.fn();
    const stop = onUnauthenticated(told);

    // A request for `me` of this session is on its way.
    const answer = gate();
    let asked = 0;
    let answered = 0;
    override("get", "/api/auth/me", async () => {
      asked += 1;
      await answer.opened;
      answered += 1;
      return ok("get", "/api/auth/me", 200, {
        ...fixtures.me.maya,
        csrf_token: "the-token-of-the-session-that-ended",
      });
    });
    void app.queryClient.invalidateQueries({ queryKey: meOptions().queryKey });
    await waitFor(() => {
      expect(asked).toBe(1);
    });

    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(gateway.logouts).toBe(1);

    answer.open();
    await waitFor(() => {
      expect(answered).toBe(1);
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    expect(heading("Sign in")).toBeInTheDocument();
    expect(href(app)).toBe("/sign-in");
    expect(screen.queryByRole("navigation", { name: "Main" })).toBeNull();
    expect(await tokenOfAWrite()).toBeNull();
    expect(cached(app.queryClient)).not.toContain("the-token-of-the-session-that-ended");
    expect(told).not.toHaveBeenCalled();

    // The next session still hears of its end, once.
    startGateway();
    await signIn();
    await waitFor(() => {
      expect(heading("Overview")).toBeInTheDocument();
    });
    override("get", "/api/users", unauthenticated);
    await act(async () => {
      await api.get("/api/users").catch(() => undefined);
      await api.get("/api/users").catch(() => undefined);
    });
    expect(told).toHaveBeenCalledTimes(1);
    stop();
  });

  test("`me` answering 401 later ends the session", async () => {
    const gateway = startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    gateway.signedIn = false;
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: meOptions().queryKey });
    });
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fkeys");
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(await tokenOfAWrite()).toBeNull();
  });
});

describe("answers of a session that is over", () => {
  /** A write that is on its way and answers when the gate opens. */
  function aWriteOnItsWay(answer: () => Response) {
    const door = gate();
    let asked = 0;
    override("post", "/api/keys", async () => {
      asked += 1;
      await door.opened;
      return answer();
    });
    const settled = api
      .post("/api/keys", { body: { name: "a key" } })
      .then(
        () => "resolved",
        (error: unknown) => (error instanceof Error ? error.name : "rejected"),
      );
    return {
      settled,
      answer: door.open,
      started: () =>
        waitFor(() => {
          expect(asked).toBe(1);
        }),
    };
  }

  test("a 401 for the user before does not end the session of the next user", async () => {
    startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    const told = vi.fn();
    const stop = onUnauthenticated(told);
    const write = aWriteOnItsWay(unauthenticated);
    await write.started();

    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    const next = startGateway({ me: fixtures.me.lena });
    next.csrfToken = "the-token-of-the-next-user";
    await signIn(PASSWORD, "lena@example.test");
    await waitFor(() => {
      expect(heading("Overview")).toBeInTheDocument();
    });

    await act(async () => {
      write.answer();
      expect(await write.settled).toBe("SessionOverError");
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    expect(told).not.toHaveBeenCalled();
    expect(heading("Overview")).toBeInTheDocument();
    expect(href(app)).toBe("/");
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(within(nav).getByText("Lena Fischer")).toBeInTheDocument();
    expect(screen.queryByText(SESSION_ENDED)).toBeNull();
    expect(await tokenOfAWrite()).toBe("the-token-of-the-next-user");

    // The session of the next user still hears of its own end.
    await aCallFindsTheSessionEnded();
    expect(told).toHaveBeenCalledTimes(1);
    stop();
  });

  test("a success for the user before refetches nothing of the next user", async () => {
    const door = gate();
    let teamCalls = 0;
    let created = 0;
    override("get", "/api/teams", () => {
      teamCalls += 1;
      return ok("get", "/api/teams", 200, { teams: fixtures.teamList });
    });
    override("post", "/api/teams", async () => {
      created += 1;
      await door.opened;
      return ok("post", "/api/teams", 201, fixtures.teams.growth);
    });
    const outcome: string[] = [];

    function Teams() {
      const teams = useTeams();
      const create = useCreateTeam();
      return (
        <div>
          <p>{teams.isSuccess ? "teams shown" : "teams loading"}</p>
          <Button
            type="button"
            onClick={() => {
              create.mutate(
                { name: "Growth" },
                {
                  onSuccess: () => outcome.push("success"),
                  onError: () => outcome.push("error"),
                },
              );
            }}
          >
            Create team
          </Button>
        </div>
      );
    }
    function Page() {
      const session = useSession();
      const signOut = useSignOut();
      const { begin } = useSessionControl();
      if (session.status === "loading") return <p>loading</p>;
      if (session.status === "signedOut") {
        return (
          <Button
            type="button"
            onClick={() => {
              void api
                .post("/api/auth/login", { body: { email: "lena@example.test", password: PASSWORD } })
                .then((answer) => {
                  void begin(answer.csrf_token);
                });
            }}
          >
            Enter
          </Button>
        );
      }
      return (
        <main>
          <p>{session.me.user.name}</p>
          <Teams />
          <Button type="button" onClick={() => void signOut()}>
            Leave
          </Button>
        </main>
      );
    }

    startGateway({ signedIn: true });
    const app = await renderWithApp(<Page />);
    await screen.findByText("teams shown");
    expect(teamCalls).toBe(1);
    await userEvent.click(screen.getByRole("button", { name: "Create team" }));
    await waitFor(() => {
      expect(created).toBe(1);
    });
    await userEvent.click(screen.getByRole("button", { name: "Leave" }));
    startGateway({ me: fixtures.me.lena });
    await userEvent.click(await screen.findByRole("button", { name: "Enter" }));
    await screen.findByText("Lena Fischer");
    await screen.findByText("teams shown");
    expect(teamCalls).toBe(2);
    const before = app.queryClient
      .getQueryCache()
      .getAll()
      .map((query) => [query.queryHash, query.state.dataUpdatedAt, query.state.isInvalidated]);

    await act(async () => {
      door.open();
      await new Promise((resolve) => setTimeout(resolve, 50));
    });

    expect(teamCalls).toBe(2);
    expect(outcome).toEqual([]);
    expect(
      app.queryClient
        .getQueryCache()
        .getAll()
        .map((query) => [query.queryHash, query.state.dataUpdatedAt, query.state.isInvalidated]),
    ).toEqual(before);
    expect(screen.getByText("Lena Fischer")).toBeInTheDocument();
  });

  test("a 401 after a sign-out shows no notice", async () => {
    startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    const told = vi.fn();
    const stop = onUnauthenticated(told);
    const write = aWriteOnItsWay(unauthenticated);
    await write.started();

    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    await act(async () => {
      write.answer();
      expect(await write.settled).toBe("SessionOverError");
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    expect(told).not.toHaveBeenCalled();
    expect(heading("Sign in")).toBeInTheDocument();
    expect(href(app)).toBe("/sign-in");
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByText(SESSION_ENDED)).toBeNull();
    stop();
  });
});

describe("sign out", () => {
  test("sign out ends the session", async () => {
    const gateway = startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    await app.queryClient.query(keysOptions());
    let sent: string | null = null;
    override("post", "/api/auth/logout", ({ request }) => {
      sent = request.headers.get("x-csrf-token");
      gateway.signedIn = false;
      return noContent();
    });
    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(sent).toBe(fixtures.csrfToken);
    expect(heading("Sign in")).toBeInTheDocument();
    expect(screen.queryByText(SESSION_ENDED)).toBeNull();
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(await tokenOfAWrite()).toBeNull();
  });

  const failing: [string, () => Response][] = [
    ["a network error", networkFailure],
    ["a 500", () => refuse(errors.internal_error)],
    ["a 401", unauthenticated],
    ["a 403", () => refuse(errors.csrf_failed)],
  ];

  const NOT_TOLD =
    "You are signed out here, but the gateway could not be reached, so your session there may still be active.";

  test("sign out says so when the gateway could not be told", async () => {
    startGateway({ signedIn: true });
    override("post", "/api/auth/logout", networkFailure);
    const app = await renderWithApp(null, { route: "/keys" });
    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(screen.getByRole("status")).toHaveTextContent(NOT_TOLD);
    expect(await tokenOfAWrite()).toBeNull();
  });

  test.each(failing.slice(1))("sign out that the gateway refused shows no notice: %s", async (_, answer) => {
    startGateway({ signedIn: true });
    override("post", "/api/auth/logout", answer);
    const app = await renderWithApp(null, { route: "/keys" });
    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(heading("Sign in")).toBeInTheDocument();
    expect(screen.queryByRole("status")).toBeNull();
  });

  test.each(failing)("sign out cleans up even if the call fails: %s", async (_, answer) => {
    startGateway({ signedIn: true });
    override("post", "/api/auth/logout", answer);
    const app = await renderWithApp(null, { route: "/keys" });
    await app.queryClient.query(keysOptions());
    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(heading("Sign in")).toBeInTheDocument();
    expect(screen.queryByText(SESSION_ENDED)).toBeNull();
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    expect(await tokenOfAWrite()).toBeNull();
  });
});

describe("another user signs in, in another window", () => {
  afterEach(forgetToasts);

  /** What the shell shows of who is signed in. */
  function shownUser(name: string): HTMLElement {
    return within(screen.getByRole("navigation", { name: "Main" })).getByText(name);
  }

  test("the tab drops what it held and starts again with the new user, as a reload does", async () => {
    startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    await app.queryClient.query(keysOptions());
    const before = app.queryClient.getQueryCache().getAll();
    expect(before.length).toBeGreaterThan(2);
    // A write and a mutation of Maya's are on their way, and a toast of hers shows.
    const door = gate();
    let asked = 0;
    override("post", "/api/teams", async () => {
      asked += 1;
      await door.opened;
      return ok("post", "/api/teams", 201, fixtures.teams.growth);
    });
    const write = api.post("/api/teams", { body: { name: "Growth" } }).then(
      () => "resolved",
      (error: unknown) => (error instanceof Error ? error.name : "rejected"),
    );
    await waitFor(() => {
      expect(asked).toBe(1);
    });
    void app.queryClient
      .getMutationCache()
      .build(app.queryClient, { mutationFn: () => door.opened })
      .execute(undefined)
      .catch(() => undefined);
    expect(app.queryClient.getMutationCache().getAll()).toHaveLength(1);
    const { result } = renderHook(() => useToast());
    act(() => {
      result.current("Saved.");
    });
    expect(await screen.findByText("Saved.")).toBeInTheDocument();

    // In another window Lena signs in: the cookie of the browser is hers now.
    const lena = startGateway({ signedIn: true, me: fixtures.me.lena });
    lena.csrfToken = "the-token-of-lena";
    theWindowGetsTheFocus();

    await waitFor(() => {
      expect(shownUser("Lena Fischer")).toBeInTheDocument();
    });
    expect(heading("Virtual keys")).toBeInTheDocument();
    expect(href(app)).toBe("/keys");
    // Nothing of before is left: every query is a new one, no mutation is kept.
    const after = app.queryClient.getQueryCache().getAll();
    expect(after.length).toBeGreaterThan(0);
    expect(after.filter((query) => before.includes(query))).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    await waitFor(() => {
      expect(toasts()).toEqual([]);
    });
    expect(await tokenOfAWrite()).toBe("the-token-of-lena");
    // What Maya's tab sent answers into a session that is over.
    door.open();
    expect(await write).toBe("SessionOverError");
  });

  test("the same user read again keeps what the tab holds", async () => {
    startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    await app.queryClient.query(keysOptions());
    const keys = app.queryClient.getQueryCache().find({ queryKey: keysOptions().queryKey });
    expect(keys).toBeDefined();
    const me = counted("get", "/api/auth/me", () => ok("get", "/api/auth/me", 200, fixtures.me.maya));
    theWindowGetsTheFocus();
    await waitFor(() => {
      expect(me.calls).toBe(1);
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    expect(app.queryClient.getQueryCache().find({ queryKey: keysOptions().queryKey })).toBe(keys);
    expect(shownUser("Maya Okafor")).toBeInTheDocument();
  });

  test("a new user whose session has no token for this tab: to the sign-in page, which says why", async () => {
    startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    override("get", "/api/auth/me", () =>
      ok("get", "/api/auth/me", 200, { ...fixtures.me.lena, csrf_token: null }),
    );
    theWindowGetsTheFocus();
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "You signed in with another account in another window.",
    );
    expect(heading("Sign in")).toBeInTheDocument();
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(await tokenOfAWrite()).toBeNull();
  });
});
