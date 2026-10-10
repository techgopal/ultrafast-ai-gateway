import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import { ok, override, refuse } from "@/test/handlers";
import { errors, validationFailed } from "@/test/errors";
import { choose, forgetToasts, installSelect, toasts } from "@/test/pages";
import { renderWithApp, unauthenticated } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);


async function chosenIn(scope: HTMLElement): Promise<string[]> {
  const list = await within(scope).findByRole("list", { name: "Chosen guardrails" });
  return within(list)
    .getAllByRole("listitem")
    .map((item) => item.textContent.replace(/\s+/g, " ").trim());
}

const platform: fixtures.TeamDetail = { ...fixtures.teamDetails.platform, guardrail_ids: [2] };
const lena: fixtures.User = { ...fixtures.users.lena, guardrail_ids: [2] };

describe("the team page", () => {
  test("an admin edits the guardrails of the team: the whole list in order, and the page shows it afterwards", async () => {
    const puts: unknown[] = [];
    let held = platform;
    override("get", "/api/teams/{id}", () => ok("get", "/api/teams/{id}", 200, held));
    override("put", "/api/teams/{id}/guardrails", async ({ request }) => {
      const body = (await request.json()) as { guardrail_ids: number[] };
      puts.push(body);
      held = { ...held, guardrail_ids: body.guardrail_ids };
      return ok("put", "/api/teams/{id}/guardrails", 200, body);
    });
    await renderWithApp(null, { route: "/teams/1" });
    expect(await screen.findByRole("heading", { level: 1, name: "Platform" })).toBeInTheDocument();
    await userEvent.click(await screen.findByRole("button", { name: "Edit guardrails" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit guardrails" });
    expect(await chosenIn(dialog)).toEqual(["1.house-rules"]);
    expect(dialog).toHaveTextContent("every key of this team");
    await choose(within(dialog).getByRole("combobox", { name: "Add a guardrail" }), "acme-scanner");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(puts).toEqual([{ guardrail_ids: [2, 3] }]);
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(toasts()).toEqual(["Guardrails saved."]);
    const shown = await screen.findByRole("group", { name: "Guardrails" });
    expect(shown).toHaveTextContent("house-rules");
    expect(shown).toHaveTextContent("acme-scanner");
  });

  test("a lead of the team sees no control for them", async () => {
    override("get", "/api/teams/{id}", () => ok("get", "/api/teams/{id}", 200, platform));
    await renderWithApp(null, { route: "/teams/1", user: fixtures.me.arjun });
    expect(await screen.findByRole("heading", { level: 1, name: "Platform" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Edit guardrails" })).toBeNull();
  });

  test("a guardrail the gateway no longer has is said on the dialog", async () => {
    override("get", "/api/teams/{id}", () => ok("get", "/api/teams/{id}", 200, platform));
    override("put", "/api/teams/{id}/guardrails", () =>
      refuse(validationFailed({ guardrail_ids: "a guardrail does not exist" })),
    );
    await renderWithApp(null, { route: "/teams/1" });
    await userEvent.click(await screen.findByRole("button", { name: "Edit guardrails" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit guardrails" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText("a guardrail does not exist")).toBeInTheDocument();
    expect(toasts()).toEqual([]);
  });

  test("the session ends while they are saved", async () => {
    override("get", "/api/teams/{id}", () => ok("get", "/api/teams/{id}", 200, platform));
    await renderWithApp(null, { route: "/teams/1" });
    await userEvent.click(await screen.findByRole("button", { name: "Edit guardrails" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit guardrails" });
    override("put", "/api/teams/{id}/guardrails", unauthenticated);
    override("get", "/api/auth/me", unauthenticated);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Edit guardrails" })).toBeNull();
    });
    expect(toasts()).toEqual([]);
  });
});

describe("the user page", () => {
  test("an admin takes them all off: an empty list is sent", async () => {
    const puts: unknown[] = [];
    override("get", "/api/users/{id}", () => ok("get", "/api/users/{id}", 200, lena));
    override("put", "/api/users/{id}/guardrails", async ({ request }) => {
      const body = (await request.json()) as { guardrail_ids: number[] };
      puts.push(body);
      return ok("put", "/api/users/{id}/guardrails", 200, body);
    });
    await renderWithApp(null, { route: `/users/${lena.id}` });
    await userEvent.click(await screen.findByRole("button", { name: "Edit guardrails" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit guardrails" });
    expect(dialog).toHaveTextContent("every key this user owns");
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove house-rules" }));
    expect(within(dialog).getByText("None chosen.")).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(puts).toEqual([{ guardrail_ids: [] }]);
    });
    expect(toasts()).toEqual(["Guardrails saved."]);
  });

  test("a user who is not an admin sees the page of another user without the control", async () => {
    override("get", "/api/users/{id}", () => ok("get", "/api/users/{id}", 200, lena));
    await renderWithApp(null, { route: `/users/${lena.id}`, user: fixtures.me.arjun });
    expect(await screen.findByRole("heading", { level: 1, name: lena.name })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Edit guardrails" })).toBeNull();
  });

  test("the gateway refusing is said, not toasted", async () => {
    override("get", "/api/users/{id}", () => ok("get", "/api/users/{id}", 200, lena));
    override("put", "/api/users/{id}/guardrails", () => refuse(errors.forbidden));
    await renderWithApp(null, { route: `/users/${lena.id}` });
    await userEvent.click(await screen.findByRole("button", { name: "Edit guardrails" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit guardrails" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(within(dialog).getByRole("alert")).toBeInTheDocument();
    });
  });
});
