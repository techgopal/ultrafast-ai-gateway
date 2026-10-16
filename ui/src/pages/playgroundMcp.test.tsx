import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import { HttpResponse } from "msw";
import { ok, override } from "@/test/handlers";
import { counted, forgetToasts, installSelect, installPointerCapture } from "@/test/pages";
import { renderWithApp } from "@/test/render";

beforeAll(() => {
  installSelect();
  installPointerCapture();
});
afterEach(forgetToasts);

const { github } = fixtures.mcpServers;
const { createIssue } = fixtures.mcpTools;

function serve(servers = [github], tools = [createIssue]) {
  override("get", "/api/mcp/servers", () => ok("get", "/api/mcp/servers", 200, { servers }));
  override("get", "/api/mcp/servers/{id}/tools", () => ok("get", "/api/mcp/servers/{id}/tools", 200, { tools }));
  override("get", "/api/models", () => ok("get", "/api/models", 200, { models: fixtures.modelList }));
  override("get", "/api/routes", () => ok("get", "/api/routes", 200, { routes: fixtures.routeList }));
}

async function openTab() {
  await renderWithApp(null, { route: "/playground" });
  await userEvent.click(await screen.findByRole("button", { name: "MCP tools" }));
  return await screen.findByRole("form", { name: "MCP call" });
}

describe("the MCP tools tab of the playground", () => {
  test("runs the chosen tool with the typed arguments and shows the result", async () => {
    serve();
    const post = counted("post", "/api/playground/mcp/{server}", () =>
      ok("post", "/api/playground/mcp/{server}", 200, fixtures.playgroundMcpReply),
    );
    const form = await openTab();
    // The first server and its first tool are chosen; the schema's hints show.
    await waitFor(() => {
      expect(within(form).getByRole("combobox", { name: "MCP server" })).toHaveTextContent("github");
      expect(within(form).getByRole("combobox", { name: "MCP tool" })).toHaveTextContent("create_issue");
    });
    const args = within(form).getByLabelText("Arguments");
    await userEvent.clear(args);
    await userEvent.click(args);
    await userEvent.paste('{"title": "Bug"}');
    await userEvent.click(within(form).getByRole("button", { name: "Run tool" }));
    await waitFor(() => {
      expect(post.bodies).toEqual([
        {
          jsonrpc: "2.0",
          id: 1,
          method: "tools/call",
          params: { name: "create_issue", arguments: { title: "Bug" } },
        },
      ]);
    });
    const result = await screen.findByRole("region", { name: "Result" });
    expect(result).toHaveTextContent("mcp:github/create_issue");
    expect(result).toHaveTextContent("Opened issue 42.");
  });

  test("arguments that are not a JSON object are refused before any request", async () => {
    serve();
    const post = counted("post", "/api/playground/mcp/{server}", () =>
      ok("post", "/api/playground/mcp/{server}", 200, fixtures.playgroundMcpReply),
    );
    const form = await openTab();
    const args = within(form).getByLabelText("Arguments");
    await userEvent.clear(args);
    await userEvent.click(args);
    await userEvent.paste("[1, 2]");
    await userEvent.click(within(form).getByRole("button", { name: "Run tool" }));
    expect(await screen.findByText("The arguments must be a JSON object.")).toBeInTheDocument();
    expect(post.calls).toBe(0);
  });

  test("an error of the reply and a tool that reports one are shown", async () => {
    serve();
    override("post", "/api/playground/mcp/{server}", () =>
      ok("post", "/api/playground/mcp/{server}", 200, {
        jsonrpc: "2.0",
        id: 1,
        result: { content: [{ type: "text", text: "no such repo" }], isError: true } as unknown as Record<string, never>,
      }),
    );
    const form = await openTab();
    await userEvent.click(within(form).getByRole("button", { name: "Run tool" }));
    const result = await screen.findByRole("region", { name: "Result" });
    expect(result).toHaveTextContent("The tool reported an error.");
    expect(result).toHaveTextContent("no such repo");
    // A call the gateway would not let through is an `error` in the place of `result`.
    override("post", "/api/playground/mcp/{server}", () =>
      HttpResponse.json({ jsonrpc: "2.0", id: 1, error: { code: -32602, message: "Unknown tool" } }),
    );
    await userEvent.click(within(form).getByRole("button", { name: "Run tool" }));
    await waitFor(() => {
      expect(screen.getByRole("region", { name: "Result" })).toHaveTextContent("Unknown tool");
    });
  });

  test("a refusal of the gateway is told in words", async () => {
    serve();
    override("post", "/api/playground/mcp/{server}", () =>
      HttpResponse.json(
        { jsonrpc: "2.0", id: null, error: { code: -32000, message: "rate limit 'requests per minute' reached" } },
        { status: 429, headers: { "retry-after": "30" } },
      ),
    );
    const form = await openTab();
    await userEvent.click(within(form).getByRole("button", { name: "Run tool" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/rate limit|Try again in/i);
  });

  test("says so when there is no tool to call", async () => {
    serve([]);
    await renderWithApp(null, { route: "/playground" });
    await userEvent.click(await screen.findByRole("button", { name: "MCP tools" }));
    expect(await screen.findByText("No MCP tools")).toBeInTheDocument();
  });
});
