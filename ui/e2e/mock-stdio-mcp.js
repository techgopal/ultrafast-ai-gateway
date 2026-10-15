// A tiny MCP server over stdio, for the browser tests of local servers: the
// gateway runs it with `node`. One JSON-RPC message a line. Tools: `echo`
// (its arguments as text) and `whoami` (the names of its environment).
import { createInterface } from "node:readline";

const tools = [
  {
    name: "echo",
    description: "Says back what it is given.",
    inputSchema: { type: "object", properties: { text: { type: "string" } } },
  },
  {
    name: "whoami",
    description: "Names the environment variables it was started with.",
    inputSchema: { type: "object", properties: {} },
  },
];

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

const lines = createInterface({ input: process.stdin });
lines.on("line", (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  const { id, method, params } = message;
  if (id === undefined) return;
  if (method === "initialize") {
    send({
      jsonrpc: "2.0",
      id,
      result: {
        protocolVersion: "2025-06-18",
        capabilities: { tools: {} },
        serverInfo: { name: "e2e-stdio", version: "1" },
      },
    });
  } else if (method === "tools/list") {
    send({ jsonrpc: "2.0", id, result: { tools } });
  } else if (method === "tools/call") {
    const text =
      params?.name === "whoami"
        ? Object.keys(process.env).sort().join(",")
        : JSON.stringify(params?.arguments ?? {});
    send({ jsonrpc: "2.0", id, result: { content: [{ type: "text", text }], isError: false } });
  } else if (method === "ping") {
    send({ jsonrpc: "2.0", id, result: {} });
  } else {
    send({ jsonrpc: "2.0", id, error: { code: -32601, message: "Method not found" } });
  }
});
process.stderr.write("e2e-stdio ready\n");
