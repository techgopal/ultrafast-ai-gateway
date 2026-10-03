import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { errors, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, noContent, ok, override, refuse } from "@/test/handlers";
import {
  aCallFindsTheSessionEnded,
  choose,
  counted,
  expectOneH1,
  expectOneMain,
  forgetToasts,
  held,
  href,
  installSelect,
  expectOneRequestWhileTheDialogStays,
  optionsOf,
  SESSION_ENDED,
  settle,
  toasts,
} from "@/test/pages";
import { renderWithApp, type AppRenderResult } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

const { platform } = fixtures.teams;

function page(options: { user?: fixtures.Me; width?: number } = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/limits", ...options });
}

async function table(name: string): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

/** The row of the table whose first cell says `name`. */
function rowIn(caption: "Limits" | "Budgets", name: string): HTMLElement {
  const found = within(screen.getByRole("table", { name: caption }))
    .getAllByRole("row")
    .find((row) => within(row).queryAllByRole("cell")[0]?.textContent === name);
  if (found === undefined) throw new Error(`no row says ${name}`);
  return found;
}

const button = (name: string) => screen.getByRole("button", { name });
const cellsOf = (row: HTMLElement) =>
  within(row)
    .getAllByRole("cell")
    .map((cell) => cell.textContent);

function paste(field: HTMLElement, text: string): Promise<void> {
  return userEvent.click(field).then(() => userEvent.paste(text));
}

async function openDialog(opener: string, title: string): Promise<HTMLElement> {
  await userEvent.click(button(opener));
  return screen.findByRole("dialog", { name: title });
}

describe("the page", () => {
  test("an admin sees both tables, with one main and one h1", async () => {
    await page();
    const limits = await table("Limits");
    const budgets = await table("Budgets");
    expectOneMain();
    expectOneH1("Budgets and limits");
    expect(screen.getByRole("heading", { level: 2, name: "Limits" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "Budgets" })).toBeInTheDocument();
    expect(within(limits).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Scope",
      "Requests/min",
      "Tokens/min",
      "Concurrent",
      "Actions",
    ]);
    expect(within(budgets).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Scope",
      "Period",
      "Amount",
      "Spent this period",
      "Action",
      "Actions",
    ]);
    expect(button("Set limit")).toBeInTheDocument();
    expect(button("Set budget")).toBeInTheDocument();
  });

  test("a limit shows its numbers, and what is not set says so", async () => {
    await page();
    await table("Limits");
    expect(cellsOf(rowIn("Limits", "team 'Platform'"))).toEqual([
      "team 'Platform'",
      "600",
      "1,000,000",
      "Not set",
      "EditDelete",
    ]);
    expect(cellsOf(rowIn("Limits", "gateway"))[1]).toBe("6,000");
  });

  test("a budget shows its period, amount, spend with a bar, and what it does", async () => {
    await page();
    await table("Budgets");
    const row = rowIn("Budgets", "gateway");
    expect(cellsOf(row)).toEqual([
      "gateway",
      "MonthlySince 2026-09-01",
      "$100.00",
      "$12.50 (12%)",
      "Blocks",
      "EditDelete",
    ]);
    const bar = within(row).getByRole("progressbar", { name: "Spent this period" });
    expect(bar).toHaveAttribute("value", "12500000");
    expect(bar).toHaveAttribute("max", "100000000");
    // An alert budget spent beyond its amount says so, and the bar is full.
    const over = rowIn("Budgets", "team 'Platform'");
    expect(cellsOf(over)[3]).toBe("$12.00 (120%)");
    expect(within(over).getByText("Alerts")).toBeInTheDocument();
    expect(within(rowIn("Budgets", "key 'platform-prod'")).getByText("Blocks")).toBeInTheDocument();
  });

  test("a member sees what applies to them, and no control", async () => {
    override("get", "/api/limits", () =>
      ok("get", "/api/limits", 200, { limits: fixtures.limitsForMember }),
    );
    override("get", "/api/budgets", () =>
      ok("get", "/api/budgets", 200, { budgets: fixtures.budgetsForMember }),
    );
    const users = counted("get", "/api/users", () => ok("get", "/api/users", 200, { users: [] }));
    await page({ user: fixtures.me.lena });
    await table("Limits");
    await table("Budgets");
    expectOneMain();
    expectOneH1("Budgets and limits");
    expect(screen.getByText("What applies to you.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Set |Edit|Delete/ })).toBeNull();
    expect(screen.queryByRole("columnheader", { name: "Actions" })).toBeNull();
    expect(screen.getAllByRole("row").length).toBeGreaterThan(2);
    expect(users.calls).toBe(0);
  });

  test("empty lists say so, differently for a member", async () => {
    override("get", "/api/limits", () => ok("get", "/api/limits", 200, { limits: [] }));
    override("get", "/api/budgets", () => ok("get", "/api/budgets", 200, { budgets: [] }));
    const first = await page();
    expect(await screen.findByText("No limits")).toBeInTheDocument();
    expect(screen.getByText("No budgets")).toBeInTheDocument();
    first.unmount();
    await page({ user: fixtures.me.lena });
    expect(await screen.findByText("No limits apply to you.")).toBeInTheDocument();
    expect(screen.getByText("No budget applies to you.")).toBeInTheDocument();
  });

  test("a section that fails shows the error with Retry; the other stays", async () => {
    override("get", "/api/limits", () => refuse(errors.internal_error));
    await page();
    await table("Budgets");
    expect(await screen.findByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    const asked = counted("get", "/api/limits", () =>
      ok("get", "/api/limits", 200, { limits: fixtures.limitList }),
    );
    await userEvent.click(button("Retry"));
    await table("Limits");
    expect(asked.calls).toBe(1);
  });

  test("a gateway that cannot be reached is said", async () => {
    override("get", "/api/budgets", networkFailure);
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
  });

  test("loading shows skeleton rows", async () => {
    const door = gate();
    override("get", "/api/limits", async () => {
      await door.opened;
      return ok("get", "/api/limits", 200, { limits: fixtures.limitList });
    });
    await page();
    const limits = screen.getByRole("table", { name: "Limits" });
    expect(limits).toHaveAttribute("aria-busy", "true");
    expect(limits.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expectOneH1("Budgets and limits");
    act(() => {
      door.open();
    });
    await table("Limits");
  });

  test("at width 390 the rows are cards and the controls are touchable", async () => {
    await page({ width: 390 });
    const cards = await screen.findByRole("list", { name: "Limits" });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    expect(screen.queryByRole("table")).toBeNull();
    const [first] = within(cards).getAllByRole("listitem");
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Scope",
      "Requests/min",
      "Tokens/min",
      "Concurrent",
    ]);
    for (const name of ["Set limit", "Set budget"]) {
      expect(button(name).className.split(/\s+/)).toContain("min-h-11");
    }
    for (const one of within(first).getAllByRole("button")) {
      expect(one.className.split(/\s+/)).toContain("min-h-11");
    }
  });
});

describe("setting a limit", () => {
  test("the gateway has no target; the numbers are sent, and the list is read again", async () => {
    const put = counted("put", "/api/limits", () => ok("put", "/api/limits", 200, fixtures.limits.gateway));
    const list = counted("get", "/api/limits", () =>
      ok("get", "/api/limits", 200, { limits: fixtures.limitList }),
    );
    await page();
    await table("Limits");
    const dialog = await openDialog("Set limit", "Set limit");
    expect(within(dialog).getByRole("combobox", { name: "Scope" })).toHaveTextContent("Gateway");
    expect(within(dialog).queryByRole("combobox", { name: "Team" })).toBeNull();
    await paste(within(dialog).getByLabelText("Requests per minute"), "120");
    await paste(within(dialog).getByLabelText("Concurrent requests"), "8");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save limit" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(put.bodies).toEqual([{ scope: "gateway", requests_per_minute: 120, concurrent: 8 }]);
    expect(toasts()).toEqual(["Limit saved."]);
    // The list was asked for again.
    await waitFor(() => {
      expect(list.calls).toBe(2);
    });
  });

  test("the scopes are offered in order, and each target is chosen from its own list", async () => {
    const put = counted("put", "/api/limits", () => ok("put", "/api/limits", 200, fixtures.limits.team));
    await page();
    await table("Limits");
    const dialog = await openDialog("Set limit", "Set limit");
    const scope = within(dialog).getByRole("combobox", { name: "Scope" });
    expect(await optionsOf(scope)).toEqual(["Gateway", "Team", "User", "Key"]);

    await choose(scope, "Team");
    const team = await within(dialog).findByRole("combobox", { name: "Team" });
    expect(await optionsOf(team)).toEqual(fixtures.teamList.map((t) => t.name));

    await choose(scope, "User");
    expect(within(dialog).queryByRole("combobox", { name: "Team" })).toBeNull();
    const user = await within(dialog).findByRole("combobox", { name: "User" });
    expect(await optionsOf(user)).toEqual(fixtures.userList.map((u) => u.email));

    await choose(scope, "Key");
    const key = await within(dialog).findByRole("combobox", { name: "Key" });
    // Revoked keys are not offered.
    const offered = fixtures.keyList.filter((k) => k.status !== "revoked").map((k) => k.name);
    expect(await optionsOf(key)).toEqual(offered);

    await choose(scope, "Team");
    await choose(await within(dialog).findByRole("combobox", { name: "Team" }), platform.name);
    await paste(within(dialog).getByLabelText("Tokens per minute"), "5000");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save limit" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(put.bodies).toEqual([{ scope: "team", scope_id: platform.id, tokens_per_minute: 5000 }]);
  });

  test("a target is required, and a limit too; nothing is sent", async () => {
    const put = counted("put", "/api/limits", () => ok("put", "/api/limits", 200, fixtures.limits.team));
    await page();
    await table("Limits");
    const dialog = await openDialog("Set limit", "Set limit");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save limit" }));
    expect(
      await within(dialog).findByText("Set at least one limit. To remove the limits, delete them."),
    ).toBeInTheDocument();
    await choose(within(dialog).getByRole("combobox", { name: "Scope" }), "Team");
    await paste(within(dialog).getByLabelText("Concurrent requests"), "2");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save limit" }));
    expect(await within(dialog).findByText("Choose a team.")).toBeInTheDocument();
    expect(within(dialog).getByRole("combobox", { name: "Team" })).toHaveAttribute("aria-invalid", "true");
    await choose(within(dialog).getByRole("combobox", { name: "Team" }), platform.name);
    await paste(within(dialog).getByLabelText("Requests per minute"), "0");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save limit" }));
    expect(
      await within(dialog).findByText("Enter a whole number from 1 to 1,000,000."),
    ).toBeInTheDocument();
    expect(put.calls).toBe(0);
  });

  test("a field error of the gateway is shown on its field, and the values stay", async () => {
    override("put", "/api/limits", () =>
      refuse(validationFailed({ scope_id: "does not exist" })),
    );
    await page();
    await table("Limits");
    const dialog = await openDialog("Set limit", "Set limit");
    await choose(within(dialog).getByRole("combobox", { name: "Scope" }), "User");
    await choose(await within(dialog).findByRole("combobox", { name: "User" }), fixtures.users.arjun.email);
    await paste(within(dialog).getByLabelText("Concurrent requests"), "3");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save limit" }));
    expect(await within(dialog).findByText("does not exist")).toBeInTheDocument();
    expect(within(dialog).getByRole("combobox", { name: "User" })).toHaveAttribute("aria-invalid", "true");
    expect(within(dialog).getByLabelText("Concurrent requests")).toHaveValue("3");
    expect(toasts()).toEqual([]);
  });

  test("the dialog stays while the request runs, sends once, and can be left after a refusal", async () => {
    const request = held("put", "/api/limits");
    await page();
    await table("Limits");
    const dialog = await openDialog("Set limit", "Set limit");
    const field = within(dialog).getByLabelText("Requests per minute");
    await paste(field, "10");
    await expectOneRequestWhileTheDialogStays(dialog, field, "Saving", request);
    expect(request.bodies).toEqual([{ scope: "gateway", requests_per_minute: 10 }]);
  });

  test("a second opening starts from an empty form", async () => {
    await page();
    await table("Limits");
    const first = await openDialog("Set limit", "Set limit");
    await paste(within(first).getByLabelText("Requests per minute"), "10");
    await userEvent.click(within(first).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    const second = await openDialog("Set limit", "Set limit");
    expect(within(second).getByLabelText("Requests per minute")).toHaveValue("");
  });

  test("edit keeps the target, starts from the numbers, and replaces all three", async () => {
    const put = counted("put", "/api/limits", () => ok("put", "/api/limits", 200, fixtures.limits.team));
    await page();
    await table("Limits");
    await userEvent.click(within(rowIn("Limits", "team 'Platform'")).getByRole("button", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit limit" });
    expect(within(dialog).queryByRole("combobox")).toBeNull();
    expect(within(dialog).getByText("team 'Platform'")).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Requests per minute")).toHaveValue("600");
    expect(within(dialog).getByLabelText("Tokens per minute")).toHaveValue("1000000");
    expect(within(dialog).getByLabelText("Concurrent requests")).toHaveValue("");
    await userEvent.clear(within(dialog).getByLabelText("Tokens per minute"));
    await userEvent.click(within(dialog).getByRole("button", { name: "Save limit" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(put.bodies).toEqual([{ scope: "team", scope_id: platform.id, requests_per_minute: 600 }]);
  });
});

describe("deleting", () => {
  test("a limit asks first, says what follows, and the list is read again", async () => {
    const removed = counted("delete", "/api/limits/{id}", () => noContent());
    const list = counted("get", "/api/limits", () =>
      ok("get", "/api/limits", 200, { limits: fixtures.limitList }),
    );
    await page();
    await table("Limits");
    await userEvent.click(within(rowIn("Limits", "key 'platform-prod'")).getByRole("button", { name: "Delete" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Calls are no longer limited by this.");
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(removed.calls).toBe(0);
    await userEvent.click(within(rowIn("Limits", "key 'platform-prod'")).getByRole("button", { name: "Delete" }));
    await userEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(removed.calls).toBe(1);
    });
    await waitFor(() => {
      expect(list.calls).toBe(2);
    });
    expect(toasts()).toEqual(["Limit deleted."]);
  });

  test("a refusal stays in the dialog", async () => {
    override("delete", "/api/limits/{id}", () => refuse(errors.internal_error));
    await page();
    await table("Limits");
    await userEvent.click(within(rowIn("Limits", "gateway")).getByRole("button", { name: "Delete" }));
    const dialog = await screen.findByRole("alertdialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(toasts()).toEqual([]);
  });
});

describe("setting a budget", () => {
  test("dollars become micros; the period and the action are sent", async () => {
    const put = counted("put", "/api/budgets", () => ok("put", "/api/budgets", 200, fixtures.budgets.gateway));
    const list = counted("get", "/api/budgets", () =>
      ok("get", "/api/budgets", 200, { budgets: fixtures.budgetList }),
    );
    await page();
    await table("Budgets");
    const dialog = await openDialog("Set budget", "Set budget");
    await choose(within(dialog).getByRole("combobox", { name: "Scope" }), "Key");
    await choose(
      await within(dialog).findByRole("combobox", { name: "Key" }),
      fixtures.keys.active.name,
    );
    await paste(within(dialog).getByLabelText("Amount (USD)"), "12.345678");
    expect(await optionsOf(within(dialog).getByRole("combobox", { name: "Period" }))).toEqual([
      "Daily",
      "Weekly",
      "Monthly",
    ]);
    await choose(within(dialog).getByRole("combobox", { name: "Period" }), "Weekly");
    await userEvent.click(within(dialog).getByRole("radio", { name: "Alerts" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Save budget" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(put.bodies).toEqual([
      {
        scope: "key",
        scope_id: fixtures.keys.active.id,
        amount_micros: 12_345_678,
        period: "weekly",
        action: "alert",
      },
    ]);
    expect(toasts()).toEqual(["Budget saved."]);
    await waitFor(() => {
      expect(list.calls).toBe(2);
    });
  });

  test("the form starts with a monthly budget that blocks", async () => {
    await page();
    await table("Budgets");
    const dialog = await openDialog("Set budget", "Set budget");
    expect(within(dialog).getByRole("combobox", { name: "Period" })).toHaveTextContent("Monthly");
    expect(within(dialog).getByRole("radio", { name: "Blocks" })).toBeChecked();
    expect(within(dialog).getByLabelText("Amount (USD)")).toHaveValue("");
  });

  test.each(["", "0", "-5", "1.2345678", "abc"])("an amount of %j is refused on the amount", async (amount) => {
    const put = counted("put", "/api/budgets", () => ok("put", "/api/budgets", 200, fixtures.budgets.gateway));
    await page();
    await table("Budgets");
    const dialog = await openDialog("Set budget", "Set budget");
    if (amount !== "") await paste(within(dialog).getByLabelText("Amount (USD)"), amount);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save budget" }));
    expect(
      await within(dialog).findByText("Enter dollars above 0, with up to 6 decimals, up to $1,000,000,000."),
    ).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Amount (USD)")).toHaveAttribute("aria-invalid", "true");
    expect(put.calls).toBe(0);
  });

  test("the gateway's refusal of the amount is shown on the amount, in dollars", async () => {
    override("put", "/api/budgets", () =>
      refuse(validationFailed({ amount_micros: "must be from 1 to 1000000000000000" })),
    );
    await page();
    await table("Budgets");
    const dialog = await openDialog("Set budget", "Set budget");
    await paste(within(dialog).getByLabelText("Amount (USD)"), "5");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save budget" }));
    expect(
      await within(dialog).findByText("Enter dollars above 0, with up to 6 decimals, up to $1,000,000,000."),
    ).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Amount (USD)")).toHaveValue("5");
  });

  test("the dialog stays while the request runs", async () => {
    const request = held("put", "/api/budgets");
    await page();
    await table("Budgets");
    const dialog = await openDialog("Set budget", "Set budget");
    const field = within(dialog).getByLabelText("Amount (USD)");
    await paste(field, "5");
    await expectOneRequestWhileTheDialogStays(dialog, field, "Saving", request);
  });

  test("edit starts from the budget, its amount in dollars", async () => {
    const put = counted("put", "/api/budgets", () => ok("put", "/api/budgets", 200, fixtures.budgets.key));
    await page();
    await table("Budgets");
    await userEvent.click(within(rowIn("Budgets", "key 'platform-prod'")).getByRole("button", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit budget" });
    expect(within(dialog).getByLabelText("Amount (USD)")).toHaveValue("2.5");
    // The period is the budget's own, as its target is: the gateway keeps one budget for each.
    expect(within(dialog).queryByRole("combobox", { name: "Period" })).toBeNull();
    expect(within(dialog).getByText("Daily")).toBeInTheDocument();
    expect(within(dialog).getByRole("radio", { name: "Blocks" })).toBeChecked();
    await userEvent.click(within(dialog).getByRole("button", { name: "Save budget" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(put.bodies).toEqual([
      {
        scope: "key",
        scope_id: fixtures.keys.active.id,
        amount_micros: 2_500_000,
        period: "daily",
        action: "block",
      },
    ]);
  });

  test("delete says that spend is no longer capped", async () => {
    const removed = counted("delete", "/api/budgets/{id}", () => noContent());
    await page();
    await table("Budgets");
    await userEvent.click(within(rowIn("Budgets", "key 'platform-prod'")).getByRole("button", { name: "Delete" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Spend is no longer capped by this budget.");
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(removed.calls).toBe(1);
    });
    expect(toasts()).toEqual(["Budget deleted."]);
    await settle();
  });
});

describe("the scope", () => {
  test("a team that was chosen is no target of the user scope", async () => {
    const put = counted("put", "/api/limits", () => ok("put", "/api/limits", 200, fixtures.limits.team));
    await page();
    await table("Limits");
    const dialog = await openDialog("Set limit", "Set limit");
    const scope = within(dialog).getByRole("combobox", { name: "Scope" });
    await choose(scope, "Team");
    await choose(await within(dialog).findByRole("combobox", { name: "Team" }), platform.name);
    await choose(scope, "User");
    const user = await within(dialog).findByRole("combobox", { name: "User" });
    expect(user).toHaveTextContent("Choose a user");
    await paste(within(dialog).getByLabelText("Concurrent requests"), "2");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save limit" }));
    expect(await within(dialog).findByText("Choose a user.")).toBeInTheDocument();
    expect(put.calls).toBe(0);
  });
});

describe("at width 390: the budgets", () => {
  test("a row is a card with its labels, the bar and touchable controls", async () => {
    await page({ width: 390 });
    const cards = await screen.findByRole("list", { name: "Budgets" });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(fixtures.budgetList.length);
    const [first] = items;
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Scope",
      "Period",
      "Amount",
      "Spent this period",
      "Action",
    ]);
    expect(within(first).getByRole("progressbar", { name: "Spent this period" })).toBeInTheDocument();
    for (const one of within(first).getAllByRole("button")) {
      expect(one.className.split(/\s+/)).toContain("min-h-11");
    }
    const dialog = await openDialog("Set budget", "Set budget");
    for (const field of [
      within(dialog).getByLabelText("Amount (USD)"),
      within(dialog).getByRole("combobox", { name: "Period" }),
      within(dialog).getByRole("combobox", { name: "Scope" }),
    ]) {
      expect(field.className.split(/\s+/)).toContain("min-h-11");
    }
  });
});

describe("the forms as they are used", () => {
  test("a limit is typed, key by key, and sent", async () => {
    const put = counted("put", "/api/limits", () => ok("put", "/api/limits", 200, fixtures.limits.gateway));
    await page();
    await table("Limits");
    const dialog = await openDialog("Set limit", "Set limit");
    await userEvent.type(within(dialog).getByLabelText("Requests per minute"), "250");
    await userEvent.type(within(dialog).getByLabelText("Tokens per minute"), "9000{Enter}");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(put.bodies).toEqual([{ scope: "gateway", requests_per_minute: 250, tokens_per_minute: 9000 }]);
  });

  test("a budget is typed, key by key, and sent; an amount may start or end with the point", async () => {
    const put = counted("put", "/api/budgets", () => ok("put", "/api/budgets", 200, fixtures.budgets.gateway));
    await page();
    await table("Budgets");
    const dialog = await openDialog("Set budget", "Set budget");
    await userEvent.type(within(dialog).getByLabelText("Amount (USD)"), ".5{Enter}");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(put.bodies).toEqual([
      { scope: "gateway", amount_micros: 500_000, period: "monthly", action: "block" },
    ]);
  });

  test.each([
    ["Set limit", "Requests per minute", "put", "/api/limits"],
    ["Set budget", "Amount (USD)", "put", "/api/budgets"],
  ] as const)("%s: an answer for a session that is over says nothing", async (opener, field, method, path) => {
    startGateway({ signedIn: true });
    const door = gate();
    override(method, path, async () => {
      await door.opened;
      return path === "/api/limits"
        ? ok("put", "/api/limits", 200, fixtures.limits.gateway)
        : ok("put", "/api/budgets", 200, fixtures.budgets.gateway);
    });
    const app = await page();
    await table("Limits");
    const dialog = await openDialog(opener, opener);
    await paste(within(dialog).getByLabelText(field), "5");
    await userEvent.click(within(dialog).getByRole("button", { name: /^Save/ }));
    await aCallFindsTheSessionEnded("/api/providers");
    door.open();
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/limits")}`);
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
  });
});
