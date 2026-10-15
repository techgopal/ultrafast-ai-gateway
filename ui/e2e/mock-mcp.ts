// An MCP server in the test process, for the gateway to read the tools of. It
// speaks streamable HTTP with JSON-RPC, statelessly: `initialize` is answered,
// notifications get 202, and `tools/list` is paged by a cursor. Only the
// gateway calls it: the browser tests fail on any request of the browser to
// another origin.
import { createServer, type IncomingMessage } from "node:http";
import type { GatewayApi } from "./fixtures";

export interface MockMcpTool {
  name: string;
  description: string;
}

export interface MockMcp {
  /** The address to give the gateway. */
  url: string;
  /** The methods asked for, in order. */
  methods: string[];
  /** What `tools/list` offers; a test may change it before the next sync. */
  tools: MockMcpTool[];
  close: () => Promise<void>;
}

const PAGE = 2;

async function rawOf(request: IncomingMessage): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks).toString("utf8");
}

function objectOf(raw: string): Record<string, unknown> {
  try {
    const value: unknown = JSON.parse(raw);
    return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

export async function startMockMcp(
  tools: MockMcpTool[] = [
    { name: "echo", description: "Says back what it is given." },
    { name: "add", description: "Adds two numbers." },
    { name: "shout", description: "Says it loudly." },
  ],
): Promise<MockMcp> {
  const mock: MockMcp = { url: "", methods: [], tools, close: () => Promise.resolve() };
  const server = createServer((request, response) => {
    void (async () => {
      const body = objectOf(await rawOf(request));
      const method = typeof body.method === "string" ? body.method : "";
      const id = body.id;
      mock.methods.push(method);
      if (request.method !== "POST" || id === undefined) {
        // A notification: accepted, no body.
        response.writeHead(202).end();
        return;
      }
      const reply = (result: unknown) => {
        response.writeHead(200, { "content-type": "application/json" });
        response.end(JSON.stringify({ jsonrpc: "2.0", id, result }));
      };
      if (method === "initialize") {
        reply({
          protocolVersion: "2025-06-18",
          capabilities: { tools: {} },
          serverInfo: { name: "e2e-mcp", version: "1" },
        });
        return;
      }
      if (method === "tools/list") {
        const params = (body.params ?? {}) as { cursor?: unknown };
        const start = typeof params.cursor === "string" ? Number(params.cursor) : 0;
        const page = mock.tools.slice(start, start + PAGE).map((tool) => ({
          name: tool.name,
          description: tool.description,
          inputSchema: { type: "object", properties: {} },
        }));
        reply({
          tools: page,
          ...(start + PAGE < mock.tools.length ? { nextCursor: String(start + PAGE) } : {}),
        });
        return;
      }
      response.writeHead(200, { "content-type": "application/json" });
      response.end(
        JSON.stringify({ jsonrpc: "2.0", id, error: { code: -32601, message: "Method not found" } }),
      );
    })();
  });
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("the mock MCP did not start");
  mock.url = `http://127.0.0.1:${String(address.port)}/mcp`;
  mock.close = () =>
    new Promise<void>((resolve) => {
      server.closeAllConnections();
      server.close(() => {
        resolve();
      });
    });
  return mock;
}

/**
 * Registers the mock as server "mock" through the API, syncs it, and turns
 * its `echo` tool on for everyone: the MCP page and the key dialog then have
 * something to show.
 */
export async function seedMcp(api: GatewayApi, url: string): Promise<void> {
  const server = (await api.send("POST", "/api/mcp/servers", { name: "mock", url })) as { id: number };
  await api.send("POST", `/api/mcp/servers/${String(server.id)}/sync`);
  const listed = (await api.get(`/api/mcp/servers/${String(server.id)}/tools`)) as {
    tools: { id: number; name: string }[];
  };
  const echo = listed.tools.find((tool) => tool.name === "echo");
  if (echo === undefined) throw new Error("the mock did not offer echo");
  await api.send("PATCH", `/api/mcp/tools/${String(echo.id)}`, { enabled: true });
  await api.send("PUT", `/api/mcp/tools/${String(echo.id)}/grants`, {
    everyone: true,
    team_ids: [],
    user_ids: [],
  });
}
