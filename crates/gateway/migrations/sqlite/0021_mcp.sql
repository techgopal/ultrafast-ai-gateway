-- MCP servers the gateway proxies, the tools they offer and who may call
-- each tool. A sync that no longer sees a tool marks it removed instead of
-- deleting it, so its grants survive the tool coming back.
CREATE TABLE mcp_servers (
  id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1,
  name TEXT NOT NULL, url TEXT NOT NULL,
  auth_kind TEXT NOT NULL DEFAULT 'none' CHECK (auth_kind IN ('none','bearer','header')),
  auth_header TEXT,                -- header name when auth_kind is 'header'
  auth_secret_enc TEXT,            -- the secret, encrypted by the caller
  enabled INTEGER NOT NULL DEFAULT 1,
  description TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  last_sync_at TEXT,
  last_sync_error TEXT,            -- our own summary, never upstream text
  UNIQUE (org_id, name));
CREATE TABLE mcp_tools (
  id INTEGER PRIMARY KEY,
  server_id INTEGER NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
  name TEXT NOT NULL, title TEXT, description TEXT NOT NULL DEFAULT '',
  input_schema TEXT NOT NULL,      -- JSON as the upstream gave it
  output_schema TEXT, annotations TEXT,
  enabled INTEGER NOT NULL DEFAULT 0,
  removed_at TEXT,                 -- set when a sync no longer sees the tool
  UNIQUE (server_id, name));
CREATE TABLE mcp_tool_grants (
  tool_id INTEGER NOT NULL REFERENCES mcp_tools(id) ON DELETE CASCADE,
  team_id INTEGER REFERENCES teams(id) ON DELETE CASCADE,
  user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
  CHECK (team_id IS NULL OR user_id IS NULL));
CREATE UNIQUE INDEX mcp_tool_grants_unique
  ON mcp_tool_grants (tool_id, COALESCE(team_id, 0), COALESCE(user_id, 0));
CREATE INDEX mcp_tool_grants_team ON mcp_tool_grants (team_id);
CREATE INDEX mcp_tool_grants_user ON mcp_tool_grants (user_id);
