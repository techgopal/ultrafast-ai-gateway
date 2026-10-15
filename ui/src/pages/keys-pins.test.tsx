import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import { ok, override } from "@/test/handlers";
import { counted, forgetToasts, installSelect, rowWithCell, toasts } from "@/test/pages";
import { memberUser, renderWithApp } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

const { active } = fixtures.keys;
const { github } = fixtures.mcpServers;
const { createIssue } = fixtures.mcpTools;

function serve(pinned: string[] = []) {
  override("get", "/api/keys", () =>
    ok("get", "/api/keys", 200, { keys: [{ ...active, pinned_tools: pinned }] }),
  );
  override("get", "/api/mcp/servers", () => ok("get", "/api/mcp/servers", 200, { servers: [github] }));
  override("get", "/api/mcp/servers/{id}/tools", () =>
    ok("get", "/api/mcp/servers/{id}/tools", 200, { tools: [createIssue] }),
  );
}

async function openPins() {
  await renderWithApp(null, { route: "/keys" });
  const table = await screen.findByRole("table", { name: "Virtual keys" });
  await waitFor(() => {
    expect(table).toHaveAttribute("aria-busy", "false");
  });
  const row = rowWithCell(active.name);
  await userEvent.click(within(row).getByRole("button", { name: "Edit pinned tools" }));
  return await screen.findByRole("dialog", { name: "Edit pinned tools" });
}

describe("pinned tools of a key", () => {
  test("an admin pins a tool the key can call, and what is sent replaces the pins", async () => {
    serve();
    const patch = counted("patch", "/api/keys/{id}", () =>
      ok("patch", "/api/keys/{id}", 200, { ...active, pinned_tools: ["mcp:github/create_issue"] }),
    );
    const dialog = await openPins();
    await userEvent.click(await within(dialog).findByRole("checkbox", { name: "mcp:github/create_issue" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(patch.bodies).toEqual([{ pinned_tools: ["mcp:github/create_issue"] }]);
    });
    await waitFor(() => {
      expect(toasts()).toContain("Pinned tools saved.");
    });
  });

  test("a pinned tool that is not offered any more is still shown, so it can be taken off", async () => {
    serve(["mcp:gone/tool"]);
    const patch = counted("patch", "/api/keys/{id}", () =>
      ok("patch", "/api/keys/{id}", 200, { ...active, pinned_tools: [] }),
    );
    const dialog = await openPins();
    const gone = await within(dialog).findByRole("checkbox", { name: "mcp:gone/tool" });
    expect(gone).toBeChecked();
    await userEvent.click(gone);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(patch.bodies).toEqual([{ pinned_tools: [] }]);
    });
  });

  test("a member sees no control to pin", async () => {
    serve();
    await renderWithApp(null, { route: "/keys", user: memberUser });
    await screen.findByRole("table", { name: "Virtual keys" });
    expect(screen.queryByRole("button", { name: "Edit pinned tools" })).toBeNull();
  });
});
