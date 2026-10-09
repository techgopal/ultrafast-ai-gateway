import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import type { components } from "@/api/schema";
import { errors, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { ok, override, refuse } from "@/test/handlers";
import {
  choose,
  counted,
  expectOneH1,
  forgetToasts,
  installSelect,
  optionsOf,
  rowWithCell,
  settle,
  toasts,
} from "@/test/pages";
import { renderWithApp, unauthenticated } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

type RouteRequest = components["schemas"]["RouteRequest"];

const refs = {
  pii: { id: 1, name: "mask-emails" },
  words: { id: 2, name: "house-rules" },
  external: { id: 3, name: "acme-scanner" },
};

async function table(name: string): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

async function chosenIn(scope: HTMLElement): Promise<string[]> {
  const list = await within(scope).findByRole("list", { name: "Chosen guardrails" });
  return within(list)
    .getAllByRole("listitem")
    .map((item) => item.textContent.replace(/\s+/g, " ").trim());
}

describe("the keys page", () => {
  const withGuardrails: fixtures.Key = { ...fixtures.keys.active, guardrails: [refs.words, refs.pii] };

  test("shows the guardrails of a key in the order they run, to anyone who sees the key", async () => {
    override("get", "/api/keys", () =>
      ok("get", "/api/keys", 200, { keys: [withGuardrails, fixtures.keys.noOwner] }),
    );
    await renderWithApp(null, { route: "/keys", user: fixtures.me.lena });
    await table("Virtual keys");
    const row = rowWithCell(withGuardrails.name);
    const chips = within(row).getByRole("group", { name: "Guardrails" });
    expect(within(chips).getAllByText(/./).map((chip) => chip.textContent)).toEqual([
      "house-rules",
      "mask-emails",
    ]);
    expect(row).not.toHaveTextContent("Edit guardrails");
    // A lead reads them and cannot change them.
    expect(screen.queryByRole("button", { name: "Edit guardrails" })).toBeNull();
    expect(rowWithCell(fixtures.keys.noOwner.name)).toHaveTextContent("None");
  });

  test("an admin changes them: the whole list in its order, and the list shows it afterwards", async () => {
    const patched: Record<string, unknown>[] = [];
    let keys: fixtures.Key[] = [withGuardrails];
    override("get", "/api/keys", () => ok("get", "/api/keys", 200, { keys }));
    override("patch", "/api/keys/{id}", async ({ request }) => {
      const body = (await request.json()) as Record<string, unknown>;
      patched.push(body);
      const ids = Array.isArray(body.guardrail_ids) ? (body.guardrail_ids as number[]) : [];
      keys = keys.map((key) => ({
        ...key,
        guardrails: ids.flatMap((id) => Object.values(refs).filter((ref) => ref.id === id)),
      }));
      return ok("patch", "/api/keys/{id}", 200, keys[0] ?? withGuardrails);
    });
    await renderWithApp(null, { route: "/keys" });
    await table("Virtual keys");
    await userEvent.click(within(rowWithCell(withGuardrails.name)).getByRole("button", { name: "Edit guardrails" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit guardrails" });
    expect(await chosenIn(dialog)).toEqual(["1.house-rules", "2.mask-emailsEvery call"]);
    // Order: the first goes down, one is added at the end, one is taken off.
    await userEvent.click(within(dialog).getByRole("button", { name: "Move house-rules down" }));
    expect(await chosenIn(dialog)).toEqual(["1.mask-emailsEvery call", "2.house-rules"]);
    expect(within(dialog).getByRole("button", { name: "Move mask-emails up" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Move house-rules down" })).toBeDisabled();
    await choose(within(dialog).getByRole("combobox", { name: "Add a guardrail" }), "acme-scanner");
    expect(await chosenIn(dialog)).toEqual(["1.mask-emailsEvery call", "2.house-rules", "3.acme-scanner"]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove house-rules" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(patched).toEqual([{ guardrail_ids: [1, 3] }]);
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(toasts()).toEqual(["Guardrails saved."]);
    const chips = within(rowWithCell(withGuardrails.name)).getByRole("group", { name: "Guardrails" });
    expect(chips).toHaveTextContent("mask-emails");
    expect(chips).toHaveTextContent("acme-scanner");
  });

  test("taking them all off sends an empty list", async () => {
    const patched: Record<string, unknown>[] = [];
    override("get", "/api/keys", () => ok("get", "/api/keys", 200, { keys: [withGuardrails] }));
    override("patch", "/api/keys/{id}", async ({ request }) => {
      patched.push((await request.json()) as Record<string, unknown>);
      return ok("patch", "/api/keys/{id}", 200, withGuardrails);
    });
    await renderWithApp(null, { route: "/keys" });
    await table("Virtual keys");
    await userEvent.click(within(rowWithCell(withGuardrails.name)).getByRole("button", { name: "Edit guardrails" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit guardrails" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove house-rules" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove mask-emails" }));
    expect(within(dialog).getByText("None chosen.")).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(patched).toEqual([{ guardrail_ids: [] }]);
    });
  });

  test("a guardrail the gateway no longer has is said on the dialog", async () => {
    override("get", "/api/keys", () => ok("get", "/api/keys", 200, { keys: [withGuardrails] }));
    override("patch", "/api/keys/{id}", () =>
      refuse(validationFailed({ guardrail_ids: "a guardrail does not exist" })),
    );
    await renderWithApp(null, { route: "/keys" });
    await table("Virtual keys");
    await userEvent.click(within(rowWithCell(withGuardrails.name)).getByRole("button", { name: "Edit guardrails" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit guardrails" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText("a guardrail does not exist")).toBeInTheDocument();
    expect(toasts()).toEqual([]);
  });

  test("a revoked key has nothing to edit", async () => {
    override("get", "/api/keys", () => ok("get", "/api/keys", 200, { keys: [fixtures.keys.revoked] }));
    await renderWithApp(null, { route: "/keys" });
    await userEvent.click(await screen.findByRole("checkbox", { name: "Show revoked" }));
    await table("Virtual keys");
    expect(screen.queryByRole("button", { name: "Edit guardrails" })).toBeNull();
  });

  test("the form of a new key offers guardrails to an admin and sends them in order", async () => {
    const created: Record<string, unknown>[] = [];
    override("post", "/api/keys", async ({ request }) => {
      created.push((await request.json()) as Record<string, unknown>);
      return ok("post", "/api/keys", 201, { key: fixtures.keys.active, secret: fixtures.newKeySecret });
    });
    await renderWithApp(null, { route: "/keys" });
    await table("Virtual keys");
    await userEvent.click(await screen.findByRole("button", { name: "Create key" }));
    const dialog = await screen.findByRole("dialog", { name: "Create key" });
    await within(dialog).findByRole("combobox", { name: "Owner" });
    await userEvent.click(within(dialog).getByLabelText("Name"));
    await userEvent.paste("ci");
    await choose(await within(dialog).findByRole("combobox", { name: "Add a guardrail" }), "house-rules");
    await choose(within(dialog).getByRole("combobox", { name: "Add a guardrail" }), "acme-scanner");
    expect(await chosenIn(dialog)).toEqual(["1.house-rules", "2.acme-scanner"]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Create key" }));
    await screen.findByRole("dialog", { name: "Your new key" });
    expect(created).toEqual([{ name: "ci", guardrail_ids: [2, 3] }]);
  });

  test("the form of a new key sends no guardrails when none are chosen, and offers none to a member", async () => {
    const created: Record<string, unknown>[] = [];
    override("post", "/api/keys", async ({ request }) => {
      created.push((await request.json()) as Record<string, unknown>);
      return ok("post", "/api/keys", 201, { key: fixtures.keys.active, secret: fixtures.newKeySecret });
    });
    const asked = counted("get", "/api/guardrails", () => refuse(errors.forbidden));
    await renderWithApp(null, { route: "/keys", user: fixtures.me.tomas });
    await table("Virtual keys");
    await userEvent.click(await screen.findByRole("button", { name: "Create key" }));
    const dialog = await screen.findByRole("dialog", { name: "Create key" });
    await userEvent.click(within(dialog).getByLabelText("Name"));
    await userEvent.paste("mine");
    expect(within(dialog).queryByText("Guardrails")).toBeNull();
    await userEvent.click(within(dialog).getByRole("button", { name: "Create key" }));
    await screen.findByRole("dialog", { name: "Your new key" });
    expect(created).toEqual([{ name: "mine" }]);
    await settle();
    expect(asked.calls).toBe(0);
  });

  test("the session ends while the guardrails of a key are saved", async () => {
    override("get", "/api/keys", () => ok("get", "/api/keys", 200, { keys: [withGuardrails] }));
    await renderWithApp(null, { route: "/keys" });
    await table("Virtual keys");
    await userEvent.click(within(rowWithCell(withGuardrails.name)).getByRole("button", { name: "Edit guardrails" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit guardrails" });
    override("patch", "/api/keys/{id}", unauthenticated);
    override("get", "/api/auth/me", unauthenticated);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Edit guardrails" })).toBeNull();
    });
    expect(toasts()).toEqual([]);
  });
});

describe("the picker", () => {
  const three: fixtures.Key = {
    ...fixtures.keys.active,
    guardrails: [refs.words, refs.pii, refs.external],
  };

  async function editKey(key: fixtures.Key): Promise<HTMLElement> {
    override("get", "/api/keys", () => ok("get", "/api/keys", 200, { keys: [key] }));
    await renderWithApp(null, { route: "/keys" });
    await table("Virtual keys");
    await userEvent.click(within(rowWithCell(key.name)).getByRole("button", { name: "Edit guardrails" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit guardrails" });
    await chosenIn(dialog);
    return dialog;
  }

  test("says on a key and on a route what a guardrail there does not cover", async () => {
    const dialog = await editKey(three);
    expect(dialog).toHaveTextContent(
      "These check only the calls made with this key: a new key, or a direct call to a model, is not checked by them.",
    );
  });

  test("says it on the form of a route too", async () => {
    const route: fixtures.Route = { ...fixtures.routes.support, guardrails: [refs.pii] };
    override("get", "/api/routes", () => ok("get", "/api/routes", 200, { routes: [route] }));
    override("get", "/api/routes/{id}", () => ok("get", "/api/routes/{id}", 200, route));
    await renderWithApp(null, { route: "/routes/1" });
    const form = await screen.findByRole("form", { name: "Route" });
    expect(form).toHaveTextContent(
      "These check only the calls that go through this route: a key that may also call a model directly is not checked by them.",
    );
  });

  test("does not offer a guardrail that applies to every call", async () => {
    const dialog = await editKey({ ...fixtures.keys.active, guardrails: [refs.words] });
    expect(await optionsOf(within(dialog).getByRole("combobox", { name: "Add a guardrail" }))).toEqual([
      "acme-scanner",
      "imported-scanner (disabled)",
    ]);
  });

  test("marks a chosen guardrail that applies to every call", async () => {
    const marked = await editKey(three);
    expect(await chosenIn(marked)).toEqual([
      "1.house-rules",
      "2.mask-emailsEvery call",
      "3.acme-scanner",
    ]);
  });

  test("keeps the focus where the person is after a move", async () => {
    const dialog = await editKey(three);
    // into the middle, the same button
    await userEvent.click(within(dialog).getByRole("button", { name: "Move house-rules down" }));
    expect(within(dialog).getByRole("button", { name: "Move house-rules down" })).toHaveFocus();
    // to the end, where down is off: the button that goes back
    await userEvent.click(within(dialog).getByRole("button", { name: "Move house-rules down" }));
    expect(within(dialog).getByRole("button", { name: "Move house-rules up" })).toHaveFocus();
    // to the start, where up is off: the button that goes on
    expect(await chosenIn(dialog)).toEqual(["1.mask-emailsEvery call", "2.acme-scanner", "3.house-rules"]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Move acme-scanner up" }));
    expect(within(dialog).getByRole("button", { name: "Move acme-scanner down" })).toHaveFocus();
  });

  test("keeps the focus in the list after a removal", async () => {
    const dialog = await editKey(three);
    // the next row takes it
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove house-rules" }));
    expect(within(dialog).getByRole("button", { name: "Remove mask-emails" })).toHaveFocus();
    // the last row: the one before it
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove acme-scanner" }));
    expect(within(dialog).getByRole("button", { name: "Remove mask-emails" })).toHaveFocus();
    // the only row: the select that adds
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove mask-emails" }));
    expect(within(dialog).getByRole("combobox", { name: "Add a guardrail" })).toHaveFocus();
  });

  test("says what happened to the order, politely", async () => {
    const dialog = await editKey(three);
    const live = within(dialog).getByRole("status");
    expect(live.textContent).toBe("");
    await userEvent.click(within(dialog).getByRole("button", { name: "Move house-rules down" }));
    expect(live).toHaveTextContent("house-rules is now 2nd of 3.");
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove acme-scanner" }));
    expect(live).toHaveTextContent("Removed acme-scanner.");
    await choose(within(dialog).getByRole("combobox", { name: "Add a guardrail" }), "acme-scanner");
    expect(live).toHaveTextContent("Added acme-scanner as 3rd of 3.");
    expect(live).toHaveAttribute("aria-live", "polite");
  });
});

describe("the route form", () => {
  const route: fixtures.Route = { ...fixtures.routes.support, guardrails: [refs.pii, refs.words] };

  function keepsRoutes() {
    const updated: RouteRequest[] = [];
    const created: RouteRequest[] = [];
    override("get", "/api/routes", () => ok("get", "/api/routes", 200, { routes: [route] }));
    override("get", "/api/routes/{id}", () => ok("get", "/api/routes/{id}", 200, route));
    override("put", "/api/routes/{id}", async ({ request }) => {
      updated.push((await request.json()) as RouteRequest);
      return ok("put", "/api/routes/{id}", 200, route);
    });
    override("post", "/api/routes", async ({ request }) => {
      created.push((await request.json()) as RouteRequest);
      return ok("post", "/api/routes", 201, route);
    });
    return { updated, created };
  }

  test("the list shows the guardrails of a route in order", async () => {
    keepsRoutes();
    await renderWithApp(null, { route: "/routes" });
    await table("Routes");
    const chips = within(rowWithCell("support-chat")).getByRole("group", { name: "Guardrails" });
    expect(chips).toHaveTextContent("mask-emailshouse-rules");
  });

  test("the form shows the chosen ones, and leaves them out of the request when they are unchanged", async () => {
    const { updated } = keepsRoutes();
    await renderWithApp(null, { route: "/routes/1" });
    const form = await screen.findByRole("form", { name: "Route" });
    expect(await chosenIn(form)).toEqual(["1.mask-emailsEvery call", "2.house-rules"]);
    await userEvent.click(screen.getByRole("button", { name: "Save route" }));
    await waitFor(() => {
      expect(updated).toHaveLength(1);
    });
    expect(updated[0]).not.toHaveProperty("guardrail_ids");
  });

  test("a change in the order is sent as the whole list", async () => {
    const { updated } = keepsRoutes();
    await renderWithApp(null, { route: "/routes/1" });
    const form = await screen.findByRole("form", { name: "Route" });
    await chosenIn(form);
    await userEvent.click(within(form).getByRole("button", { name: "Move house-rules up" }));
    await userEvent.click(screen.getByRole("button", { name: "Save route" }));
    await waitFor(() => {
      expect(updated).toHaveLength(1);
    });
    expect(updated[0]?.guardrail_ids).toEqual([2, 1]);
  });

  async function newRouteForm(): Promise<HTMLElement> {
    await renderWithApp(null, { route: "/routes/new" });
    const form = await screen.findByRole("form", { name: "Route" });
    await userEvent.click(within(form).getByLabelText("Name"));
    await userEvent.paste("chat");
    await userEvent.click(within(form).getByRole("button", { name: "Add primary target" }));
    await choose(within(form).getByRole("combobox", { name: "Model of primary target 1" }), "openai/gpt-4o-mini");
    return form;
  }

  test("a new route sends no guardrails when none are chosen", async () => {
    const { created } = keepsRoutes();
    const form = await newRouteForm();
    await userEvent.click(within(form).getByRole("button", { name: "Create route" }));
    await waitFor(() => {
      expect(created).toHaveLength(1);
    });
    expect(created[0]).not.toHaveProperty("guardrail_ids");
  });

  test("a new route sends the guardrails that are chosen", async () => {
    const { created } = keepsRoutes();
    const form = await newRouteForm();
    await choose(await within(form).findByRole("combobox", { name: "Add a guardrail" }), "acme-scanner");
    expect(await chosenIn(form)).toEqual(["1.acme-scanner"]);
    await userEvent.click(within(form).getByRole("button", { name: "Create route" }));
    await waitFor(() => {
      expect(created).toHaveLength(1);
    });
    expect(created[0]?.guardrail_ids).toEqual([3]);
  });
});

describe("the logs", () => {
  test("a call shows what the guardrails did at worst", async () => {
    override("get", "/api/logs", () =>
      ok("get", "/api/logs", 200, {
        logs: [fixtures.blockedLog, fixtures.redactedLog, fixtures.flaggedLog, fixtures.logs.answered],
      }),
    );
    await renderWithApp(null, { route: "/logs" });
    const list = await table("Request logs");
    const rows = [...list.querySelectorAll("tbody tr")];
    expect(rows).toHaveLength(4);
    expect(within(rows[0] as HTMLElement).getByText("Blocked")).toBeInTheDocument();
    expect(within(rows[1] as HTMLElement).getByText("Redacted")).toBeInTheDocument();
    expect(within(rows[2] as HTMLElement).getByText("Flagged")).toBeInTheDocument();
    expect(within(rows[3] as HTMLElement).queryByText(/Blocked|Redacted|Flagged/)).toBeNull();
  });

  test("the filter asks the gateway for one outcome, and Any asks for none", async () => {
    const queries: URLSearchParams[] = [];
    override("get", "/api/logs", ({ request }) => {
      queries.push(new URL(request.url).searchParams);
      return ok("get", "/api/logs", 200, { logs: [fixtures.redactedLog] });
    });
    await renderWithApp(null, { route: "/logs" });
    await table("Request logs");
    expect(queries.at(-1)?.has("guardrail")).toBe(false);
    await choose(screen.getByRole("combobox", { name: "Guardrails" }), "Redacted");
    await waitFor(() => {
      expect(queries.at(-1)?.get("guardrail")).toBe("redacted");
    });
    await choose(screen.getByRole("combobox", { name: "Guardrails" }), "Blocked");
    await waitFor(() => {
      expect(queries.at(-1)?.get("guardrail")).toBe("blocked");
    });
    await choose(screen.getByRole("combobox", { name: "Guardrails" }), "Any guardrail result");
    await waitFor(() => {
      expect(queries.at(-1)?.has("guardrail")).toBe(false);
    });
  });

  test("the call page lists what each direction was checked with and what it did, by names and counts", async () => {
    await renderWithApp(null, { route: `/logs/${String(fixtures.flaggedLog.id)}` });
    const list = await screen.findByRole("heading", { name: "Guardrails" });
    expect(list).toBeInTheDocument();
    const details = screen.getAllByRole("definition").map((d) => d.textContent);
    const input = details.find((text) => text.includes("Checked with house-rules, acme-scanner."));
    expect(input).toContain("Flagged: ticket.");
    expect(input).toContain("Could not be asked: timeout.");
    expectOneH1("Call");
  });

  test("a blocked call names the guardrail that blocked it", async () => {
    await renderWithApp(null, { route: `/logs/${String(fixtures.blockedLog.id)}` });
    await screen.findByRole("heading", { name: "Guardrails" });
    expect(document.body).toHaveTextContent("Blocked by house-rules.");
  });

  test("a call the guardrails did not touch has no section", async () => {
    await renderWithApp(null, { route: "/logs/5" });
    await screen.findByRole("heading", { name: "Routing attempts" });
    expect(screen.queryByRole("heading", { name: "Guardrails" })).toBeNull();
  });
});
