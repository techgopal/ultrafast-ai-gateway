import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import { ok, override } from "@/test/handlers";
import {
  counted,
  expectLabelsNameControls,
  expectOneH1,
  expectOneMain,
  forgetToasts,
  installSelect,
  settle,
  toasts,
} from "@/test/pages";
import { renderWithApp } from "@/test/render";

const { github } = fixtures.mcpServers;
const { search } = fixtures.mcpTools;

beforeAll(installSelect);
afterEach(forgetToasts);

function serve(servers = fixtures.mcpServerList, tools = fixtures.mcpToolList) {
  override("get", "/api/mcp/servers", () => ok("get", "/api/mcp/servers", 200, { servers }));
  override("get", "/api/mcp/servers/{id}/tools", (call) =>
    ok("get", "/api/mcp/servers/{id}/tools", 200, {
      tools: tools.filter((tool) => tool.server_id === Number(call.params.id)),
    }),
  );
}

async function serversTable(): Promise<HTMLElement> {
  const table = await screen.findByRole("table", { name: "MCP servers" });
  await waitFor(() => {
    expect(table).toHaveAttribute("aria-busy", "false");
  });
  return table;
}

async function toolsTable(name: string): Promise<HTMLElement> {
  const table = await screen.findByRole("table", { name });
  await waitFor(() => {
    expect(table).toHaveAttribute("aria-busy", "false");
  });
  return table;
}

function rowOf(table: HTMLElement, name: string): HTMLElement {
  // The first cell holds the name, and below it the description or the title.
  const row = [...table.querySelectorAll("tbody tr")].find((one) =>
    one.querySelector("td")?.textContent.startsWith(name),
  );
  if (!(row instanceof HTMLElement)) throw new Error(`no row for ${name}`);
  return row;
}

describe("MCP page for an admin", () => {
  test("lists the servers with their status and tool counts, and the tools of the first", async () => {
    serve();
    await renderWithApp(null, { route: "/mcp" });
    const table = await serversTable();
    expectOneMain();
    expectOneH1("MCP tools");
    expect(rowOf(table, "github")).toHaveTextContent("Synced");
    expect(rowOf(table, "docs")).toHaveTextContent("Sync failed");
    expect(rowOf(table, "docs")).toHaveTextContent("The server did not answer.");
    const tools = await toolsTable("Tools of github");
    expect(rowOf(tools, "create_issue")).toHaveTextContent("1 team");
    expect(rowOf(tools, "search")).toHaveTextContent("No one");
    expect(rowOf(tools, "old_tool")).toHaveTextContent("Removed by the server");
    expect(within(rowOf(tools, "old_tool")).getByRole("switch")).toBeDisabled();
    expect(within(rowOf(tools, "create_issue")).getByRole("switch")).toBeChecked();
    expect(within(rowOf(tools, "search")).getByRole("switch")).not.toBeChecked();
  });

  test("shows the tools of the server the admin picks", async () => {
    serve();
    await renderWithApp(null, { route: "/mcp" });
    const table = await serversTable();
    await userEvent.click(within(rowOf(table, "docs")).getByRole("button", { name: "Tools" }));
    expect(await screen.findByRole("heading", { name: "Tools of docs" })).toBeInTheDocument();
    expect(await screen.findByText("No tools")).toBeInTheDocument();
  });

  test("a switch turns a tool on", async () => {
    serve();
    const patch = counted("patch", "/api/mcp/tools/{id}", () =>
      ok("patch", "/api/mcp/tools/{id}", 200, { ...search, enabled: true }),
    );
    await renderWithApp(null, { route: "/mcp" });
    const tools = await toolsTable("Tools of github");
    await userEvent.click(within(rowOf(tools, "search")).getByRole("switch"));
    await waitFor(() => {
      expect(patch.bodies).toEqual([{ enabled: true }]);
    });
  });

  test("Sync says what changed", async () => {
    serve();
    const sync = counted("post", "/api/mcp/servers/{id}/sync", () =>
      ok("post", "/api/mcp/servers/{id}/sync", 200, fixtures.mcpSyncResult),
    );
    await renderWithApp(null, { route: "/mcp" });
    const table = await serversTable();
    await userEvent.click(within(rowOf(table, "github")).getByRole("button", { name: "Sync" }));
    await waitFor(() => {
      expect(toasts()).toContain("Synced: added 1, updated 1, removed 0, skipped 0.");
    });
    expect(sync.calls).toBe(1);
  });

  test("a server is added with its address and a bearer secret", async () => {
    serve([]);
    const post = counted("post", "/api/mcp/servers", () =>
      ok("post", "/api/mcp/servers", 201, github),
    );
    await renderWithApp(null, { route: "/mcp" });
    await screen.findByText("No MCP servers");
    await userEvent.click(screen.getByRole("button", { name: "Add server" }));
    const dialog = await screen.findByRole("dialog", { name: "Add MCP server" });
    expectLabelsNameControls(dialog);
    await userEvent.type(within(dialog).getByLabelText(/^Name/), "github");
    await userEvent.type(within(dialog).getByLabelText(/^Address/), "https://mcp.example.test/github");
    await userEvent.click(within(dialog).getByRole("radio", { name: "Bearer token" }));
    await userEvent.type(within(dialog).getByLabelText(/^Secret/), "tok-123");
    await userEvent.click(within(dialog).getByRole("button", { name: "Add server" }));
    await waitFor(() => {
      expect(post.bodies).toEqual([
        {
          name: "github",
          url: "https://mcp.example.test/github",
          description: "",
          auth_kind: "bearer",
          secret: "tok-123",
        },
      ]);
    });
    await waitFor(() => {
      expect(toasts()).toContain("Server added. Sync it to read its tools.");
    });
    // The secret is not left in the page.
    expect(document.body.textContent).not.toContain("tok-123");
  });

  test("changing the host of a server with a secret asks for the secret again, before any request", async () => {
    serve();
    const patch = counted("patch", "/api/mcp/servers/{id}", () =>
      ok("patch", "/api/mcp/servers/{id}", 200, github),
    );
    await renderWithApp(null, { route: "/mcp" });
    const table = await serversTable();
    await userEvent.click(within(rowOf(table, "github")).getByRole("button", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit MCP server" });
    const address = within(dialog).getByLabelText(/^Address/);
    await userEvent.clear(address);
    await userEvent.type(address, "https://other.example.test/mcp");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save server" }));
    expect(await within(dialog).findByText("Enter the secret again: the host changed.")).toBeInTheDocument();
    expect(patch.calls).toBe(0);
    await userEvent.type(within(dialog).getByLabelText(/^Secret/), "tok-456");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save server" }));
    await waitFor(() => {
      expect(patch.bodies).toEqual([{ url: "https://other.example.test/mcp", secret: "tok-456" }]);
    });
  });

  test("the access dialog replaces the grants of a tool", async () => {
    serve();
    override("get", "/api/teams", () => ok("get", "/api/teams", 200, { teams: fixtures.teamList }));
    const put = counted("put", "/api/mcp/tools/{id}/grants", () =>
      ok("put", "/api/mcp/tools/{id}/grants", 200, search),
    );
    await renderWithApp(null, { route: "/mcp" });
    const tools = await toolsTable("Tools of github");
    await userEvent.click(within(rowOf(tools, "search")).getByRole("button", { name: "Edit access" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit access" });
    await userEvent.click(await within(dialog).findByRole("checkbox", { name: fixtures.teams.platform.name }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Save access" }));
    await waitFor(() => {
      expect(put.bodies).toEqual([{ everyone: false, team_ids: [fixtures.teams.platform.id], user_ids: [] }]);
    });
    await waitFor(() => {
      expect(toasts()).toContain("Access updated.");
    });
  });

  test("an empty list says what to do", async () => {
    serve([]);
    await renderWithApp(null, { route: "/mcp" });
    expect(await screen.findByText("No MCP servers")).toBeInTheDocument();
  });
});

describe("MCP page for a member", () => {
  test("lists the tools they may call by server, read-only", async () => {
    serve(fixtures.mcpServersForMember, fixtures.mcpToolsForMember);
    await renderWithApp(null, { route: "/mcp", user: fixtures.me.lena });
    expect(await screen.findByRole("heading", { name: "github" })).toBeInTheDocument();
    const list = await screen.findByRole("list", { name: "Tools of github" });
    expect(within(list).getByText("create_issue")).toBeInTheDocument();
    expect(within(list).getByText("mcp:github/create_issue")).toBeInTheDocument();
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByRole("button", { name: /Sync|Edit|Add server/ })).toBeNull();
    await settle();
  });

  test("says so when nothing is available", async () => {
    serve([], []);
    await renderWithApp(null, { route: "/mcp", user: fixtures.me.lena });
    expect(await screen.findByText("No MCP tools are available to you yet. Ask an admin.")).toBeInTheDocument();
  });
});
