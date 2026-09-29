import type { QueryClient } from "@tanstack/react-query";
import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createBrowserHistory } from "@tanstack/react-router";
import { afterEach, describe, expect, test } from "vitest";
import { api } from "@/api/client";
import * as fixtures from "@/test/fixtures";
import { createQueryClient } from "@/api/queries";
import { gate, PASSWORD, startGateway } from "@/test/gateway";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import { networkFailure, noContent, ok, override, refuse } from "@/test/handlers";
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
    expect(confirm).toHaveFocus();
    expect(screen.getByRole("alert")).toHaveTextContent("The passwords do not match.");
    expect(sent).toEqual([]);
    expect(href(app)).toBe("/setup");
  });

  test("a 422 whose fields the form does not have shows the message of the API", async () => {
    startGateway({ needsSetup: true });
    override("post", "/api/setup", () =>
      // The gateway has no such field: this is a field the form does not know.
      refuse(validationFailed({ organisation: "It is too long." })),
    );
    await renderWithApp(null, { route: "/setup" });
    await fillSetup({});
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Some fields are not valid.");
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    const form = screen.getByRole("form", { name: "Set up the gateway" });
    expect(form.querySelector('[aria-invalid="true"]')).toBeNull();
    // It is at the top of the form, and the focus is on it.
    expect(alert.compareDocumentPosition(form) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    await waitFor(() => {
      expect(alert).toHaveFocus();
    });
  });

  test("a 422 with a field of the form and one it does not have shows both", async () => {
    startGateway({ needsSetup: true });
    override("post", "/api/setup", () =>
      // `organisation` is a field the gateway does not have, and the form neither.
      refuse(validationFailed({ organisation: "It is too long.", name: fieldMessages.name })),
    );
    await renderWithApp(null, { route: "/setup" });
    await fillSetup({});
    const name = screen.getByLabelText("Name");
    await waitFor(() => {
      expect(descriptionOf(name)).toBe("name must be 1 to 100 characters");
    });
    expect(name).toHaveFocus();
    expect(screen.getAllByRole("alert").map((a) => a.textContent)).toEqual([
      "Some fields are not valid.",
      "name must be 1 to 100 characters",
    ]);
  });

  test("setup shows field errors", async () => {
    startGateway({ needsSetup: true });
    override("post", "/api/setup", () =>
      refuse(validationFailed({ email: fieldMessages.email, password: fieldMessages.password })),
    );
    const app = await renderWithApp(null, { route: "/setup" });
    await fillSetup({ email: "maya@example", password: "short", confirm: "short" });
    const email = screen.getByLabelText("Email");
    await waitFor(() => {
      expect(descriptionOf(email)).toBe("email is not valid");
    });
    expect(email).toHaveAttribute("aria-invalid", "true");
    expect(email).toHaveValue("maya@example");
    // The first field with an error has the focus, and the errors are announced.
    expect(email).toHaveFocus();
    expect(screen.getAllByRole("alert").map((a) => a.textContent)).toEqual([
      "email is not valid",
      "password must be at least 12 characters",
    ]);
    // The text is under its field: the next element after the input.
    expect(email.nextElementSibling).toHaveTextContent("email is not valid");
    const password = screen.getByLabelText("Password");
    expect(descriptionOf(password)).toContain("password must be at least 12 characters");
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
      refuse(errors.already_set_up),
    );
    await renderWithApp(null, { route: "/setup" });
    await fillSetup({});
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Setup is already complete.");
    const link = screen.getByRole("link", { name: "Sign in" });
    expect(link).toHaveAttribute("href", "/sign-in");
    await userEvent.click(link);
    await waitFor(() => {
      expect(heading("Sign in")).toBeInTheDocument();
    });
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

const OPEN_AGAIN = "Open your invite link again. This page cannot be reloaded.";

/** The token is in none of the places something could read it from later. */
function expectNoToken(app: AppRenderResult): void {
  expect(shown()).not.toContain(INVITE_TOKEN);
  expect(JSON.stringify(app.router.state)).not.toContain(INVITE_TOKEN);
  expect(cached(app.queryClient)).not.toContain(INVITE_TOKEN);
  expect(stored()).not.toContain(INVITE_TOKEN);
  expect(window.location.href).not.toContain(INVITE_TOKEN);
}

/** Comes back to the page without a token in the address: it has none. */
async function expectTheFlowIsOver(app: AppRenderResult): Promise<void> {
  const sent = record("post", "/api/auth/accept-invite", noContent);
  await act(async () => {
    await app.router.navigate({ to: "/accept-invite" });
  });
  await waitFor(() => {
    expect(screen.getByText(OPEN_AGAIN)).toBeInTheDocument();
  });
  expect(screen.queryByLabelText("Password")).toBeNull();
  expect(screen.queryByRole("button", { name: "Set password" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Sign out and continue" })).toBeNull();
  expect(sent).toEqual([]);
  expectNoToken(app);
}

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
      refuse(errors.not_found),
    );
    const app = await renderWithApp(null, { route: `/accept-invite?token=${INVITE_TOKEN}` });
    await fillInvite();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(INVALID_INVITE);
    await waitFor(() => {
      expect(alert).toHaveFocus();
    });
    expect(screen.queryByText(errors.not_found.body.error.message)).toBeNull();
    // The token is of no use any more, and the form went with it.
    expect(screen.queryByLabelText("Password")).toBeNull();
    expect(screen.queryByRole("button", { name: "Set password" })).toBeNull();
    expectNoToken(app);
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
      refuse(validationFailed({ password: fieldMessages.password })),
    );
    await renderWithApp(null, { route: `/accept-invite?token=${INVITE_TOKEN}` });
    await fillInvite("short");
    const password = screen.getByLabelText("Password");
    await waitFor(() => {
      expect(descriptionOf(password)).toContain("password must be at least 12 characters");
    });
    expect(password).toHaveAttribute("aria-invalid", "true");
    expect(password).toHaveFocus();
    expect(screen.getByRole("alert")).toHaveTextContent("password must be at least 12 characters");
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
        screen.getByText("Open your invite link again. This page cannot be reloaded."),
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
      screen.getByText("Open your invite link again. This page cannot be reloaded."),
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
      screen.getByText("Open your invite link again. This page cannot be reloaded."),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).toBeNull();
    expect(sent).toEqual([]);
  });

  describe("the ways the flow ends", () => {
    test("the invite is accepted", async () => {
      startGateway();
      override("post", "/api/auth/accept-invite", noContent);
      const app = await renderWithApp(null, {
        history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}`),
      });
      await fillInvite();
      await waitFor(() => {
        expect(href(app)).toBe("/sign-in");
      });
      expectNoToken(app);
      await expectTheFlowIsOver(app);
    });

    test("the API refuses the invite", async () => {
      startGateway();
      override("post", "/api/auth/accept-invite", () =>
        refuse(errors.not_found),
      );
      const app = await renderWithApp(null, {
        history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}`),
      });
      await fillInvite();
      expect(await screen.findByRole("alert")).toHaveTextContent(INVALID_INVITE);
      expectNoToken(app);
      await act(async () => {
        await app.router.navigate({ to: "/sign-in" });
      });
      await expectTheFlowIsOver(app);
    });

    test("the user stays signed in", async () => {
      const gateway = startGateway({ signedIn: true });
      const app = await renderWithApp(null, {
        history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}`),
      });
      await userEvent.click(await screen.findByRole("link", { name: "Stay signed in" }));
      await waitFor(() => {
        expect(heading("Overview")).toBeInTheDocument();
      });
      expect(gateway.logouts).toBe(0);
      expectNoToken(app);
      await expectTheFlowIsOver(app);
    });

    test("the user goes to another route", async () => {
      startGateway();
      const app = await renderWithApp(null, {
        history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}`),
      });
      expect(await screen.findByLabelText("Password")).toBeInTheDocument();
      await act(async () => {
        await app.router.navigate({ to: "/sign-in" });
      });
      expect(heading("Sign in")).toBeInTheDocument();
      expectNoToken(app);
      await expectTheFlowIsOver(app);
    });

    test.each([
      ["answers", () => noContent()],
      ["fails", networkFailure],
    ])("the user leaves while the sign-out runs, which then %s", async (_, answer) => {
      startGateway({ signedIn: true });
      const door = gate();
      let asked = 0;
      override("post", "/api/auth/logout", async () => {
        asked += 1;
        await door.opened;
        return answer();
      });
      const app = await renderWithApp(null, {
        history: openInBrowser(`/accept-invite?token=${INVITE_TOKEN}`),
      });
      await userEvent.click(await screen.findByRole("button", { name: "Sign out and continue" }));
      await waitFor(() => {
        expect(asked).toBe(1);
      });
      // Both are disabled while the sign-out runs.
      expect(screen.getByRole("button", { name: /Sign(ing)? out/ })).toBeDisabled();
      const stay = screen.getByText("Stay signed in").closest("a, button");
      expect(stay).not.toBeNull();
      expect(
        stay?.getAttribute("aria-disabled") === "true" || stay?.hasAttribute("disabled") === true,
      ).toBe(true);
      expect(stay).not.toHaveAttribute("href");

      // The user leaves by the address, and the call settles after that.
      await act(async () => {
        await app.router.navigate({ to: "/keys" });
      });
      await act(async () => {
        door.open();
        await new Promise((resolve) => setTimeout(resolve, 30));
      });
      await waitFor(() => {
        expect(heading("Sign in")).toBeInTheDocument();
      });
      expectNoToken(app);
      await expectTheFlowIsOver(app);
    });
  });

  test.each([
    ["the app is loading", () => undefined],
    [
      "the gateway cannot be reached",
      () => {
        override("get", "/api/setup", networkFailure);
      },
    ],
  ])("the token leaves the URL while %s", async (_, arrange) => {
    startGateway();
    const door = gate();
    override("get", "/api/auth/me", async () => {
      await door.opened;
      return refuse(errors.unauthenticated);
    });
    arrange();
    window.history.replaceState(null, "", `/accept-invite?token=${INVITE_TOKEN}`);
    const { AppProviders } = await import("@/providers");
    const { createAppRouter } = await import("@/router");
    const { RouterProvider } = await import("@tanstack/react-router");
    const { render } = await import("@testing-library/react");
    const router = createAppRouter({ history: createBrowserHistory() });
    const view = render(
      <AppProviders queryClient={createQueryClient({ retry: false })}>
        <RouterProvider router={router} />
      </AppProviders>,
    );
    await waitFor(() => {
      expect(window.location.href).not.toContain("token");
    });
    // The session is not known yet: the page itself is not there.
    expect(screen.queryByRole("heading", { name: "Accept your invite" })).toBeNull();
    expect(window.location.pathname).toBe("/accept-invite");
    expect(JSON.stringify(router.state)).not.toContain(INVITE_TOKEN);
    expect(shown()).not.toContain(INVITE_TOKEN);
    door.open();
    view.unmount();
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
