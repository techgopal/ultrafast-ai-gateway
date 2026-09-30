import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HttpResponse } from "msw";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { queryKeys } from "@/api/queries";
import {
  active,
  cellsOf,
  changeButton,
  CURRENT,
  DIFFER,
  expectCleared,
  field,
  fields,
  fill,
  formError,
  keeps,
  maya,
  neverUsed,
  NEXT,
  openCreate,
  page,
  part,
  PASSWORD_CHANGED,
  passwordIs,
  secretDialog,
  send,
  table,
  TOO_MANY,
  WRONG_CURRENT,
} from "@/test/account";
import { errors, fieldMessages } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, noContent, override, refuse } from "@/test/handlers";
import {
  aCallFindsTheSessionEnded,
  counted,
  descriptionOf,
  expectLabelsNameControls,
  expectNoSecret,
  forgetToasts,
  href,
  installSelect,
  listenToConsole,
  SESSION_ENDED,
  settle,
  toasts,
} from "@/test/pages";
import { unauthenticated } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

describe("the password", () => {
  test("the form: three fields that hide what is typed, and say what they are for", async () => {
    await page();
    expect(fields().map((one) => one.getAttribute("type"))).toEqual([
      "password",
      "password",
      "password",
    ]);
    expect(fields().map((one) => one.getAttribute("autocomplete"))).toEqual([
      "current-password",
      "new-password",
      "new-password",
    ]);
    // The names the API has for them; the confirmation is the console's own.
    expect(fields().map((one) => one.getAttribute("name"))).toEqual([
      "current_password",
      "new_password",
      "confirm_password",
    ]);
    for (const one of fields()) expect(one).toBeRequired();
    // Whose password it is, for the password manager of the browser: not shown, not sent.
    const form = within(part("Password")).getByRole("form", { name: "Change password" });
    const username = form.querySelector('input[autocomplete="username"]');
    expect(username).toHaveValue(maya.email);
    expect(username).toHaveAttribute("hidden");
    expect(username).toHaveAttribute("readonly");
    expect(username).not.toBeVisible();
    expect(form.querySelectorAll("input")).toHaveLength(4);
    expect(descriptionOf(field("New password"))).toBe("12 characters or more");
    expectLabelsNameControls(part("Password"));
    expectCleared();
  });

  test("password change success", async () => {
    startGateway({ signedIn: true });
    const tokens = keeps();
    const gateway = passwordIs(CURRENT, tokens);
    const app = await page();
    await table();
    expect(tokens.lists).toBe(1);
    expect(cellsOf(active.name)[4]).toBe("active");

    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    // The message of rule 3.
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    // What the API takes: the confirmation is not sent.
    expect(gateway.bodies).toEqual([{ current_password: CURRENT, new_password: NEXT }]);
    expect(gateway.csrf).toEqual([fixtures.csrfToken]);
    // The fields are cleared, and nothing keeps the passwords.
    expectCleared();
    expect(screen.queryByRole("alert")).toBeNull();
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    for (const password of [CURRENT, NEXT]) expectNoSecret(app, password);

    // All access tokens were ended: the list is read again, and says so.
    await waitFor(() => {
      expect(cellsOf(active.name)[4]).toBe("revoked");
    });
    expect(tokens.lists).toBe(2);
    expect(cellsOf(neverUsed.name)[4]).toBe("revoked");
    expect(within(part("Access tokens")).queryByRole("button", { name: "Revoke" })).toBeNull();

    // The session of the caller goes on: still signed in, with the token it had.
    expect(href(app)).toBe("/account");
    expect(screen.getByRole("heading", { level: 1, name: "Account" })).toBeInTheDocument();
    expect(screen.queryByText(SESSION_ENDED)).toBeNull();
    expect(app.queryClient.getQueryData(queryKeys.me())).toBeDefined();
    // A further call succeeds.
    const dialog = await openCreate();
    await userEvent.type(within(dialog).getByLabelText("Name"), "deploy");
    await send(dialog);
    expect(await secretDialog()).toBeInTheDocument();
    expect(tokens.csrf).toEqual([fixtures.csrfToken]);
  });

  test("two submits of the password form at once are one request", async () => {
    const door = gate();
    const posts = counted("post", "/api/auth/password", async () => {
      await door.opened;
      return noContent();
    });
    await page();
    await fill(CURRENT, NEXT);
    const form = within(part("Password")).getByRole("form", { name: "Change password" });
    act(() => {
      fireEvent.submit(form);
      fireEvent.submit(form);
    });
    await within(part("Password")).findByRole("button", { name: "Changing the password" });
    await settle();
    expect(posts.calls).toBe(1);
    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    expect(posts.calls).toBe(1);
  });

  test("while the password is changed the button is disabled and says so; one request", async () => {
    const door = gate();
    const posts = counted("post", "/api/auth/password", async () => {
      await door.opened;
      return noContent();
    });
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    const running = await within(part("Password")).findByRole("button", {
      name: "Changing the password",
    });
    expect(running).toBeDisabled();
    await userEvent.type(field("Confirm new password"), "{Enter}");
    fireEvent.submit(within(part("Password")).getByRole("form", { name: "Change password" }));
    await settle();
    expect(posts.calls).toBe(1);
    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    expect(changeButton()).toBeEnabled();
    expect(posts.calls).toBe(1);
    expectCleared();
  });

  test("after the change the focus is back on the button, which lost it while it was disabled", async () => {
    const door = gate();
    override("post", "/api/auth/password", async () => {
      await door.opened;
      return noContent();
    });
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    const running = await within(part("Password")).findByRole("button", {
      name: "Changing the password",
    });
    expect(running).toBeDisabled();
    // A browser takes the focus from a button that is disabled, and nothing has it
    // then. jsdom leaves it: here it goes as it does when what has it is removed.
    act(() => {
      const probe = document.createElement("button");
      document.body.append(probe);
      probe.focus();
      probe.remove();
    });
    expect(document.body).toHaveFocus();
    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    await waitFor(() => {
      expect(changeButton()).toHaveFocus();
    });
  });

  test("a change that was sent from a field leaves the focus in that field", async () => {
    passwordIs(CURRENT);
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.type(field("Confirm new password"), "{Enter}");
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    await settle();
    expect(field("Confirm new password")).toHaveFocus();
    expectCleared();
  });

  test("wrong current password", async () => {
    const session = startGateway({ signedIn: true });
    const gateway = passwordIs(CURRENT);
    const app = await page();
    await table();
    const asked = session.meCalls;

    await fill("not-the-password", NEXT);
    await userEvent.click(changeButton());
    // Shown on the current field, in the words of the form.
    const current = field("Current password");
    await waitFor(() => {
      expect(descriptionOf(current)).toBe(WRONG_CURRENT);
    });
    expect(current).toHaveAttribute("aria-invalid", "true");
    expect(within(part("Password")).getAllByRole("alert")).toHaveLength(1);
    expect(screen.queryByText(errors.invalid_credentials.body.error.message)).toBeNull();
    expect(gateway.bodies).toEqual([{ current_password: "not-the-password", new_password: NEXT }]);
    // The three fields are cleared, and the focus is on the first of them.
    expectCleared();
    await waitFor(() => {
      expect(current).toHaveFocus();
    });
    expect(toasts()).toEqual([]);
    // Nothing keeps what was typed.
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    for (const password of ["not-the-password", NEXT]) expectNoSecret(app, password);

    // The user is still signed in: this 401 is not the end of a session.
    await settle();
    expect(href(app)).toBe("/account");
    expect(screen.getByRole("heading", { level: 1, name: "Account" })).toBeInTheDocument();
    expect(screen.queryByText(SESSION_ENDED)).toBeNull();
    expect(app.queryClient.getQueryData(queryKeys.me())).toBeDefined();
    expect(app.queryClient.getQueryData(queryKeys.tokens.list())).toBeDefined();
    expect(session.meCalls).toBe(asked);
    expect(screen.getByRole("table", { name: "Access tokens" })).toBeInTheDocument();

    // A second submit right after it sends nothing: the form is empty.
    await userEvent.click(changeButton());
    await userEvent.type(current, "{Enter}");
    fireEvent.submit(within(part("Password")).getByRole("form", { name: "Change password" }));
    await settle();
    expect(gateway.bodies).toHaveLength(1);
    expect(descriptionOf(current)).toBe(WRONG_CURRENT);

    // The error goes when the field is typed in, and the right password is taken.
    await fill(CURRENT, NEXT);
    expect(descriptionOf(current)).toBe("");
    expect(current).not.toHaveAttribute("aria-invalid");
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    expect(gateway.bodies).toHaveLength(2);
  });

  test("password mismatch sends nothing", async () => {
    const posts = counted("post", "/api/auth/password", noContent);
    await page();
    await fill(CURRENT, NEXT, "next-staple-correct-8");
    await userEvent.click(changeButton());
    const confirmation = field("Confirm new password");
    await waitFor(() => {
      expect(descriptionOf(confirmation)).toBe(DIFFER);
    });
    expect(confirmation).toHaveAttribute("aria-invalid", "true");
    await waitFor(() => {
      expect(confirmation).toHaveFocus();
    });
    await settle();
    expect(posts.calls).toBe(0);
    expect(toasts()).toEqual([]);
    // Nothing was sent, so nothing was refused: what was typed stays, to be corrected.
    expect(field("Current password")).toHaveValue(CURRENT);
    expect(field("New password")).toHaveValue(NEXT);
    expect(confirmation).toHaveValue("next-staple-correct-8");

    // Corrected, it is sent.
    await userEvent.clear(confirmation);
    expect(descriptionOf(confirmation)).toBe("");
    await userEvent.type(confirmation, NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    expect(posts.bodies).toEqual([{ current_password: CURRENT, new_password: NEXT }]);
    expectCleared();
  });

  test.each([
    ["nothing is typed", "", "", ""],
    ["the current password is missing", "", NEXT, NEXT],
    ["the new password is missing", CURRENT, "", ""],
    ["the confirmation is missing", CURRENT, NEXT, ""],
  ])("a form in which %s is not sent", async (_, current, next, confirmation) => {
    const posts = counted("post", "/api/auth/password", noContent);
    await page();
    await fill(current, next, confirmation);
    await userEvent.click(changeButton());
    await userEvent.type(field("New password"), "{Enter}");
    fireEvent.submit(within(part("Password")).getByRole("form", { name: "Change password" }));
    await settle();
    // An attempt with a wrong current password counts against the limit of the
    // gateway, and an empty one is wrong: none is made for nothing. (A 422, a
    // 429 and a 401 without a session do not count.)
    expect(posts.calls).toBe(0);
    expect(toasts()).toEqual([]);
  });

  test("too many attempts", async () => {
    const posts = counted("post", "/api/auth/password", () => refuse(errors.too_many_attempts));
    const app = await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    const current = field("Current password");
    // The text of the sign-in page, not the one of the gateway, at the top of
    // the form as there: it is about no field.
    await waitFor(() => {
      expect(formError()).toHaveTextContent(TOO_MANY);
    });
    expect(formError()?.textContent).toBe(TOO_MANY);
    expect(within(part("Password")).getAllByRole("alert")).toEqual([formError()]);
    for (const one of fields()) expect(one).not.toHaveAttribute("aria-invalid");
    expect(descriptionOf(current)).toBe("");
    expect(screen.queryByText(errors.too_many_attempts.body.error.message)).toBeNull();
    expectCleared();
    await waitFor(() => {
      expect(current).toHaveFocus();
    });
    expect(posts.calls).toBe(1);
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe("/account");
    for (const password of [CURRENT, NEXT]) expectNoSecret(app, password);
  });

  test("too many attempts stays while the form is typed in, and goes when the form is sent again", async () => {
    const door = gate();
    const posts = counted("post", "/api/auth/password", async () => {
      // The first attempt is one too many; the second is held, and then taken.
      if (posts.calls === 1) return refuse(errors.too_many_attempts);
      await door.opened;
      return noContent();
    });
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(formError()).toHaveTextContent(TOO_MANY);
    });

    // A keystroke in a field does not take it away: the limit lasts for minutes.
    await userEvent.type(field("Current password"), "c");
    expect(formError()).toHaveTextContent(TOO_MANY);
    await fill(CURRENT, NEXT);
    expect(formError()).toHaveTextContent(TOO_MANY);
    expect(posts.calls).toBe(1);

    // The next submit starts without it.
    await userEvent.click(changeButton());
    await within(part("Password")).findByRole("button", { name: "Changing the password" });
    expect(formError()).toBeNull();
    expect(posts.calls).toBe(2);
    act(() => {
      door.open();
    });
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    expect(within(part("Password")).queryByRole("alert")).toBeNull();
  });

  test("a press right after too many attempts sends nothing, and the message stays", async () => {
    const posts = counted("post", "/api/auth/password", () => refuse(errors.too_many_attempts));
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(formError()).toHaveTextContent(TOO_MANY);
    });
    // The form is empty.
    await userEvent.click(changeButton());
    await userEvent.type(field("Current password"), "{Enter}");
    fireEvent.submit(within(part("Password")).getByRole("form", { name: "Change password" }));
    await settle();
    expect(posts.calls).toBe(1);
    expect(formError()).toHaveTextContent(TOO_MANY);

    // Sent again and refused again, it says so again.
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(posts.calls).toBe(2);
    });
    await waitFor(() => {
      expect(formError()).toHaveTextContent(TOO_MANY);
    });
    expectCleared();
  });

  test("a 429 is known by its status, whatever its code", async () => {
    override("post", "/api/auth/password", () =>
      // What a proxy or a later gateway may answer: the description of the API has no such code.
      HttpResponse.json({ error: { code: "rate_limited", message: "Slow down." } }, { status: 429 }),
    );
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(formError()?.textContent).toBe(TOO_MANY);
    });
    expect(screen.queryByText("Slow down.")).toBeNull();
    for (const one of fields()) expect(one).not.toHaveAttribute("aria-invalid");
    expectCleared();
  });

  test("a new password the gateway does not take shows on its field; the fields are cleared", async () => {
    const gateway = passwordIs(CURRENT);
    await page();
    await fill(CURRENT, "short", "short");
    await userEvent.click(changeButton());
    const next = field("New password");
    await waitFor(() => {
      expect(descriptionOf(next)).toBe(`${fieldMessages.password} 12 characters or more`);
    });
    expect(next).toHaveAttribute("aria-invalid", "true");
    expect(field("Current password")).not.toHaveAttribute("aria-invalid");
    expect(gateway.bodies).toHaveLength(1);
    expectCleared();
    // All three are empty: the user starts again at the first.
    await waitFor(() => {
      expect(field("Current password")).toHaveFocus();
    });
    expect(toasts()).toEqual([]);
  });

  test("a refusal that is about no field shows at the top of the form; the fields are cleared", async () => {
    override("post", "/api/auth/password", () => refuse(errors.internal_error));
    const app = await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    expect(await within(part("Password")).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    for (const one of fields()) expect(one).not.toHaveAttribute("aria-invalid");
    expectCleared();
    expect(toasts()).toEqual([]);
    for (const password of [CURRENT, NEXT]) expectNoSecret(app, password);
  });

  test("a gateway that cannot be reached: the form says so, and the fields are cleared", async () => {
    override("post", "/api/auth/password", networkFailure);
    await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    expect(await within(part("Password")).findByRole("alert")).toHaveTextContent(
      "Could not reach the gateway.",
    );
    expectCleared();
  });

  test("the session has ended when the password is changed: signed out, and the form says nothing", async () => {
    startGateway({ signedIn: true });
    const posts = counted("post", "/api/auth/password", unauthenticated);
    const app = await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    await settle();
    expect(posts.calls).toBe(1);
    expect(screen.getByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    // No error of a field, none of a form, and not the answer of the gateway.
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(WRONG_CURRENT)).toBeNull();
    expect(screen.queryByText(errors.unauthenticated.body.error.message)).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    for (const password of [CURRENT, NEXT]) expectNoSecret(app, password);
  });

  test("a change whose answer came for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("post", "/api/auth/password", async () => {
      await door.opened;
      return noContent();
    });
    const app = await page();
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    await within(part("Password")).findByRole("button", { name: "Changing the password" });
    await aCallFindsTheSessionEnded("/api/providers");
    door.open();
    await settle();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
    for (const password of [CURRENT, NEXT]) expectNoSecret(app, password);
  });

  test("nothing is written to the console about a password", async () => {
    const written = listenToConsole();
    passwordIs(CURRENT);
    await page();
    await fill("not-the-password", NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(descriptionOf(field("Current password"))).toBe(WRONG_CURRENT);
    });
    await fill(CURRENT, NEXT);
    await userEvent.click(changeButton());
    await waitFor(() => {
      expect(toasts()).toEqual([PASSWORD_CHANGED]);
    });
    for (const password of ["not-the-password", CURRENT, NEXT]) {
      expect(written()).not.toContain(password);
    }
  });
});
