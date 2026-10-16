import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import { ok, override } from "@/test/handlers";
import { counted, forgetToasts, installSelect } from "@/test/pages";
import { renderWithApp } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

const github = fixtures.mcpServers.github;

function serve(servers = [github]) {
  override("get", "/api/mcp/servers", () => ok("get", "/api/mcp/servers", 200, { servers }));
  override("get", "/api/mcp/servers/{id}/tools", () => ok("get", "/api/mcp/servers/{id}/tools", 200, { tools: [] }));
}

describe("OAuth in the server dialog", () => {
  test("a server is added with a token endpoint, a client and a secret", async () => {
    serve([]);
    const post = counted("post", "/api/mcp/servers", () => ok("post", "/api/mcp/servers", 201, github));
    await renderWithApp(null, { route: "/mcp" });
    await screen.findByText("No MCP servers");
    await userEvent.click(screen.getByRole("button", { name: "Add server" }));
    const dialog = await screen.findByRole("dialog", { name: "Add MCP server" });
    await userEvent.type(within(dialog).getByLabelText(/^Name/), "api");
    await userEvent.type(within(dialog).getByLabelText(/^Address/), "https://mcp.example.test/api");
    await userEvent.click(within(dialog).getByRole("radio", { name: "OAuth (the gateway gets a token)" }));
    await userEvent.type(within(dialog).getByLabelText(/^Token URL/), "https://idp.example.test/token");
    await userEvent.type(within(dialog).getByLabelText(/^Client id/), "gateway");
    await userEvent.type(within(dialog).getByLabelText(/^Scopes/), "mcp:read");
    await userEvent.click(within(dialog).getByRole("radio", { name: "Token exchange (RFC 8693)" }));
    await userEvent.type(within(dialog).getByLabelText(/^Client secret/), "s3cret");
    await userEvent.click(within(dialog).getByRole("button", { name: "Add server" }));
    await waitFor(() => {
      expect(post.bodies).toEqual([
        {
          name: "api",
          url: "https://mcp.example.test/api",
          description: "",
          auth_kind: "oauth",
          oauth_token_url: "https://idp.example.test/token",
          oauth_client_id: "gateway",
          oauth_scopes: "mcp:read",
          oauth_grant: "token_exchange",
          secret: "s3cret",
        },
      ]);
    });
    expect(document.body.textContent).not.toContain("s3cret");
  });

  test("a missing token URL or client is said before any request", async () => {
    serve([]);
    const post = counted("post", "/api/mcp/servers", () => ok("post", "/api/mcp/servers", 201, github));
    await renderWithApp(null, { route: "/mcp" });
    await screen.findByText("No MCP servers");
    await userEvent.click(screen.getByRole("button", { name: "Add server" }));
    const dialog = await screen.findByRole("dialog", { name: "Add MCP server" });
    await userEvent.type(within(dialog).getByLabelText(/^Name/), "api");
    await userEvent.type(within(dialog).getByLabelText(/^Address/), "https://mcp.example.test/api");
    await userEvent.click(within(dialog).getByRole("radio", { name: "OAuth (the gateway gets a token)" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Add server" }));
    expect(await within(dialog).findByText("Enter the address of the token endpoint.")).toBeInTheDocument();
    expect(post.calls).toBe(0);
  });

  test("an OAuth server is edited with its fields filled in and no secret shown", async () => {
    const api = {
      ...github,
      name: "api",
      auth_kind: "oauth",
      oauth_token_url: "https://idp.example.test/token",
      oauth_client_id: "gw",
      oauth_scopes: "a",
      oauth_grant: "client_credentials",
    };
    serve([api as never]);
    const patch = counted("patch", "/api/mcp/servers/{id}", () => ok("patch", "/api/mcp/servers/{id}", 200, api));
    await renderWithApp(null, { route: "/mcp" });
    const table = await screen.findByRole("table", { name: "MCP servers" });
    await waitFor(() => {
      expect(table).toHaveAttribute("aria-busy", "false");
    });
    await userEvent.click(within(table).getByRole("button", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit MCP server" });
    expect(within(dialog).getByLabelText(/^Token URL/)).toHaveValue("https://idp.example.test/token");
    expect(within(dialog).getByLabelText(/^Client id/)).toHaveValue("gw");
    expect(within(dialog).getByLabelText(/^Client secret/)).toHaveValue("");
    await userEvent.type(within(dialog).getByLabelText(/^Scopes/), " b");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save server" }));
    await waitFor(() => {
      expect(patch.bodies).toEqual([{ oauth_scopes: "a b" }]);
    });
  });
});
