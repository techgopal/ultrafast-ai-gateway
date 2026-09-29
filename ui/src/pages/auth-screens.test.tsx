import type { QueryClient } from "@tanstack/react-query";
import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createBrowserHistory } from "@tanstack/react-router";
import { afterEach, describe, expect, test } from "vitest";
import { api } from "@/api/client";
import * as fixtures from "@/test/fixtures";
import { PASSWORD, startGateway } from "@/test/gateway";
import { apiError, networkFailure, noContent, ok, override } from "@/test/handlers";
import { renderWithApp, type AppRenderResult } from "@/test/render";

const INVITE_TOKEN = "invite-token-0000-of-the-test";
const INVALID_INVITE =
  "This invite link is not valid or has expired. Ask an admin for a new one.";
const POLICY = "12 characters or more";

function href(app: AppRenderResult): string {
  return app.router.state.location.href;
}

function heading(name: string): HTMLElement {
  return screen.getByRole("heading", { name });
}

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

function shown(): string {
  const values = [...document.querySelectorAll("input")].map((field) => field.value);
  return document.documentElement.outerHTML + JSON.stringify(values);
}

function stored(): string {
  return JSON.stringify([
    Object.entries(window.localStorage),
    Object.entries(window.sessionStorage),
    document.cookie,
    window.history.state,
  ]);
}

/** The text that `aria-describedby` of the field points to. */
function descriptionOf(field: HTMLElement): string {
  const ids = (field.getAttribute("aria-describedby") ?? "").split(/\s+/).filter(Boolean);
  return ids.map((id) => document.getElementById(id)?.textContent ?? "").join(" | ");
}

/** The requests to the operation, with their bodies. */
function record(
  method: "post",
  path: "/api/setup" | "/api/auth/accept-invite",
  answer: () => Response,
): unknown[] {
  const bodies: unknown[] = [];
  override(method, path, async ({ request }) => {
    bodies.push(await request.json());
    return answer();
  });
  return bodies;
}

async function fillSetup(values: {
  name?: string;
  email?: string;
  password?: string;
  confirm?: string;
}): Promise<void> {
  const user = userEvent.setup();
  await user.type(screen.getByLabelText("Name"), values.name ?? "Maya Okafor");
  await user.type(screen.getByLabelText("Email"), values.email ?? "maya@example.test");
  await user.type(screen.getByLabelText("Password"), values.password ?? PASSWORD);
  await user.type(screen.getByLabelText("Confirm password"), values.confirm ?? PASSWORD);
  await user.click(screen.getByRole("button", { name: "Create admin account" }));
}

async function fillInvite(password = PASSWORD, confirm = password): Promise<void> {
  const user = userEvent.setup();
  await user.type(screen.getByLabelText("Password"), password);
  await user.type(screen.getByLabelText("Confirm password"), confirm);
  await user.click(screen.getByRole("button", { name: "Set password" }));
}

describe("setup", () => {
  test("setup creates the admin and goes to sign-in with a notice", async () => {
    const gateway = startGateway({ needsSetup: true });
    const sent = record("post", "/api/setup", () => {
      gateway.needsSetup = false;
      return ok("post", "/api/setup", 201, fixtures.users.maya);
    });
    const app = await renderWithApp(null, { route: "/" });
    await waitFor(() => {
      expect(heading("Set up the gateway")).toBeInTheDocument();
    });
    await fillSetup({});
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(sent).toEqual([
      { name: "Maya Okafor", email: "maya@example.test", password: PASSWORD },
    ]);
    expect(heading("Sign in")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "The admin account is created. Sign in to continue.",
    );
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    expect(cached(app.queryClient)).not.toContain(PASSWORD);
    expect(shown()).not.toContain(PASSWORD);
    expect(JSON.stringify(app.router.state)).not.toContain(PASSWORD);
  });

  test("setup validates before sending", async () => {
    startGateway({ needsSetup: true });
    const sent = record("post", "/api/setup", () =>
      ok("post", "/api/setup", 201, fixtures.users.maya),
    );
    const app = await renderWithApp(null, { route: "/setup" });
    await fillSetup({ confirm: "another-password-0000" });
    const confirm = screen.getByLabelText("Confirm password");
    await waitFor(() => {
      expect(descriptionOf(confirm)).toContain("The passwords do not match.");
    });
    expect(confirm).toHaveAttribute("aria-invalid", "true");
    expect(sent).toEqual([]);
    expect(href(app)).toBe("/setup");
  });

  test("setup shows field errors", async () => {
    startGateway({ needsSetup: true });
    override("post", "/api/setup", () =>
      apiError(422, "validation_failed", "Some fields are not valid.", {
        email: "This is not an email address.",
        password: "Use 12 characters or more.",
      }),
    );
    const app = await renderWithApp(null, { route: "/setup" });
    await fillSetup({ email: "maya@example", password: "short", confirm: "short" });
    const email = screen.getByLabelText("Email");
    await waitFor(() => {
      expect(descriptionOf(email)).toBe("This is not an email address.");
    });
    expect(email).toHaveAttribute("aria-invalid", "true");
    expect(email).toHaveValue("maya@example");
    // The text is under its field: the next element after the input.
    expect(email.nextElementSibling).toHaveTextContent("This is not an email address.");
    const password = screen.getByLabelText("Password");
    expect(descriptionOf(password)).toContain("Use 12 characters or more.");
    expect(password).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Name")).not.toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Name")).toHaveValue("Maya Okafor");
    // The passwords are gone after the attempt.
    expect(password).toHaveValue("");
    expect(screen.getByLabelText("Confirm password")).toHaveValue("");
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    expect(href(app)).toBe("/setup");
  });

  test("setup already done", async () => {
    startGateway({ needsSetup: true });
    override("post", "/api/setup", () =>
      apiError(409, "already_set_up", "The gateway is set up."),
    );
    await renderWithApp(null, { route: "/setup" });
    await fillSetup({});
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Setup is already complete.");
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/sign-in");
  });

  test("setup shows a network error", async () => {
    startGateway({ needsSetup: true });
    override("post", "/api/setup", networkFailure);
    await renderWithApp(null, { route: "/setup" });
    await fillSetup({});
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(screen.getByLabelText("Password")).toHaveValue("");
  });

  test("the password fields of setup show the policy and are new passwords", async () => {
    startGateway({ needsSetup: true });
    await renderWithApp(null, { route: "/setup" });
    const password = screen.getByLabelText("Password");
    expect(descriptionOf(password)).toContain(POLICY);
    for (const field of [password, screen.getByLabelText("Confirm password")]) {
      expect(field).toHaveAttribute("autocomplete", "new-password");
      expect(field).toHaveAttribute("type", "password");
    }
    expect(screen.getByLabelText("Email")).toHaveAttribute("autocomplete", "username");
    expect(screen.getByLabelText("Name")).toHaveAttribute("autocomplete", "name");
  });
});

describe("accept invite", () => {
  afterEach(() => {
    window.history.replaceState(null, "", "/");
  });

  function openInBrowser(path: string) {
    window.history.replaceState(null, "", path);
    return createBrowserHistory();
  }

  test("invite token leaves the URL", async () => {
    startGateway();
    const sent = record("post", "/api/auth/accept-invite", noContent);
    const before = window.history.length;
    const app = await renderWithApp(null, {
      history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}`),
    });
    await waitFor(() => {
      expect(window.location.search).toBe("");
    });
    expect(window.location.pathname).toBe("/accept-invite");
    expect(window.location.href).not.toContain(INVITE_TOKEN);
    // The entry was replaced: there is no entry to go back to that holds the token.
    expect(window.history.length).toBe(before);
    expect(heading("Accept your invite")).toBeInTheDocument();

    // It is nowhere but in the page's memory.
    expect(JSON.stringify(app.router.state)).not.toContain(INVITE_TOKEN);
    expect(cached(app.queryClient)).not.toContain(INVITE_TOKEN);
    expect(stored()).not.toContain(INVITE_TOKEN);
    expect(shown()).not.toContain(INVITE_TOKEN);

    await fillInvite();
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(sent).toEqual([{ token: INVITE_TOKEN, password: PASSWORD }]);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Your password is set. Sign in to continue.",
    );
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    for (const secret of [INVITE_TOKEN, PASSWORD]) {
      expect(JSON.stringify(app.router.state)).not.toContain(secret);
      expect(cached(app.queryClient)).not.toContain(secret);
      expect(stored()).not.toContain(secret);
      expect(shown()).not.toContain(secret);
    }
  });

  test("the other parts of the address stay when the token leaves", async () => {
    startGateway();
    await renderWithApp(null, {
      history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}&lang=en#top`),
    });
    await waitFor(() => {
      expect(window.location.href).not.toContain("token");
    });
    expect(window.location.href).not.toContain(INVITE_TOKEN);
    expect(window.location.pathname).toBe("/accept-invite");
  });

  test("invalid invite", async () => {
    startGateway();
    override("post", "/api/auth/accept-invite", () =>
      apiError(404, "not_found", "It does not exist."),
    );
    const app = await renderWithApp(null, { route: `/accept-invite?token=${INVITE_TOKEN}` });
    await fillInvite();
    expect(await screen.findByRole("alert")).toHaveTextContent(INVALID_INVITE);
    expect(screen.queryByText("It does not exist.")).toBeNull();
    expect(screen.getByLabelText("Password")).toHaveValue("");
    expect(screen.getByLabelText("Confirm password")).toHaveValue("");
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    expect(shown()).not.toContain(INVITE_TOKEN);
  });

  test("passwords that differ send nothing", async () => {
    startGateway();
    const sent = record("post", "/api/auth/accept-invite", noContent);
    await renderWithApp(null, { route: `/accept-invite?token=${INVITE_TOKEN}` });
    await fillInvite(PASSWORD, "another-password-0000");
    const confirm = screen.getByLabelText("Confirm password");
    await waitFor(() => {
      expect(descriptionOf(confirm)).toContain("The passwords do not match.");
    });
    expect(sent).toEqual([]);
  });

  test("a field error of the password is shown on its field", async () => {
    startGateway();
    override("post", "/api/auth/accept-invite", () =>
      apiError(422, "validation_failed", "Some fields are not valid.", {
        password: "Use 12 characters or more.",
      }),
    );
    await renderWithApp(null, { route: `/accept-invite?token=${INVITE_TOKEN}` });
    await fillInvite("short");
    const password = screen.getByLabelText("Password");
    await waitFor(() => {
      expect(descriptionOf(password)).toContain("Use 12 characters or more.");
    });
    expect(password).toHaveAttribute("aria-invalid", "true");
  });

  test("the password fields show the policy and are new passwords", async () => {
    startGateway();
    await renderWithApp(null, { route: `/accept-invite?token=${INVITE_TOKEN}` });
    const password = screen.getByLabelText("Password");
    expect(descriptionOf(password)).toContain(POLICY);
    for (const field of [password, screen.getByLabelText("Confirm password")]) {
      expect(field).toHaveAttribute("autocomplete", "new-password");
    }
  });

  test.each(["/accept-invite", "/accept-invite?token=", "/accept-invite?other=1"])(
    "without a token the page says to open the link again: %s",
    async (route) => {
      startGateway();
      const sent = record("post", "/api/auth/accept-invite", noContent);
      await renderWithApp(null, { route });
      expect(heading("Accept your invite")).toBeInTheDocument();
      expect(
        screen.getByText("Open the link of your invite again. This page cannot be reloaded."),
      ).toBeInTheDocument();
      expect(screen.queryByLabelText("Password")).toBeNull();
      expect(screen.queryByRole("button", { name: "Set password" })).toBeNull();
      expect(sent).toEqual([]);
    },
  );

  test("a reload after the token left the URL does not bring the token back", async () => {
    startGateway();
    const first = await renderWithApp(null, {
      history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}`),
    });
    await waitFor(() => {
      expect(window.location.search).toBe("");
    });
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    first.unmount();

    // The reload: a new app at the address the browser has now.
    await renderWithApp(null, { history: createBrowserHistory() });
    expect(
      screen.getByText("Open the link of your invite again. This page cannot be reloaded."),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).toBeNull();
  });

  test("a signed-in user is asked, then signed out, and the invite goes on", async () => {
    const gateway = startGateway({ signedIn: true, me: fixtures.me.lena });
    const sent = record("post", "/api/auth/accept-invite", noContent);
    const app = await renderWithApp(null, {
      history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}`),
    });
    await waitFor(() => {
      expect(window.location.search).toBe("");
    });
    expect(screen.getByText(/You are signed in as lena@example\.test\./)).toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).toBeNull();
    expect(gateway.logouts).toBe(0);
    expect(screen.getByRole("link", { name: "Stay signed in" })).toHaveAttribute("href", "/");

    await userEvent.click(screen.getByRole("button", { name: "Sign out and continue" }));
    await waitFor(() => {
      expect(screen.getByLabelText("Password")).toBeInTheDocument();
    });
    expect(gateway.logouts).toBe(1);
    expect(window.location.pathname).toBe("/accept-invite");
    expect(window.location.search).toBe("");
    expect(app.queryClient.getQueryCache().getAll().filter((q) => q.state.data !== undefined && q.queryKey[0] !== "setup" && q.queryKey[0] !== "me")).toEqual([]);

    await fillInvite();
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in");
    });
    expect(sent).toEqual([{ token: INVITE_TOKEN, password: PASSWORD }]);
  });

  test("the token does not outlive the page that took it over the sign-out", async () => {
    startGateway({ signedIn: true });
    const sent = record("post", "/api/auth/accept-invite", noContent);
    const app = await renderWithApp(null, {
      history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}`),
    });
    await userEvent.click(await screen.findByRole("button", { name: "Sign out and continue" }));
    await waitFor(() => {
      expect(screen.getByLabelText("Password")).toBeInTheDocument();
    });
    await act(async () => {
      await app.router.navigate({ to: "/sign-in" });
    });
    expect(heading("Sign in")).toBeInTheDocument();
    await act(async () => {
      await app.router.navigate({ to: "/accept-invite" });
    });
    expect(
      screen.getByText("Open the link of your invite again. This page cannot be reloaded."),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).toBeNull();
    expect(sent).toEqual([]);
  });

  test("no link of the page carries the token", async () => {
    startGateway({ signedIn: true });
    await renderWithApp(null, {
      history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}`),
    });
    await waitFor(() => {
      expect(window.location.search).toBe("");
    });
    const links = [...document.querySelectorAll("a")].map((a) => a.getAttribute("href"));
    expect(links.length).toBeGreaterThan(0);
    for (const link of links) {
      expect(link).not.toContain("token");
      expect(link).toMatch(/^\/(?!\/)/);
    }
    // A write elsewhere does not carry it either.
    let seen = "";
    override("post", "/api/teams", async ({ request }) => {
      seen = request.url + JSON.stringify([...request.headers]) + (await request.text());
      return noContent();
    });
    await api.post("/api/teams", { body: { name: "x" } });
    expect(seen).not.toContain(INVITE_TOKEN);
  });
});
