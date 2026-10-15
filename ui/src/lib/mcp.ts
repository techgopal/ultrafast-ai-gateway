// What the console knows of MCP servers and their tools: how a tool is named
// in a key's allowlist, what a sync said, and what is sent when a server is edited.
import { ConsoleRefusal } from "@/api/errors";
import type { components } from "@/api/schema";

type Server = components["schemas"]["McpServerView"];
type Tool = components["schemas"]["McpToolView"];
type SyncResult = components["schemas"]["McpSyncResult"];
type UpdateRequest = components["schemas"]["UpdateMcpServerRequest"];

export const HOST_CHANGED = "Enter the secret again: the host changed.";
export const AUTH_KINDS = [
  ["none", "No authentication"],
  ["bearer", "Bearer token"],
  ["header", "Custom header"],
] as const;

/** How a call and a key's allowlist name a tool. */
export function toolRef(server: string, tool: string): string {
  return `mcp:${server}/${tool}`;
}

/** What a sync changed, for the toast. */
export function syncText(result: SyncResult): string {
  return `Synced: added ${String(result.added.length)}, updated ${String(result.updated.length)}, removed ${String(result.removed.length)}, skipped ${String(result.skipped)}.`;
}

/** Whether the address of the server is on another host (or cannot be read). */
export function hostChanged(was: string, now: string): boolean {
  try {
    return new URL(was).host !== new URL(now).host;
  } catch {
    return true;
  }
}

/** In a few words, how the server stands. */
export function statusOf(server: Server): string {
  if (!server.enabled) return "Disabled";
  if (server.usable === false) return "Secret needed";
  if (server.last_sync_error !== null) return "Sync failed";
  if (server.last_sync_at === null) return "Not synced yet";
  return "Synced";
}

export interface ServerValues {
  url: string;
  description: string;
  auth_kind: string;
  auth_header: string;
  secret: string;
}

/**
 * The change to send for an edit: only what differs, the secret only when one
 * was typed. A new host without the secret is refused here, as the gateway
 * would: the stored secret goes only to the host it was given for.
 */
export function updateOf(was: Server, values: ServerValues): UpdateRequest {
  const body: UpdateRequest = {};
  const url = values.url.trim();
  if (url !== (was.url ?? "")) body.url = url;
  if (values.description !== was.description) body.description = values.description;
  if (values.auth_kind !== (was.auth_kind ?? "none")) body.auth_kind = values.auth_kind;
  const header = values.auth_header.trim();
  if (values.auth_kind === "header" && header !== (was.auth_header ?? "")) body.auth_header = header;
  if (values.secret !== "") body.secret = values.secret;
  else if (
    body.url !== undefined &&
    was.has_secret === true &&
    values.auth_kind !== "none" &&
    hostChanged(was.url ?? "", url)
  ) {
    throw new ConsoleRefusal(HOST_CHANGED, "secret");
  }
  return body;
}

/** Callable tools of each server, for the lists a member and a key form show. */
export interface ServerTools {
  server: Server;
  tools: readonly Tool[];
}

/** The tools that can be called, by name. */
export function liveTools(tools: readonly Tool[]): Tool[] {
  return tools
    .filter((tool) => tool.enabled && !tool.removed)
    .sort((a, b) => a.name.localeCompare(b.name));
}
