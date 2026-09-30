import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test } from "vitest";
import { noContent, override } from "@/test/handlers";
import { adminUser, memberUser, renderWithApp } from "@/test/render";

const sections = ["Observe", "Configure", "Govern"];
const items = [
  "Overview",
  "Logs",
  "Playground",
  "Providers",
  "Models",
  "Routing",
  "Virtual keys",
  "Users",
  "Teams",
  "Budgets and limits",
  "Guardrails",
  "MCP tools",
  "Audit log",
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
      "Playground",
      "Configure",
      "Providers",
      "Models",
      "Routing",
      "Virtual keys",
      "Govern",
      "Users",
      "Teams",
      "Budgets and limits",
      "Guardrails",
      "MCP tools",
      "Audit log",
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
      ["Providers", "/providers"],
      ["Virtual keys", "/keys"],
      ["Users", "/users"],
      ["Teams", "/teams"],
      ["Audit log", "/audit"],
      ["Account", "/account"],
    ]);
  });

  test("coming items are not links", async () => {
    await renderWithApp(null);
    const nav = sidebar();
    expect(within(nav).queryByRole("link", { name: /Logs/ })).toBeNull();
    const logs = within(nav)
      .getAllByTestId("nav-item")
      .find((el) => el.getAttribute("data-label") === "Logs");
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

  test("audit item is for admins", async () => {
    const first = await renderWithApp(null, { user: memberUser });
    expect(
      within(sidebar()).queryByRole("link", { name: "Audit log" }),
    ).toBeNull();
    expect(within(sidebar()).queryByText("Audit log")).toBeNull();
    first.unmount();

    await renderWithApp(null, { user: adminUser });
    expect(
      within(sidebar()).getByRole("link", { name: "Audit log" }),
    ).toHaveAttribute("href", "/audit");
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
    expect(within(nav).getByText("member")).toBeInTheDocument();
    await userEvent.click(within(nav).getByRole("button", { name: "Sign out" }));
    await waitFor(() => {
      expect(signedOut).toBe(1);
    });
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    });
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
