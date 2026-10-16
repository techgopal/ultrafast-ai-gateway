import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import type { components } from "@/api/schema";
import * as fixtures from "@/test/fixtures";
import { ok, override, refuse } from "@/test/handlers";
import {
  counted,
  expectLabelsNameControls,
  expectNotAvailable,
  expectOneH1,
  expectOneMain,
  expectSessionEndsOnPage,
  forgetToasts,
  installSelect,
  toasts,
} from "@/test/pages";
import { renderWithApp } from "@/test/render";
import { errors, validationFailed } from "@/test/errors";

type Schemas = components["schemas"];

beforeAll(installSelect);
afterEach(forgetToasts);

function serve(patch: Partial<Schemas["McpCatalogView"]> = {}) {
  override("get", "/api/mcp/catalog", () =>
    ok("get", "/api/mcp/catalog", 200, { ...fixtures.mcpCatalog, ...patch }),
  );
  override("get", "/api/mcp/servers", () =>
    ok("get", "/api/mcp/servers", 200, { servers: [] }),
  );
}

async function entries(): Promise<HTMLElement> {
  return await screen.findByRole("list", { name: "Catalog entries" });
}

function card(list: HTMLElement, name: string): HTMLElement {
  const item = within(list)
    .getAllByRole("heading", { level: 2 })
    .find((h) => h.textContent === name)
    ?.closest("li");
  if (!(item instanceof HTMLElement)) throw new Error(`no card for ${name}`);
  return item;
}

describe("the MCP catalog page", () => {
  test("is not available to a member, who also asks for nothing", async () => {
    const asked = counted("get", "/api/mcp/catalog", () =>
      refuse(errors.forbidden),
    );
    await renderWithApp(null, {
      route: "/mcp/catalog",
      user: fixtures.me.lena,
    });
    await expectNotAvailable();
    expectOneMain();
    await new Promise((r) => setTimeout(r, 50));
    expect(asked.calls).toBe(0);
  });

  test("lists the entries with their tags and where the copy came from", async () => {
    serve();
    await renderWithApp(null, { route: "/mcp/catalog" });
    const list = await entries();
    expectOneMain();
    expectOneH1("MCP catalog");
    expect(
      screen.getByText("Showing the copy that ships with this gateway."),
    ).toBeInTheDocument();
    expect(card(list, "GitHub")).toHaveTextContent(
      "Repositories, issues and pull requests.",
    );
    expect(
      within(card(list, "GitHub")).getByRole("list", {
        name: "Tags of GitHub",
      }),
    ).toHaveTextContent("codeissues");
    expect(card(list, "Filesystem")).toHaveTextContent(
      "Runs on the gateway host",
    );
  });

  test("a program that runs on the gateway host needs local servers", async () => {
    serve();
    await renderWithApp(null, { route: "/mcp/catalog" });
    const list = await entries();
    const files = card(list, "Filesystem");
    expect(
      within(files).getByRole("button", { name: "Add Filesystem" }),
    ).toBeDisabled();
    expect(files).toHaveTextContent(
      "Needs local servers: start the gateway with UF_MCP_LOCAL=on.",
    );
    expect(
      within(card(list, "GitHub")).getByRole("button", { name: "Add GitHub" }),
    ).toBeEnabled();
  });

  test("with local servers on the same entry can be added", async () => {
    serve({ local_enabled: true });
    await renderWithApp(null, { route: "/mcp/catalog" });
    const list = await entries();
    expect(
      within(card(list, "Filesystem")).getByRole("button", {
        name: "Add Filesystem",
      }),
    ).toBeEnabled();
  });

  test("the search narrows the entries by name, text and tag", async () => {
    serve();
    await renderWithApp(null, { route: "/mcp/catalog" });
    await entries();
    await userEvent.type(screen.getByLabelText("Search"), "chat");
    const list = await entries();
    expect(
      within(list)
        .getAllByRole("heading", { level: 2 })
        .map((h) => h.textContent),
    ).toEqual(["Slack"]);
    await userEvent.clear(screen.getByLabelText("Search"));
    await userEvent.type(screen.getByLabelText("Search"), "nothing like this");
    expect(await screen.findByText("No entries")).toBeInTheDocument();
  });

  test("a server is added with only the secrets the entry asks for", async () => {
    serve();
    const post = counted("post", "/api/mcp/servers/from-catalog", () =>
      ok(
        "post",
        "/api/mcp/servers/from-catalog",
        201,
        fixtures.mcpServers.github,
      ),
    );
    await renderWithApp(null, { route: "/mcp/catalog" });
    const list = await entries();
    await userEvent.click(
      within(card(list, "Slack")).getByRole("button", { name: "Add Slack" }),
    );
    const dialog = await screen.findByRole("dialog", { name: "Add Slack" });
    expectLabelsNameControls(dialog);
    // Only the entry's items, and the secret is a password field.
    expect(within(dialog).getByLabelText(/^SLACK_TOKEN/)).toHaveAttribute(
      "type",
      "password",
    );
    expect(within(dialog).getByLabelText(/^SLACK_TEAM/)).toHaveAttribute(
      "type",
      "text",
    );
    await userEvent.type(
      within(dialog).getByLabelText(/^SLACK_TOKEN/),
      "xoxb-123",
    );
    await userEvent.type(within(dialog).getByLabelText(/^SLACK_TEAM/), "T42");
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Add server" }),
    );
    await waitFor(() => {
      expect(post.bodies).toEqual([
        {
          catalog_id: "slack",
          secrets: { SLACK_TOKEN: "xoxb-123", SLACK_TEAM: "T42" },
        },
      ]);
    });
    await waitFor(() => {
      expect(toasts()).toContain(
        "Server added from the catalog. Its tools start off.",
      );
    });
    expect(document.body.textContent).not.toContain("xoxb-123");
  });

  test("a missing secret is refused before any request, on its field", async () => {
    serve();
    const post = counted("post", "/api/mcp/servers/from-catalog", () =>
      ok(
        "post",
        "/api/mcp/servers/from-catalog",
        201,
        fixtures.mcpServers.github,
      ),
    );
    await renderWithApp(null, { route: "/mcp/catalog" });
    const list = await entries();
    await userEvent.click(
      within(card(list, "GitHub")).getByRole("button", { name: "Add GitHub" }),
    );
    const dialog = await screen.findByRole("dialog", { name: "Add GitHub" });
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Add server" }),
    );
    expect(
      await within(dialog).findByText("Enter a value."),
    ).toBeInTheDocument();
    expect(post.calls).toBe(0);
  });

  test("a refusal of the gateway shows on the field it names, and the typed secret stays", async () => {
    serve();
    override("post", "/api/mcp/servers/from-catalog", () =>
      refuse(
        validationFailed({ "secrets.GITHUB_TOKEN": "A value is required." }),
      ),
    );
    await renderWithApp(null, { route: "/mcp/catalog" });
    const list = await entries();
    await userEvent.click(
      within(card(list, "GitHub")).getByRole("button", { name: "Add GitHub" }),
    );
    const dialog = await screen.findByRole("dialog", { name: "Add GitHub" });
    await userEvent.type(
      within(dialog).getByLabelText(/^GITHUB_TOKEN/),
      "ghp-secret",
    );
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Add server" }),
    );
    expect(
      await within(dialog).findByText("A value is required."),
    ).toBeInTheDocument();
    expect(within(dialog).getByLabelText(/^GITHUB_TOKEN/)).toHaveValue(
      "ghp-secret",
    );
  });

  test("Refresh asks the gateway to fetch the catalog again", async () => {
    serve();
    const post = counted("post", "/api/mcp/catalog/refresh", () =>
      ok("post", "/api/mcp/catalog/refresh", 200, fixtures.mcpCatalog),
    );
    await renderWithApp(null, { route: "/mcp/catalog" });
    await entries();
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => {
      expect(toasts()).toContain("Catalog refreshed.");
    });
    expect(post.calls).toBe(1);
  });

  test("an air-gapped gateway says so and cannot refresh", async () => {
    serve({ fetch_enabled: false });
    await renderWithApp(null, { route: "/mcp/catalog" });
    await entries();
    expect(
      screen.getByText(/Fetching the catalog is switched off/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
  });

  test("a catalog that failed to verify says why", async () => {
    serve({
      source: "cache",
      fetched_at: "2026-10-01T09:00:00Z",
      error: "catalog signature invalid, showing the copy from 2026-10-01",
    });
    await renderWithApp(null, { route: "/mcp/catalog" });
    await entries();
    expect(
      screen.getByText(
        "catalog signature invalid, showing the copy from 2026-10-01",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Showing the copy from 2026-10-01."),
    ).toBeInTheDocument();
  });

  test("the MCP page links to it", async () => {
    serve();
    await renderWithApp(null, { route: "/mcp" });
    const link = await screen.findByRole("link", { name: "Add from catalog" });
    expect(link).toHaveAttribute("href", "/mcp/catalog");
  });

  test("a session that ends takes the page to sign-in", async () => {
    serve();
    const app = await renderWithApp(null, { route: "/mcp/catalog" });
    await entries();
    await expectSessionEndsOnPage(app, {
      path: "/api/mcp/catalog",
      queryKey: ["mcp", "catalog"],
      at: "/mcp/catalog",
    });
  });
});
