import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import { ok, override } from "@/test/handlers";
import { counted, expectLabelsNameControls, forgetToasts, installSelect, toasts } from "@/test/pages";
import { renderWithApp } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

const { github } = fixtures.mcpServers;
const files = fixtures.mcpLocalServer;

function serve(opts: { local?: boolean; servers?: (typeof files)[]; stderr?: string } = {}) {
  const servers = opts.servers ?? [github, files];
  override("get", "/api/mcp/servers", () => ok("get", "/api/mcp/servers", 200, { servers }));
  override("get", "/api/mcp/servers/{id}/tools", () => ok("get", "/api/mcp/servers/{id}/tools", 200, { tools: [] }));
  override("get", "/api/mcp/catalog", () =>
    ok("get", "/api/mcp/catalog", 200, { ...fixtures.mcpCatalog, local_enabled: opts.local ?? true }),
  );
  override("get", "/api/mcp/servers/{id}/stderr", () =>
    ok("get", "/api/mcp/servers/{id}/stderr", 200, { text: opts.stderr ?? "", supervised: true }),
  );
}

async function serversTable(): Promise<HTMLElement> {
  const table = await screen.findByRole("table", { name: "MCP servers" });
  await waitFor(() => {
    expect(table).toHaveAttribute("aria-busy", "false");
  });
  return table;
}

function rowOf(table: HTMLElement, name: string): HTMLElement {
  const row = [...table.querySelectorAll("tbody tr")].find((one) =>
    one.querySelector("td")?.textContent.startsWith(name),
  );
  if (!(row instanceof HTMLElement)) throw new Error(`no row for ${name}`);
  return row;
}

describe("local MCP servers in the console", () => {
  test("a program shows its command, and how it stands", async () => {
    serve({ servers: [github, { ...files, local: { phase: "crashlooping", restarts: 5 } }] });
    await renderWithApp(null, { route: "/mcp" });
    const table = await serversTable();
    expect(rowOf(table, "files")).toHaveTextContent("npx -y files-mcp");
    expect(rowOf(table, "files")).toHaveTextContent("Crash-looping (5 restarts)");
    // Only a program has output to read.
    expect(within(rowOf(table, "github")).queryByRole("button", { name: "Output" })).toBeNull();
  });

  test("the output of a program is read from the gateway", async () => {
    serve({ stderr: "starting up\nlistening" });
    await renderWithApp(null, { route: "/mcp" });
    const table = await serversTable();
    await userEvent.click(within(rowOf(table, "files")).getByRole("button", { name: "Output" }));
    const box = await screen.findByRole("textbox", { name: /Output of files/ });
    await waitFor(() => {
      expect(box).toHaveValue("starting up\nlistening");
    });
    await userEvent.click(screen.getByRole("button", { name: "Close output" }));
    expect(screen.queryByRole("textbox", { name: /Output of files/ })).toBeNull();
  });

  test("a program is added with its arguments and environment", async () => {
    serve({ servers: [] });
    const post = counted("post", "/api/mcp/servers", () => ok("post", "/api/mcp/servers", 201, files));
    await renderWithApp(null, { route: "/mcp" });
    await screen.findByText("No MCP servers");
    await userEvent.click(screen.getByRole("button", { name: "Add server" }));
    const dialog = await screen.findByRole("dialog", { name: "Add MCP server" });
    await userEvent.click(await within(dialog).findByRole("radio", { name: "Program on the gateway host" }));
    expectLabelsNameControls(dialog);
    // The address and the authentication are gone.
    expect(within(dialog).queryByLabelText(/^Address/)).toBeNull();
    await userEvent.type(within(dialog).getByLabelText(/^Name/), "files");
    await userEvent.type(within(dialog).getByLabelText(/^Command/), "npx");
    await userEvent.type(within(dialog).getByLabelText(/^Arguments/), "-y{Enter}files-mcp");
    await userEvent.click(within(dialog).getByRole("button", { name: "Add variable" }));
    await userEvent.type(within(dialog).getByLabelText("Variable 1 name"), "API_TOKEN");
    const value = within(dialog).getByLabelText("Variable 1 value");
    expect(value).toHaveAttribute("type", "password");
    await userEvent.type(value, "tok-1");
    await userEvent.click(within(dialog).getByRole("switch", { name: "Variable 1 is secret" }));
    expect(value).toHaveAttribute("type", "text");
    await userEvent.click(within(dialog).getByRole("button", { name: "Add server" }));
    await waitFor(() => {
      expect(post.bodies).toEqual([
        {
          name: "files",
          transport: "stdio",
          command: "npx",
          args: ["-y", "files-mcp"],
          description: "",
          env: { API_TOKEN: "tok-1" },
        },
      ]);
    });
    await waitFor(() => {
      expect(toasts()).toContain("Server added. Sync it to read its tools.");
    });
    expect(document.body.textContent).not.toContain("tok-1");
  });

  test("a program cannot be added while local servers are off", async () => {
    serve({ servers: [], local: false });
    await renderWithApp(null, { route: "/mcp" });
    await screen.findByText("No MCP servers");
    await userEvent.click(screen.getByRole("button", { name: "Add server" }));
    const dialog = await screen.findByRole("dialog", { name: "Add MCP server" });
    expect(within(dialog).queryByRole("radio", { name: "Program on the gateway host" })).toBeNull();
    expect(within(dialog).getByLabelText(/^Address/)).toBeInTheDocument();
  });

  test("an empty command or a variable without a value is refused before any request", async () => {
    serve({ servers: [] });
    const post = counted("post", "/api/mcp/servers", () => ok("post", "/api/mcp/servers", 201, files));
    await renderWithApp(null, { route: "/mcp" });
    await screen.findByText("No MCP servers");
    await userEvent.click(screen.getByRole("button", { name: "Add server" }));
    const dialog = await screen.findByRole("dialog", { name: "Add MCP server" });
    await userEvent.click(await within(dialog).findByRole("radio", { name: "Program on the gateway host" }));
    await userEvent.type(within(dialog).getByLabelText(/^Name/), "files");
    await userEvent.click(within(dialog).getByRole("button", { name: "Add server" }));
    expect(await within(dialog).findByText("Enter the program to run.")).toBeInTheDocument();
    await userEvent.type(within(dialog).getByLabelText(/^Command/), "npx");
    await userEvent.click(within(dialog).getByRole("button", { name: "Add variable" }));
    await userEvent.type(within(dialog).getByLabelText("Variable 1 name"), "A");
    await userEvent.click(within(dialog).getByRole("button", { name: "Add server" }));
    expect(await within(dialog).findByText("Enter a value.")).toBeInTheDocument();
    expect(post.calls).toBe(0);
  });

  test("editing a program sends only what changed and keeps stored values", async () => {
    serve();
    const patch = counted("patch", "/api/mcp/servers/{id}", () => ok("patch", "/api/mcp/servers/{id}", 200, files));
    await renderWithApp(null, { route: "/mcp" });
    const table = await serversTable();
    await userEvent.click(within(rowOf(table, "files")).getByRole("button", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit MCP server" });
    expect(within(dialog).getByLabelText(/^Command/)).toHaveValue("npx");
    expect(within(dialog).getByLabelText("Variable 1 name")).toHaveValue("API_TOKEN");
    expect(within(dialog).getByLabelText("Variable 1 value")).toHaveValue("");
    const command = within(dialog).getByLabelText(/^Command/);
    await userEvent.clear(command);
    await userEvent.type(command, "uvx");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save server" }));
    await waitFor(() => {
      expect(patch.bodies).toEqual([{ command: "uvx" }]);
    });
  });

  test("a new value for a stored variable is sent, and a removed variable is left out", async () => {
    serve();
    const patch = counted("patch", "/api/mcp/servers/{id}", () => ok("patch", "/api/mcp/servers/{id}", 200, files));
    await renderWithApp(null, { route: "/mcp" });
    const table = await serversTable();
    await userEvent.click(within(rowOf(table, "files")).getByRole("button", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit MCP server" });
    await userEvent.type(within(dialog).getByLabelText("Variable 1 value"), "new-value");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save server" }));
    await waitFor(() => {
      expect(patch.bodies).toEqual([{ env: { API_TOKEN: "new-value" } }]);
    });
  });
});
