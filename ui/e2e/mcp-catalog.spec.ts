// The MCP catalog, local servers, health and the playground's MCP tab, against
// the real gateway binary. The catalog is the copy built into the binary
// (`UF_MCP_CATALOG=off` in every test gateway); a local server is the Node
// script `mock-stdio-mcp.js`; an HTTP server is the mock of `mock-mcp.ts`.
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { fileURLToPath } from "node:url";
import { expect, goTo, itemOf, signInFromStart, test } from "./fixtures";
import { startMockMcp, type MockMcp } from "./mock-mcp";
import { startMockProvider, type MockProvider } from "./mock-provider";

const STDIO_SCRIPT = fileURLToPath(new URL("./mock-stdio-mcp.js", import.meta.url));

test.describe("the catalog", () => {
  test("lists the entries, says what needs local servers, and asks only for the secrets of an entry", async ({
    page,
    admin,
  }) => {
    await signInFromStart(page, admin);
    await goTo(page, "MCP tools");
    await page.getByRole("link", { name: "Add from catalog" }).click();
    await expect(page.getByRole("heading", { level: 1, name: "MCP catalog" })).toBeVisible();
    await expect(page.getByText("Showing the copy that ships with this gateway.")).toBeVisible();
    await expect(page.getByText(/Fetching the catalog is switched off/)).toBeVisible();

    const entries = page.getByRole("list", { name: "Catalog entries" });
    await expect(entries.getByRole("heading", { level: 2, name: "GitHub" })).toBeVisible();
    // A program on the host needs local servers, which this gateway has off.
    const files = entries.getByRole("listitem").filter({ hasText: "Filesystem" });
    await expect(files.getByRole("button", { name: "Add Filesystem" })).toBeDisabled();
    await expect(files).toContainText("Needs local servers");

    // The search narrows the list.
    await page.getByLabel("Search").fill("error");
    await expect(entries.getByRole("heading", { level: 2 })).toHaveText(["Sentry"]);

    // The dialog asks for the name and the one secret of the entry.
    await entries.getByRole("button", { name: "Add Sentry" }).click();
    const dialog = page.getByRole("dialog", { name: "Add Sentry" });
    await expect(dialog.getByLabel("Name")).toHaveValue("sentry");
    await expect(dialog.getByLabel(/SENTRY_AUTH_TOKEN/)).toHaveAttribute("type", "password");
    await dialog.getByRole("button", { name: "Add server" }).click();
    await expect(dialog.getByText("Enter a value.")).toBeVisible();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toBeHidden();
  });
});

test.describe("local servers", () => {
  test.use({ extraEnv: { UF_MCP_LOCAL: "on", UF_MCP_HEALTH_INTERVAL: "5" } });

  let provider: MockProvider;
  test.beforeEach(async () => {
    provider = await startMockProvider(["e2e-model"]);
  });
  test.afterEach(async () => {
    await provider.close();
  });

  test("an admin adds a program, reads its output, calls its tool in the playground, and the call is logged", async ({
    page,
    admin,
    apiAs,
  }) => {
    const api = await apiAs(admin);
    // The playground needs something to chat with.
    const added = await api.addProvider("alpha", provider.baseUrl, provider.apiKey);
    await api.sync(added);
    await api.open("alpha", "e2e-model");

    await signInFromStart(page, admin);
    await goTo(page, "MCP tools");
    await page.getByRole("button", { name: "Add server" }).click();
    const add = page.getByRole("dialog", { name: "Add MCP server" });
    await add.getByRole("radio", { name: "Program on the gateway host" }).check();
    await add.getByLabel("Name", { exact: true }).fill("files");
    await add.getByLabel("Command", { exact: true }).fill(process.execPath);
    await add.getByLabel("Arguments").fill(STDIO_SCRIPT);
    await add.getByRole("button", { name: "Add variable" }).click();
    await add.getByLabel("Variable 1 name").fill("E2E_FLAG");
    await add.getByLabel("Variable 1 value").fill("on");
    await add.getByRole("button", { name: "Add server" }).click();
    await expect(add).toBeHidden();

    const server = itemOf(page, "MCP servers", "files");
    await expect(server).toContainText(process.execPath);
    // The program comes up on its own; sync reads its tools.
    await expect(async () => {
      await server.getByRole("button", { name: "Sync" }).click();
      await expect(page.getByText("Synced: added 2, updated 0, removed 0, skipped 0.")).toBeVisible({
        timeout: 2_000,
      });
    }).toPass({ timeout: 20_000 });

    // What it wrote to stderr is shown.
    await server.getByRole("button", { name: "Output" }).click();
    await expect(page.getByRole("textbox", { name: /Output of files/ })).toHaveValue(/e2e-stdio ready/);
    await page.getByRole("button", { name: "Close output" }).click();

    // Both tools on, for everyone.
    for (const name of ["echo", "whoami"]) {
      const row = itemOf(page, "Tools of files", name);
      await row.getByRole("switch", { name: `${name} on` }).click();
      await expect(row).toContainText("On");
      await row.getByRole("button", { name: "Edit access" }).click();
      const access = page.getByRole("dialog", { name: "Edit access" });
      await access.getByRole("radio", { name: /Everyone/ }).check();
      await access.getByRole("button", { name: "Save access" }).click();
      await expect(access).toBeHidden();
    }

    // The playground calls the tool as the admin.
    await goTo(page, "Playground");
    await page.getByRole("button", { name: "MCP tools" }).click();
    const form = page.getByRole("form", { name: "MCP call" });
    await form.getByRole("combobox", { name: "MCP tool" }).click();
    await page.getByRole("option", { name: "echo" }).click();
    await form.getByLabel("Arguments").fill('{"text": "hello"}');
    await form.getByRole("button", { name: "Run tool" }).click();
    const result = page.getByRole("region", { name: "Result" });
    await expect(result).toContainText("mcp:files/echo");
    await expect(result).toContainText('{"text":"hello"}');

    // The program saw only what it was given.
    await form.getByRole("combobox", { name: "MCP tool" }).click();
    await page.getByRole("option", { name: "whoami" }).click();
    await form.getByRole("button", { name: "Run tool" }).click();
    await expect(result).toContainText("E2E_FLAG");
    await expect(result).not.toContainText("UF_ADMIN_PASSWORD");
    await expect(result).not.toContainText("UF_DATA_DIR");

    // Logged like an agent's call: under the user, under no key.
    await expect
      .poll(async () => (await api.logs()).filter((row) => row.endpoint === "mcp").length, {
        timeout: 15_000,
      })
      .toBe(2);
    const calls = (await api.logs()).filter((row) => row.endpoint === "mcp");
    expect(calls.map((row) => row.requested).sort()).toEqual(["mcp:files/echo", "mcp:files/whoami"]);
    expect(calls.every((row) => row.key_name === null && row.user_email === admin.email)).toBe(true);

    // Switched off, the program stops and the status says so.
    await goTo(page, "MCP tools");
    await server.getByRole("button", { name: "Edit" }).click();
    await expect(page.getByRole("dialog", { name: "Edit MCP server" })).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();
    await api.send("PATCH", `/api/mcp/servers/${String(await serverId(api, "files"))}`, { enabled: false });
    await page.reload();
    await expect(itemOf(page, "MCP servers", "files")).toContainText("Disabled");
  });
});

async function serverId(api: { get: (path: string) => Promise<unknown> }, name: string): Promise<number> {
  const list = (await api.get("/api/mcp/servers")) as { servers: { id: number; name: string }[] };
  const found = list.servers.find((one) => one.name === name);
  if (found === undefined) throw new Error(`no server ${name}`);
  return found.id;
}

test.describe("health", () => {
  test.use({ extraEnv: { UF_MCP_HEALTH_INTERVAL: "5" } });

  let mock: MockMcp;
  test.beforeEach(async () => {
    mock = await startMockMcp();
  });
  test.afterEach(async () => {
    await mock.close();
  });

  test("a server that goes down is shown as down and an mcp_down alert reaches the webhook, then recovers", async ({
    page,
    admin,
    apiAs,
  }) => {
    const received: { body: string }[] = [];
    const hook = createServer((request, response) => {
      const chunks: Buffer[] = [];
      request.on("data", (chunk: Buffer) => chunks.push(chunk));
      request.on("end", () => {
        received.push({ body: Buffer.concat(chunks).toString("utf8") });
        response.statusCode = 200;
        response.end("ok");
      });
    });
    await new Promise<void>((done) => hook.listen(0, "127.0.0.1", done));
    const { port } = hook.address() as AddressInfo;
    try {
      const api = await apiAs(admin);
      const server = (await api.send("POST", "/api/mcp/servers", { name: "mock", url: mock.url })) as {
        id: number;
      };
      await api.send("POST", `/api/mcp/servers/${String(server.id)}/sync`);
      const channel = (await api.send("POST", "/api/alerts/channels", {
        name: "ops",
        kind: "webhook",
        url: `http://127.0.0.1:${String(port)}/hook/e2e`,
      })) as { channel: { id: number } };
      await api.send("POST", "/api/alerts/rules", {
        name: "mcp down",
        kind: "mcp_down",
        params: { server: "mock" },
        channel_ids: [channel.channel.id],
      });

      await signInFromStart(page, admin);
      await goTo(page, "MCP tools");
      const row = itemOf(page, "MCP servers", "mock");
      // The first look comes a few seconds after the start.
      await expect(row).toContainText("Answered in", { timeout: 20_000 });

      mock.down = true;
      await expect(row).toContainText("Down", { timeout: 40_000 });
      await expect.poll(() => received.length, { timeout: 20_000 }).toBeGreaterThanOrEqual(1);
      const firing = JSON.parse(received[0]?.body ?? "{}") as {
        state: string;
        subject: string;
        rule: { kind: string };
      };
      expect(firing).toMatchObject({ state: "firing", subject: "mcp:mock", rule: { kind: "mcp_down" } });

      mock.down = false;
      await expect(row).toContainText("Answered in", { timeout: 30_000 });
      await expect
        .poll(() => received.some((one) => (JSON.parse(one.body) as { state: string }).state === "resolved"), {
          timeout: 20_000,
        })
        .toBe(true);
    } finally {
      await new Promise<void>((done) => hook.close(() => { done(); }));
    }
  });
});
