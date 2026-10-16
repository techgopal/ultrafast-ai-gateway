-- MCP at scale. A key may pin tools: the combined endpoint lists them (with
-- the search tools) when the key can call too many to list all of them.
-- A tool may have a projection: what of its result the agent is given.
ALTER TABLE virtual_keys ADD COLUMN pinned_tools TEXT;           -- JSON array of 'mcp:<server>/<tool>', or NULL
CREATE TABLE mcp_tool_projections (
  tool_id INTEGER PRIMARY KEY REFERENCES mcp_tools(id) ON DELETE CASCADE,
  keep_paths TEXT NOT NULL DEFAULT '[]',   -- JSON array of JSONPath queries
  drop_paths TEXT NOT NULL DEFAULT '[]',   -- JSON array of JSONPath queries
  max_items INTEGER,
  max_bytes INTEGER);
