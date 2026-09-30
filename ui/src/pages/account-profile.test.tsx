import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HttpResponse } from "msw";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import {
  arjun,
  closed,
  keepsTheName,
  maya,
  openName,
  overlay,
  page,
  part,
  profile,
  said,
  shellName,
} from "@/test/account";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { ok, override, refuse } from "@/test/handlers";
import {
  counted,
  descriptionOf,
  expectOneRequestWhileTheDialogStays,
  forgetToasts,
  held,
  href,
  installSelect,
  SESSION_ENDED,
  settle,
  toasts,
} from "@/test/pages";
import { unauthenticated } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

describe("the profile", () => {
  test("it shows the name, the email, the role and the teams with the role in each", async () => {
    await page({ user: fixtures.me.arjun });
    expect([...profile().keys()]).toEqual(["Name", "Email", "Role", "Teams"]);
    expect(said("Name")).toHaveTextContent(arjun.name);
    expect(said("Email")).toHaveTextContent(arjun.email);
    const role = within(said("Role")).getByText("Member");
    expect(role).toHaveAttribute("data-slot", "badge");
    // Lead of Platform, member of Research.
    const teams = within(said("Teams")).getAllByRole("listitem");
    expect(teams.map((team) => team.textContent)).toEqual(["Platform Lead", "Research Member"]);
    for (const [index, text] of ["Lead", "Member"].entries()) {
      const team = teams[index];
      if (team === undefined) throw new Error("no team");
      expect(within(team).getByText(text)).toHaveAttribute("data-slot", "badge");
    }
  });

  test("an admin in no team", async () => {
    await page();
    expect(within(said("Role")).getByText("Admin")).toHaveAttribute("data-slot", "badge");
    expect(said("Teams")).toHaveTextContent("No teams");
    expect(within(said("Teams")).queryByRole("list")).toBeNull();
  });

  test("a role the console does not know is shown as it is, in a neutral pill", async () => {
    override("get", "/api/auth/me", () =>
      // What a later gateway may answer: the description of the API has neither value.
      HttpResponse.json({
        user: { ...arjun, role: "auditor" },
        teams: [{ team_id: 1, name: "Platform", role: "deputy" }],
        csrf_token: fixtures.csrfToken,
      }),
    );
    await page();
    for (const [label, text] of [
      ["Role", "auditor"],
      ["Teams", "deputy"],
    ] as const) {
      const pill = within(said(label)).getByText(text);
      expect(pill).toHaveAttribute("data-slot", "badge");
      expect(pill).toHaveAttribute("data-variant", "outline");
    }
  });

  test("profile name edit", async () => {
    const gateway = keepsTheName();
    await page();
    expect(said("Name")).toHaveTextContent(maya.name);
    expect(shellName()).toBe(maya.name);
    expect(gateway.me).toBe(1);

    const dialog = await openName();
    const name = within(dialog).getByLabelText("Name");
    expect(name).toHaveValue(maya.name);
    await userEvent.clear(name);
    await userEvent.type(name, "Maya O");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await closed();
    // The user's own id, and only the name.
    expect(gateway.patches).toEqual([{ name: "Maya O" }]);
    await waitFor(() => {
      expect(toasts()).toEqual(["Name changed."]);
    });
    // Who is signed in was asked for again: the profile and the shell follow.
    await waitFor(() => {
      expect(said("Name")).toHaveTextContent("Maya O");
    });
    expect(shellName()).toBe("Maya O");
    expect(gateway.me).toBe(2);
    expect(screen.queryByText(maya.name)).toBeNull();
    // The focus is back where the dialog was opened.
    await waitFor(() => {
      expect(within(part("Profile")).getByRole("button", { name: "Edit name" })).toHaveFocus();
    });
  });

  test("a member edits their own name: the call names their own id", async () => {
    // This gateway knows the id of who is signed in, and no other.
    const gateway = keepsTheName(fixtures.me.lena);
    await page();
    expect(said("Name")).toHaveTextContent(fixtures.users.lena.name);
    const dialog = await openName();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Lena F");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await closed();
    expect(gateway.patches).toEqual([{ name: "Lena F" }]);
    await waitFor(() => {
      expect(said("Name")).toHaveTextContent("Lena F");
    });
    expect(shellName()).toBe("Lena F");
  });

  test("a name that is not valid shows on its field, and the dialog keeps the value", async () => {
    const patches = counted("patch", "/api/users/{id}", () =>
      refuse(validationFailed({ name: fieldMessages.name })),
    );
    await page();
    const dialog = await openName();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(descriptionOf(name)).toBe(fieldMessages.name);
    });
    expect(name).toHaveValue("");
    expect(name).toHaveAttribute("aria-invalid", "true");
    expect(patches.calls).toBe(1);
    expect(toasts()).toEqual([]);

    // Closed and opened again, the dialog has the name as it is and no error.
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    expect(said("Name")).toHaveTextContent(maya.name);
    const again = await openName();
    expect(within(again).getByLabelText("Name")).toHaveValue(maya.name);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("while the name is saved the dialog stays: no Escape, no click beside it, no Cancel, no second submit", async () => {
    const door = gate();
    const patches = counted("patch", "/api/users/{id}", async () => {
      await door.opened;
      return ok("patch", "/api/users/{id}", 200, { ...maya, name: "Maya O" });
    });
    await page();
    const dialog = await openName();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, " O");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    const running = await within(dialog).findByRole("button", { name: "Saving" });
    expect(running).toBeDisabled();
    const open = () => screen.queryByRole("dialog", { name: "Edit name" });

    await userEvent.keyboard("{Escape}");
    expect(open()).toBe(dialog);
    await userEvent.click(overlay());
    expect(open()).toBe(dialog);
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(within(dialog).queryByRole("button", { name: "Close" })).toBeNull();
    await userEvent.type(name, "{Enter}");
    fireEvent.submit(within(dialog).getByRole("form", { name: "Edit name" }));
    await settle();
    expect(open()).toBe(dialog);
    expect(patches.calls).toBe(1);

    act(() => {
      door.open();
    });
    await closed();
    expect(patches.calls).toBe(1);
  });

  test("the name dialog: two submits at once are one request, and after the refusal the X leaves the dialog", async () => {
    const request = held("patch", "/api/users/{id}");
    await page();
    const dialog = await openName();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, " O");
    await expectOneRequestWhileTheDialogStays(dialog, name, "Saving", request);
    expect(request.bodies).toEqual([{ name: `${maya.name} O` }]);
    expect(toasts()).toEqual([]);
  });

  test("the session has ended when the name is saved: signed out, and the form says nothing", async () => {
    startGateway({ signedIn: true });
    const patches = counted("patch", "/api/users/{id}", unauthenticated);
    const app = await page();
    const dialog = await openName();
    await userEvent.type(within(dialog).getByLabelText("Name"), " O");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/account")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    await settle();
    expect(patches.calls).toBe(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(errors.unauthenticated.body.error.message)).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
  });
});
