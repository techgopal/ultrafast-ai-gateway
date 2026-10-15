// What the console knows of MCP servers and their tools: how a tool is named
// in a key's allowlist, what a sync said, and what is sent when a server is edited.
import { ApiError, ConsoleRefusal } from "@/api/errors";
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
  if (server.local !== null) {
    const { phase, restarts } = server.local;
    if (phase === "off") return "Local servers are off";
    if (phase === "crashlooping") return `Crash-looping (${String(restarts)} restarts)`;
    if (phase === "backoff") return "Restarting";
    if (phase === "starting") return "Starting";
    if (phase === "stopped") return "Stopped";
  }
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

type CatalogEntry = components["schemas"]["Entry"];
type CatalogView = components["schemas"]["McpCatalogView"];
type FromCatalogRequest = components["schemas"]["FromCatalogRequest"];

export const NEEDS_LOCAL = "Needs local servers: start the gateway with UF_MCP_LOCAL=on.";
export const NEEDS_BY_HAND = "Signs in with OAuth. Add it by hand.";
export const ENTER_A_VALUE = "Enter a value.";

/** The form field that holds the value of an item of an entry's `env`. */
export const envField = (name: string): string => `env_${name}`;

/**
 * Whether the entry can be added on this gateway, and if not why not:
 * a program that runs on the gateway host needs local servers, and OAuth
 * servers are added by hand.
 */
export function entryBlock(entry: CatalogEntry, view: CatalogView): string | null {
  if (entry.auth === "oauth") return NEEDS_BY_HAND;
  if (entry.transport === "stdio" && !view.local_enabled) return NEEDS_LOCAL;
  return null;
}

export interface CatalogValues {
  name: string;
  [field: string]: string;
}

/** The registration to send: only what the entry asks for, secrets required. */
export function catalogRequest(entry: CatalogEntry, values: CatalogValues): FromCatalogRequest {
  const secrets: Record<string, string> = {};
  for (const item of entry.env) {
    const value = values[envField(item.name)] ?? "";
    if (value.trim() === "") {
      if (item.secret) throw new ConsoleRefusal(ENTER_A_VALUE, envField(item.name));
      continue;
    }
    secrets[item.name] = value;
  }
  const body: FromCatalogRequest = { catalog_id: entry.id, secrets };
  const name = values.name.trim();
  if (name !== "" && name !== entry.id) body.name = name;
  return body;
}

/** An answer's `secrets.NAME` field is the form's `env_NAME`. */
export function formFieldsOf(fields: Readonly<Record<string, string>>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(fields).map(([name, text]) => [
      name.startsWith("secrets.") ? envField(name.slice("secrets.".length)) : name,
      text,
    ]),
  );
}

/** The error of the gateway, with the names of its fields as the catalog form has them. */
export function catalogFailure(error: unknown): unknown {
  return error instanceof ApiError
    ? new ApiError(error.status, error.code, error.message, formFieldsOf(error.fields), error.retryAfter)
    : error;
}

/** What the catalog's source says, in a few words. */
export function sourceText(view: CatalogView): string {
  const at = view.fetched_at === null ? "" : ` from ${view.fetched_at.slice(0, 10)}`;
  switch (view.source) {
    case "remote":
      return `Fetched${at}.`;
    case "cache":
      return `Showing the copy${at}.`;
    case "builtin":
      return "Showing the copy that ships with this gateway.";
  }
}

/** Entries whose name, text or tags contain every word of the search. */
export function searchCatalog(entries: readonly CatalogEntry[], search: string): CatalogEntry[] {
  const words = search.toLowerCase().split(/\s+/).filter((w) => w !== "");
  return entries.filter((entry) => {
    const text = `${entry.id} ${entry.name} ${entry.description} ${entry.tags.join(" ")}`.toLowerCase();
    return words.every((word) => text.includes(word));
  });
}

type CreateRequest = components["schemas"]["CreateMcpServerRequest"];

export const ENTER_A_COMMAND = "Enter the program to run.";
export const ENTER_A_VARIABLE = "Enter the name of the variable.";
export const VARIABLE_TWICE = "A variable is listed twice.";
export const PROGRAM_KINDS = [
  ["http", "Remote server (an address)"],
  ["stdio", "Program on the gateway host"],
] as const;

/** One environment variable of a local server in the editor. */
export interface EnvRow {
  name: string;
  /** What was typed; empty for a stored value that is kept. */
  value: string;
  /** Shown as dots while typing. Values are never shown again once saved. */
  secret: boolean;
  /** The server has a value for this name already. */
  stored: boolean;
}

/** The lines of the arguments box, one argument each, empty lines left out. */
export function argsOf(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.replace(/\r$/, ""))
    .filter((line) => line !== "");
}

/** The environment to send: name to value, an empty value keeping the stored one. */
export function envOf(rows: readonly EnvRow[]): Record<string, string> {
  const env: Record<string, string> = {};
  for (const row of rows) {
    const name = row.name.trim();
    if (name === "") throw new ConsoleRefusal(ENTER_A_VARIABLE, "env");
    if (name in env) throw new ConsoleRefusal(VARIABLE_TWICE, "env");
    if (row.value === "" && !row.stored) throw new ConsoleRefusal(ENTER_A_VALUE, "env");
    env[name] = row.value;
  }
  return env;
}

export interface LocalValues {
  name: string;
  command: string;
  args: string;
  description: string;
}

/** The registration of a program the gateway runs. */
export function localCreateOf(values: LocalValues, rows: readonly EnvRow[]): CreateRequest {
  const name = values.name.trim();
  const command = values.command.trim();
  if (name === "") throw new ConsoleRefusal("Enter the server's name.", "name");
  if (command === "") throw new ConsoleRefusal(ENTER_A_COMMAND, "command");
  const body: CreateRequest = {
    name,
    transport: "stdio",
    command,
    args: argsOf(values.args),
    description: values.description,
  };
  const env = envOf(rows);
  if (Object.keys(env).length > 0) body.env = env;
  return body;
}

/** The change to send for an edit of a local server: only what differs. */
export function localUpdateOf(
  was: Server,
  values: Omit<LocalValues, "name">,
  rows: readonly EnvRow[],
): UpdateRequest {
  const body: UpdateRequest = {};
  const command = values.command.trim();
  if (command === "") throw new ConsoleRefusal(ENTER_A_COMMAND, "command");
  if (command !== (was.command ?? "")) body.command = command;
  const args = argsOf(values.args);
  if (JSON.stringify(args) !== JSON.stringify(was.args ?? [])) body.args = args;
  if (values.description !== was.description) body.description = values.description;
  const env = envOf(rows);
  const names = Object.keys(env).sort();
  const was_ = [...(was.env_names ?? [])].sort();
  const typed = Object.values(env).some((value) => value !== "");
  if (typed || JSON.stringify(names) !== JSON.stringify(was_)) body.env = env;
  return body;
}

/** Whether the server is a program the gateway runs. */
export function isLocal(server: Server): boolean {
  return server.transport === "stdio";
}
