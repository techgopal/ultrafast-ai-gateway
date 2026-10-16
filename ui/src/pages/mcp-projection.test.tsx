import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { rememberResult } from "@/lib/mcp";
import * as fixtures from "@/test/fixtures";
import { ok, override } from "@/test/handlers";
import { counted, forgetToasts, installSelect, toasts } from "@/test/pages";
import { renderWithApp } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

const { github } = fixtures.mcpServers;
const { createIssue } = fixtures.mcpTools;

function serve() {
  override("get", "/api/mcp/servers", () => ok("get", "/api/mcp/servers", 200, { servers: [github] }));
  override("get", "/api/mcp/servers/{id}/tools", () =>
    ok("get", "/api/mcp/servers/{id}/tools", 200, { tools: [createIssue] }),
  );
}

async function openDialog() {
  await renderWithApp(null, { route: "/mcp" });
  const tools = await screen.findByRole("table", { name: "Tools of github" });
  await waitFor(() => {
    expect(tools).toHaveAttribute("aria-busy", "false");
  });
  await userEvent.click(within(tools).getByRole("button", { name: "Edit projection" }));
  return await screen.findByRole("dialog", { name: "Edit projection" });
}

describe("the projection of a tool", () => {
  test("is saved as typed: queries a line each, limits as numbers", async () => {
    serve();
    const put = counted("put", "/api/mcp/tools/{id}/projection", () =>
      ok("put", "/api/mcp/tools/{id}/projection", 200, createIssue),
    );
    const dialog = await openDialog();
    await userEvent.click(within(dialog).getByLabelText("Keep"));
    await userEvent.paste("$.items[*].id\n\n $.total ");
    await userEvent.click(within(dialog).getByLabelText("Drop"));
    await userEvent.paste("$..body");
    await userEvent.type(within(dialog).getByLabelText("Most items in a list"), "5");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save projection" }));
    await waitFor(() => {
      expect(put.bodies).toEqual([
        { keep: ["$.items[*].id", "$.total"], drop: ["$..body"], max_items: 5, max_bytes: null },
      ]);
    });
    await waitFor(() => {
      expect(toasts()).toContain("Projection saved.");
    });
  });

  test("a limit that is not a number is refused before any request", async () => {
    serve();
    const put = counted("put", "/api/mcp/tools/{id}/projection", () =>
      ok("put", "/api/mcp/tools/{id}/projection", 200, createIssue),
    );
    const dialog = await openDialog();
    await userEvent.type(within(dialog).getByLabelText("Most bytes of text"), "lots");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save projection" }));
    expect(await within(dialog).findByText("Enter a whole number, or leave it empty.")).toBeInTheDocument();
    expect(put.calls).toBe(0);
  });

  test("is tried on the last result of the playground before it is saved", async () => {
    serve();
    rememberResult("github", "create_issue", { content: [{ type: "text", text: '{"id":1,"body":"x"}' }] });
    const preview = counted("post", "/api/mcp/tools/{id}/projection/preview", () =>
      ok("post", "/api/mcp/tools/{id}/projection/preview", 200, fixtures.mcpProjectionPreview),
    );
    const dialog = await openDialog();
    expect(within(dialog).getByLabelText("Sample result")).toHaveValue(
      JSON.stringify({ content: [{ type: "text", text: '{"id":1,"body":"x"}' }] }, null, 2),
    );
    await userEvent.click(within(dialog).getByLabelText("Keep"));
    await userEvent.paste("$.id");
    await userEvent.click(within(dialog).getByRole("button", { name: "Try it on the sample" }));
    const shown = await within(dialog).findByRole("region", { name: "Preview" });
    expect(shown).toHaveTextContent("120 bytes become 40.");
    expect(shown).toHaveTextContent('{"id":1}');
    expect(preview.bodies).toEqual([
      {
        result: { content: [{ type: "text", text: '{"id":1,"body":"x"}' }] },
        projection: { keep: ["$.id"], drop: [], max_items: null, max_bytes: null },
      },
    ]);
  });

  test("a sample that is not a result object is said, and nothing is sent", async () => {
    serve();
    const preview = counted("post", "/api/mcp/tools/{id}/projection/preview", () =>
      ok("post", "/api/mcp/tools/{id}/projection/preview", 200, fixtures.mcpProjectionPreview),
    );
    const dialog = await openDialog();
    await userEvent.clear(within(dialog).getByLabelText("Sample result"));
    await userEvent.click(within(dialog).getByLabelText("Sample result"));
    await userEvent.paste("[1]");
    await userEvent.click(within(dialog).getByRole("button", { name: "Try it on the sample" }));
    expect(await within(dialog).findByText(/must be a JSON object/)).toBeInTheDocument();
    expect(preview.calls).toBe(0);
  });
});
