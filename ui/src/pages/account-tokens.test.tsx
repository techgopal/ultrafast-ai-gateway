import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, onTestFinished, test, vi } from "vitest";
import {
  active,
  askToRevoke,
  cellsOf,
  changeButton,
  closed,
  closeSecret,
  confirm,
  expired,
  keeps,
  maya,
  neverUsed,
  openCreate,
  overlay,
  page,
  part,
  PART_NOT_AVAILABLE,
  REVOKE,
  revoked,
  rowOf,
  said,
  SECRET,
  secretDialog,
  send,
  SHOWN_ONCE,
  table,
  tokensAre,
} from "@/test/account";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, noContent, ok, override, refuse } from "@/test/handlers";
import {
  aCallFindsTheSessionEnded,
  cached,
  counted,
  descriptionOf,
  expectLabelsNameControls,
  expectNoSecret,
  expectOneRequestWhileTheDialogStays,
  expectOneH1,
  expectOneMain,
  forgetToasts,
  held,
  href,
  installSelect,
  listenToConsole,
  SESSION_ENDED,
  settle,
  shown,
  theClockIs,
  toasts,
} from "@/test/pages";
import { unauthenticated } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

describe("the access tokens", () => {
  test("the list: name, what is shown of the token, expiry, last use and status", async () => {
    // The status is worked out with the clock.
    theClockIs(fixtures.now);
    await page();
    const tokens = await table();
    expect(within(tokens).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Name",
      "Token",
      "Expires",
      "Last used",
      "Status",
      "Actions",
    ]);
    for (const token of fixtures.tokenList) {
      const display = within(rowOf(token.name)).getByText(token.display);
      expect(display.className.split(/\s+/)).toContain("font-mono");
    }
    // The status is worked out from what the API gives: it has no field for it.
    expect(fixtures.tokenList.map((token) => cellsOf(token.name)[4])).toEqual([
      "active",
      "active",
      "revoked",
      "expired",
    ]);
    for (const token of fixtures.tokenList) {
      const status = cellsOf(token.name)[4] ?? "";
      expect(within(rowOf(token.name)).getByText(status)).toHaveAttribute("data-slot", "badge");
    }
    // Times are times; what never was says so.
    expect(cellsOf(active.name)[2]).toBe("Never");
    expect(cellsOf(neverUsed.name)[3]).toBe("Never");
    // Its expiry is far away: the first day of 2999 in UTC, the day before west of it.
    expect(within(rowOf(neverUsed.name)).getByText(/2999|2998/).tagName).toBe("TIME");
    expect(rowOf(active.name).querySelector("time")).toHaveAttribute(
      "title",
      `${active.last_used_at} UTC`,
    );
    // What is revoked cannot be revoked again; the rest can.
    expect(within(rowOf(revoked.name)).queryByRole("button")).toBeNull();
    for (const token of [active, neverUsed, expired]) {
      expect(within(rowOf(token.name)).getByRole("button", { name: "Revoke" })).toBeInTheDocument();
    }
    // A token itself is nowhere: the list has only what the gateway shows of it.
    expect(shown()).not.toContain(SECRET);
    expect(shown()).not.toMatch(/uf-at-[0-9a-f]{8}/);
  });

  test("loading shows skeleton rows", async () => {
    const door = gate();
    override("get", "/api/tokens", async () => {
      await door.opened;
      return ok("get", "/api/tokens", 200, { tokens: fixtures.tokenList });
    });
    await page();
    const tokens = screen.getByRole("table", { name: "Access tokens" });
    expect(tokens).toHaveAttribute("aria-busy", "true");
    expect(tokens.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    // The rest of the page does not wait for them.
    expect(said("Name")).toHaveTextContent(maya.name);
    expect(changeButton()).toBeEnabled();
    act(() => {
      door.open();
    });
    await table();
    expect(tokens.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0);
  });

  test("an empty list says so", async () => {
    tokensAre([]);
    await page();
    expect(await screen.findByRole("heading", { name: "No access tokens" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    expect(
      within(part("Access tokens")).getByRole("button", { name: "Create token" }),
    ).toBeInTheDocument();
  });

  test("a failed list call shows the error with Retry, and Retry asks again; the rest of the page stays", async () => {
    const failing = counted("get", "/api/tokens", () => refuse(errors.internal_error));
    await page();
    const tokens = part("Access tokens");
    expect(await within(tokens).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.queryByRole("table")).toBeNull();
    expect(within(tokens).queryByRole("button", { name: "Create token" })).toBeNull();
    expect(toasts()).toEqual([]);
    expect(failing.calls).toBe(1);
    expect(said("Email")).toHaveTextContent(maya.email);
    expect(changeButton()).toBeInTheDocument();

    const again = tokensAre(fixtures.tokenList);
    await userEvent.click(within(tokens).getByRole("button", { name: "Retry" }));
    await table();
    expect(again.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(within(tokens).getByRole("button", { name: "Create token" })).toBeInTheDocument();
  });

  test("a gateway that cannot be reached is an error with Retry", async () => {
    override("get", "/api/tokens", networkFailure);
    await page();
    expect(await within(part("Access tokens")).findByRole("alert")).toHaveTextContent(
      "Could not reach the gateway.",
    );
    expect(within(part("Access tokens")).getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("the list call answers 403: the part says that it is not available, not an error and not a toast", async () => {
    const refused = counted("get", "/api/tokens", () => refuse(errors.forbidden));
    await page();
    const tokens = part("Access tokens");
    expect(await within(tokens).findByText(PART_NOT_AVAILABLE)).toBeInTheDocument();
    expect(refused.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(errors.forbidden.body.error.message)).toBeNull();
    expect(within(tokens).queryByRole("button")).toBeNull();
    expect(screen.queryByRole("table")).toBeNull();
    expect(toasts()).toEqual([]);
    // The profile and the password are the user's own whatever the tokens are.
    expect(screen.getByRole("heading", { level: 1, name: "Account" })).toBeInTheDocument();
    expect(said("Email")).toHaveTextContent(maya.email);
    expect(changeButton()).toBeInTheDocument();
    expectOneMain();
    expectOneH1();
  });

  test("at width 390 the rows are cards with their labels and actions", async () => {
    await page({ width: 390 });
    const cards = await screen.findByRole("list", { name: "Access tokens" });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    expect(screen.queryByRole("table")).toBeNull();
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(fixtures.tokenList.length);
    const [first] = items;
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Name",
      "Token",
      "Expires",
      "Last used",
      "Status",
    ]);
    expect(first).toHaveTextContent(active.name);
    const revoke = within(first).getByRole("button", { name: "Revoke" });
    expect(revoke.className.split(/\s+/)).toContain("min-h-11");
  });

  test("the list sorts by name", async () => {
    await page();
    await table();
    const names = () =>
      within(screen.getByRole("table", { name: "Access tokens" }))
        .getAllByRole("row")
        .slice(1)
        .map((row) => within(row).getAllByRole("cell")[0]?.textContent);
    expect(names()).toEqual(fixtures.tokenList.map((token) => token.name));
    await userEvent.click(screen.getByRole("button", { name: "Name" }));
    expect(names()).toEqual(fixtures.tokenList.map((token) => token.name).sort());
  });
});

describe("creating a token", () => {
  test("token is shown once", async () => {
    const written = listenToConsole();
    const state = keeps();
    const app = await page();
    await table();
    expect(state.lists).toBe(1);
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);

    const secret = await secretDialog();
    expect(screen.queryByRole("dialog", { name: "Create token" })).toBeNull();
    expect(secret).toHaveTextContent(SHOWN_ONCE);
    expect(within(secret).getByLabelText("Your new access token")).toHaveValue(SECRET);
    // How to use it: the header with a placeholder.
    const usage = within(secret).getByRole("group", { name: "How to use the token" });
    expect(usage).toHaveTextContent("Use it as Authorization: Bearer <token> with the admin API.");
    expect(usage.querySelector("code")).toHaveTextContent("Authorization: Bearer <token>");
    expect(usage.textContent).not.toContain(SECRET);
    expect(usage.textContent).not.toContain(SECRET.slice(-8));
    expect(secret.textContent).not.toContain(SECRET);
    // While it is shown, the token is in the state of the page only.
    expect(cached(app.queryClient)).not.toContain(SECRET);
    expect(JSON.stringify(app.router.state)).not.toContain(SECRET);
    expect(href(app)).toBe("/account");
    expect(toasts()).toEqual([]);

    await closeSecret();
    expectNoSecret(app, SECRET);
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    expect(toasts().join()).not.toContain(SECRET);
    // The list was asked for again, and has the token.
    expect(await screen.findByRole("cell", { name: "deploy" })).toBeInTheDocument();
    expect(state.lists).toBe(2);
    expect(state.created).toEqual([{ name: "deploy" }]);
    // Nothing was written to the console about it.
    expect(written()).not.toContain(SECRET);
    expect(written()).not.toContain(SECRET.slice(6));
  });

  test("the labels of the form name controls, and the group of expiries is named once", async () => {
    await page();
    await table();
    const dialog = await openCreate();
    expectLabelsNameControls(dialog);
    const expires = within(dialog).getByRole("radiogroup", { name: "Expires" });
    expect(within(expires).getAllByRole("radio")).toEqual(
      ["Never", "In 30 days", "In 90 days", "On a date"].map((name) =>
        within(expires).getByRole("radio", { name }),
      ),
    );
    expect(within(expires).getByRole("radio", { name: "Never" })).toBeChecked();
    expect(descriptionOf(expires)).toBe("A token expires at the end of its day, in UTC.");
    expect(within(dialog).queryByLabelText("Expiry date")).toBeNull();
  });

  describe("expiry is sent in UTC", () => {
    /** Late in the UTC day: the next day already east of Greenwich, the same day west of it. */
    const NOW = "2026-10-01T23:30:00Z";

    async function created(choice: string, date?: string): Promise<unknown> {
      vi.setSystemTime(new Date(NOW));
      onTestFinished(() => {
        vi.useRealTimers();
      });
      const state = keeps();
      await page();
      await table();
      const dialog = await openCreate();
      await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
      const expires = within(dialog).getByRole("radiogroup", { name: "Expires" });
      if (choice !== "Never") {
        await userEvent.click(within(expires).getByRole("radio", { name: choice }));
      }
      if (date !== undefined) {
        const day = within(dialog).getByLabelText("Expiry date");
        expect(day).toHaveAttribute("type", "date");
        expect(day).toHaveAttribute("min", "2026-10-01");
        fireEvent.change(day, { target: { value: date } });
      }
      await send(dialog);
      await secretDialog();
      expect(state.created).toHaveLength(1);
      return state.created[0];
    }

    test("never sends no expires_at", async () => {
      const body = await created("Never");
      expect(body).toEqual({ name: "deploy" });
      expect(Object.keys(body as object)).not.toContain("expires_at");
    });

    test("a date is sent as the end of that day", async () => {
      expect(await created("On a date", "2027-01-31")).toEqual({
        name: "deploy",
        expires_at: "2027-01-31 23:59:59",
      });
    });

    test("30 days are counted from today in UTC", async () => {
      expect(await created("In 30 days")).toEqual({
        name: "deploy",
        expires_at: "2026-10-31 23:59:59",
      });
    });

    test("90 days are counted from today in UTC", async () => {
      expect(await created("In 90 days")).toEqual({
        name: "deploy",
        expires_at: "2026-12-30 23:59:59",
      });
    });
  });

  test("without a date nothing is sent, and the field says so", async () => {
    const state = keeps();
    await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    const expires = within(dialog).getByRole("radiogroup", { name: "Expires" });
    await userEvent.click(within(expires).getByRole("radio", { name: "On a date" }));
    await send(dialog);
    await waitFor(() => {
      expect(descriptionOf(expires)).toContain("Choose a date.");
    });
    expect(within(dialog).getByLabelText("Expiry date")).toHaveAttribute("aria-invalid", "true");
    await settle();
    expect(state.created).toEqual([]);
    expect(toasts()).toEqual([]);
  });

  test("the errors of the fields show on their fields, and the dialog keeps what was typed", async () => {
    const posts = counted("post", "/api/tokens", () =>
      refuse(
        validationFailed({ name: fieldMessages.name, expires_at: fieldMessages.expiresAtPast }),
      ),
    );
    await page();
    await table();
    const dialog = await openCreate();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, "x");
    await send(dialog);
    await waitFor(() => {
      expect(descriptionOf(name)).toBe(fieldMessages.name);
    });
    expect(name).toHaveAttribute("aria-invalid", "true");
    const expires = within(dialog).getByRole("radiogroup", { name: "Expires" });
    expect(descriptionOf(expires)).toContain(fieldMessages.expiresAtPast);
    expect(name).toHaveValue("x");
    expect(posts.calls).toBe(1);
    expect(toasts()).toEqual([]);
    expect(screen.queryByRole("dialog", { name: "Your new access token" })).toBeNull();
  });

  test("a refusal that is about no field shows at the top of the form", async () => {
    override("post", "/api/tokens", () => refuse(errors.internal_error));
    await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(within(dialog).getByLabelText("Name")).not.toHaveAttribute("aria-invalid");
  });

  test("a dialog that is opened again is empty and shows no old error", async () => {
    override("post", "/api/tokens", () => refuse(errors.internal_error));
    const app = await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    await within(dialog).findByRole("alert");
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    const again = await openCreate();
    expect(within(again).getByLabelText("Name")).toHaveValue("");
    expect(within(again).queryByRole("alert")).toBeNull();
    expect(within(again).getByRole("radio", { name: "Never" })).toBeChecked();
  });

  describe("while the token is created the dialog stays", () => {
    async function sending(answer: () => Response) {
      const door = gate();
      const posts = counted("post", "/api/tokens", async () => {
        await door.opened;
        return answer();
      });
      const app = await page();
      await table();
      const dialog = await openCreate();
      const name = within(dialog).getByLabelText("Name");
      await userEvent.type(name, "deploy");
      await send(dialog);
      const running = await within(dialog).findByRole("button", { name: "Creating the token" });
      expect(running).toBeDisabled();
      return { app, dialog, name, posts, door };
    }

    test("Escape, a click beside it, Cancel and a second submit do nothing; one request, and the token is shown once", async () => {
      const { app, dialog, name, posts, door } = await sending(() =>
        ok("post", "/api/tokens", 201, { token: active, secret: SECRET }),
      );
      const open = () => screen.queryByRole("dialog", { name: "Create token" });

      await userEvent.keyboard("{Escape}");
      expect(open()).toBe(dialog);
      await userEvent.click(overlay());
      expect(open()).toBe(dialog);
      expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
      expect(within(dialog).queryByRole("button", { name: "Close" })).toBeNull();
      await userEvent.type(name, "{Enter}");
      fireEvent.submit(within(dialog).getByRole("form", { name: "Create token" }));
      await settle();
      expect(open()).toBe(dialog);
      expect(name).toHaveValue("deploy");
      expect(posts.calls).toBe(1);

      act(() => {
        door.open();
      });
      const secret = await secretDialog();
      expect(within(secret).getByLabelText("Your new access token")).toHaveValue(SECRET);
      expect(open()).toBeNull();
      await closeSecret();
      await settle();
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(posts.calls).toBe(1);
      expectNoSecret(app, SECRET);
      await waitFor(() => {
        expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
      });
    });

    test("two submits at once are one request, and after the refusal the X leaves the dialog", async () => {
      const request = held("post", "/api/tokens");
      await page();
      await table();
      const dialog = await openCreate();
      const name = within(dialog).getByLabelText("Name");
      await userEvent.type(name, "deploy");
      await expectOneRequestWhileTheDialogStays(dialog, name, "Creating the token", request);
      expect(request.bodies).toEqual([{ name: "deploy" }]);
    });

    test("after a refusal it can be left again", async () => {
      const { dialog, posts, door } = await sending(() => refuse(errors.forbidden));
      act(() => {
        door.open();
      });
      expect(await within(dialog).findByRole("alert")).toHaveTextContent(
        errors.forbidden.body.error.message,
      );
      await waitFor(() => {
        expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
      });
      expect(within(dialog).getByRole("button", { name: "Close" })).toBeInTheDocument();
      await userEvent.keyboard("{Escape}");
      await closed();
      expect(posts.calls).toBe(1);
    });
  });

  test("the session has ended when the token is created: signed out, and the form says nothing", async () => {
    startGateway({ signedIn: true });
    const posts = counted("post", "/api/tokens", unauthenticated);
    const app = await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    await settle();
    expect(posts.calls).toBe(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(errors.unauthenticated.body.error.message)).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
  });

  test("a token whose answer came for a session that is over is not shown", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("post", "/api/tokens", async () => {
      await door.opened;
      return ok("post", "/api/tokens", 201, { token: active, secret: SECRET });
    });
    const app = await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    await within(dialog).findByRole("button", { name: "Creating the token" });
    await aCallFindsTheSessionEnded("/api/providers");
    door.open();
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expectNoSecret(app, SECRET);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
  });

  test("the session ends while the new token is shown", async () => {
    startGateway({ signedIn: true });
    const app = await page();
    await table();
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    await secretDialog();
    expect(shown()).toContain(SECRET);

    await aCallFindsTheSessionEnded("/api/providers");

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expectNoSecret(app, SECRET);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Faccount");
    });
    expectNoSecret(app, SECRET);
    expect(toasts()).toEqual([]);
  });

  test("at width 390 the controls of the dialog are high enough to touch", async () => {
    await page({ width: 390 });
    await screen.findByRole("list", { name: "Access tokens" });
    const dialog = await openCreate();
    for (const control of [
      within(dialog).getByLabelText("Name"),
      within(dialog).getByRole("button", { name: "Create token" }),
      within(dialog).getByRole("button", { name: "Cancel" }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
    expect(dialog.className).toContain("max-w-[calc(100%-2rem)]");
  });
});

describe("revoking a token", () => {
  test("token revoke", async () => {
    // The status of the other tokens is worked out with the clock.
    theClockIs(fixtures.now);
    const state = keeps();
    await page();
    await table();
    const dialog = await askToRevoke(active);
    expect(dialog).toHaveAccessibleDescription(REVOKE);
    expect(state.revoked).toEqual([]);
    await confirm(dialog);
    await closed();
    expect(state.revoked).toEqual([String(active.id)]);
    // The toast names nothing.
    expect(toasts()).toEqual(["Token revoked."]);
    // The list was asked for again: the pill of the token changes where it is.
    await waitFor(() => {
      expect(cellsOf(active.name)[4]).toBe("revoked");
    });
    expect(state.lists).toBe(2);
    expect(within(rowOf(active.name)).getByText("revoked")).toHaveAttribute("data-slot", "badge");
    // What is revoked cannot be revoked again.
    expect(within(rowOf(active.name)).queryByRole("button")).toBeNull();
    // The others are as they were.
    expect(cellsOf(neverUsed.name)[4]).toBe("active");
  });

  test("cancel changes nothing", async () => {
    const state = keeps();
    await page();
    await table();
    const dialog = await askToRevoke(active);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    expect(state.revoked).toEqual([]);
    expect(toasts()).toEqual([]);
    expect(cellsOf(active.name)[4]).toBe("active");
    expect(state.lists).toBe(1);
  });

  test("a refusal stays in the dialog", async () => {
    const state = keeps();
    const removes = counted("delete", "/api/tokens/{id}", () => refuse(errors.not_found));
    await page();
    await table();
    const dialog = await askToRevoke(active);
    await confirm(dialog);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.not_found.body.error.message,
    );
    expect(removes.calls).toBe(1);
    expect(toasts()).toEqual([]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    expect(cellsOf(active.name)[4]).toBe("active");
    expect(state.revoked).toEqual([]);
  });

  test("a revocation whose answer came for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("delete", "/api/tokens/{id}", async () => {
      await door.opened;
      return noContent();
    });
    const app = await page();
    await table();
    const dialog = await askToRevoke(active);
    await confirm(dialog);
    await aCallFindsTheSessionEnded("/api/providers");
    door.open();
    await settle();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
  });
});
