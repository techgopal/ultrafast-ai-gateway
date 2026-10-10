import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test } from "vitest";
import { noContent, override } from "@/test/handlers";
import { adminUser, memberUser, renderWithApp } from "@/test/render";

const sections = ["Observe", "Configure", "Govern"];
const items = [
  "Overview",
  "Logs",
  "Alerts",
  "Playground",
  "Providers",
  "Models",
  "Routing",
  "Prompts",
  "Virtual keys",
  "Users",
  "Teams",
  "Budgets and limits",
  "Guardrails",
  "MCP tools",
  "Settings",
  "Account",
];

function sidebar(): HTMLElement {
  return screen.getByRole("navigation", { name: "Main" });
}

describe("sidebar", () => {
  test("sidebar lists sections in order", async () => {
    await renderWithApp(null);
    const nav = sidebar();
    const headings = within(nav)
      .getAllByRole("heading")
      .map((el) => el.textContent);
    expect(headings).toEqual(sections);

    const texts = within(nav)
      .getAllByTestId("nav-item")
      .map((el) => el.getAttribute("data-label"));
    expect(texts).toEqual(items);

    // Sections and their items are in document order: Observe, its items, Configure, ...
    const order = within(nav)
      .getAllByTestId(/^nav-(item|section)$/)
      .map((el) => el.getAttribute("data-label") ?? el.textContent);
    expect(order).toEqual([
      "Observe",
      "Overview",
      "Logs",
      "Alerts",
      "Playground",
      "Configure",
      "Providers",
      "Models",
      "Routing",
      "Prompts",
      "Virtual keys",
      "Govern",
      "Users",
      "Teams",
      "Budgets and limits",
      "Guardrails",
      "MCP tools",
      "Settings",
      "Account",
    ]);
    for (const el of within(nav).getAllByTestId("nav-item")) {
      expect(el).toHaveTextContent(el.getAttribute("data-label") ?? "missing");
    }
  });

  test("links go where rule 5 says", async () => {
    await renderWithApp(null);
    const nav = sidebar();
    const hrefs = within(nav)
      .getAllByRole("link")
      .map((el) => [el.textContent, el.getAttribute("href")]);
    expect(hrefs).toEqual([
      ["Overview", "/"],
      ["Logs", "/logs"],
      ["Alerts", "/alerts"],
      ["Playground", "/playground"],
      ["Providers", "/providers"],
      ["Models", "/models"],
      ["Routing", "/routes"],
      ["Prompts", "/prompts"],
      ["Virtual keys", "/keys"],
      ["Users", "/users"],
      ["Teams", "/teams"],
      ["Budgets and limits", "/limits"],
      ["Guardrails", "/guardrails"],
      ["Settings", "/settings"],
      ["Account", "/account"],
    ]);
  });

  test("coming items are not links", async () => {
    await renderWithApp(null);
    const nav = sidebar();
    expect(within(nav).queryByRole("link", { name: /MCP tools/ })).toBeNull();
    const logs = within(nav)
      .getAllByTestId("nav-item")
      .find((el) => el.getAttribute("data-label") === "MCP tools");
    expect(logs).toBeDefined();
    expect(logs).not.toHaveAttribute("href");
    expect(logs).toHaveAttribute("aria-disabled", "true");
    expect(logs).toHaveTextContent("Coming");
    expect(logs?.tagName).not.toBe("A");
    expect(logs?.tagName).not.toBe("BUTTON");
    expect(logs?.querySelector("a, button")).toBeNull();
  });

  test("active item is marked", async () => {
    await renderWithApp(null, { route: "/keys" });
    const nav = sidebar();
    expect(
      within(nav).getByRole("link", { name: "Virtual keys" }),
    ).toHaveAttribute("aria-current", "page");
    const current = nav.querySelectorAll("[aria-current]");
    expect(current).toHaveLength(1);
  });

  test("settings is for admins, and budgets and limits is for everybody", async () => {
    const first = await renderWithApp(null, { user: memberUser });
    expect(within(sidebar()).queryByRole("link", { name: "Settings" })).toBeNull();
    expect(within(sidebar()).getByRole("link", { name: "Budgets and limits" })).toHaveAttribute(
      "href",
      "/limits",
    );
    first.unmount();
    await renderWithApp(null, { user: adminUser });
    expect(within(sidebar()).getByRole("link", { name: "Settings" })).toHaveAttribute(
      "href",
      "/settings",
    );
  });

  test("alerts is for admins", async () => {
    const first = await renderWithApp(null, { user: memberUser });
    expect(within(sidebar()).queryByRole("link", { name: "Alerts" })).toBeNull();
    first.unmount();
    await renderWithApp(null, { user: adminUser });
    expect(within(sidebar()).getByRole("link", { name: "Alerts" })).toHaveAttribute(
      "href",
      "/alerts",
    );
  });

  test("guardrails is for admins", async () => {
    const first = await renderWithApp(null, { user: memberUser });
    expect(within(sidebar()).queryByRole("link", { name: "Guardrails" })).toBeNull();
    first.unmount();
    await renderWithApp(null, { user: adminUser });
    expect(within(sidebar()).getByRole("link", { name: "Guardrails" })).toHaveAttribute(
      "href",
      "/guardrails",
    );
  });

  test("the audit log is no item of its own: it is on the settings page, for admins", async () => {
    const first = await renderWithApp(null, { user: memberUser });
    expect(within(sidebar()).queryByRole("link", { name: "Audit log" })).toBeNull();
    expect(within(sidebar()).queryByText("Audit log")).toBeNull();
    first.unmount();

    await renderWithApp(null, { user: adminUser });
    expect(within(sidebar()).queryByRole("link", { name: "Audit log" })).toBeNull();
    expect(within(sidebar()).getByRole("link", { name: "Settings" })).toHaveAttribute(
      "href",
      "/settings",
    );
  });

  test("footer shows the signed-in user and signs out", async () => {
    let signedOut = 0;
    override("post", "/api/auth/logout", () => {
      signedOut += 1;
      return noContent();
    });
    await renderWithApp(null, { user: memberUser });
    const nav = sidebar();
    expect(within(nav).getByText("Mel Member")).toBeInTheDocument();
    // The role as the badges of the console name it.
    expect(within(nav).getByText("Member")).toBeInTheDocument();
    expect(within(nav).queryByText("member")).toBeNull();
    await userEvent.click(within(nav).getByRole("button", { name: "Sign out" }));
    await waitFor(() => {
      expect(signedOut).toBe(1);
    });
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    });
  });
});

describe("the role in the footer", () => {
  test("an admin is named as the badges name the role", async () => {
    await renderWithApp(null, { user: adminUser });
    const nav = sidebar();
    expect(within(nav).getByText("Ada Admin")).toBeInTheDocument();
    expect(within(nav).getByText("Admin")).toBeInTheDocument();
    expect(within(nav).queryByText("admin")).toBeNull();
  });
});

describe("routes", () => {
  test("unknown route shows not found", async () => {
    await renderWithApp(null, { route: "/nope" });
    expect(
      screen.getByRole("heading", { name: "Page not found" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("link", { name: "Back to Overview" }),
    ).toHaveAttribute("href", "/");
  });
});

describe("narrow screens", () => {
  test("narrow screens use a drawer", async () => {
    const user = userEvent.setup();
    const { router } = await renderWithApp(null, { width: 390 });

    expect(screen.queryByRole("navigation", { name: "Main" })).toBeNull();
    // A link that only the sidebar has: the overview links to the virtual keys itself.
    expect(screen.queryByRole("link", { name: "Account" })).toBeNull();
    // The top bar shows the page title.
    const banner = screen.getByRole("banner");
    expect(within(banner).getByText("Overview")).toBeInTheDocument();

    const menu = within(banner).getByRole("button", { name: "Open menu" });
    await user.click(menu);
    const drawer = await screen.findByRole("dialog");
    expect(within(drawer).getByRole("navigation", { name: "Main" })).toBeVisible();
    // Focus moved into the drawer.
    expect(drawer.contains(document.activeElement)).toBe(true);

    // Choosing an item closes it and goes there.
    await user.click(within(drawer).getByRole("link", { name: "Virtual keys" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(router.state.location.pathname).toBe("/keys");
    await waitFor(() => {
      expect(menu).toHaveFocus();
    });

    // Escape closes it, and focus returns to the menu button.
    await user.click(menu);
    await screen.findByRole("dialog");
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    await waitFor(() => {
      expect(menu).toHaveFocus();
    });

    // Pressing the backdrop closes it.
    await user.click(menu);
    await screen.findByRole("dialog");
    const backdrop = document.querySelector('[data-slot="sheet-overlay"]');
    if (backdrop === null) throw new Error("no backdrop");
    await user.click(backdrop);
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    await waitFor(() => {
      expect(menu).toHaveFocus();
    });
  });
});

describe("wide screens", () => {
  test("wide screens show the sidebar without a menu button", async () => {
    await renderWithApp(null, { width: 1280 });
    expect(screen.getByRole("navigation", { name: "Main" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  test("a collapsed sidebar can be reopened on wide screens", async () => {
    const user = userEvent.setup();
    await renderWithApp(null, { width: 1280 });
    const state = () =>
      document.querySelector('[data-slot="sidebar"]')?.getAttribute("data-state");
    expect(state()).toBe("expanded");
    const toggle = screen.getByRole("button", { name: "Toggle sidebar" });

    // Collapsed with the keyboard shortcut: the control is still there to reopen it.
    await user.keyboard("{Control>}b{/Control}");
    expect(state()).toBe("collapsed");
    expect(toggle).toBeVisible();
    await user.click(toggle);
    expect(state()).toBe("expanded");

    await user.click(toggle);
    expect(state()).toBe("collapsed");
  });
});
