-- MCP at scale. A key may pin tools: the combined endpoint lists them (with
-- the search tools) when the key can call too many to list all of them.
-- A tool may have a projection: what of its result the agent is given. This
-- is SQLite migration 0023.
ALTER TABLE virtual_keys ADD COLUMN pinned_tools TEXT COLLATE "C";
CREATE TABLE mcp_tool_projections (
  tool_id BIGINT PRIMARY KEY REFERENCES mcp_tools(id) ON DELETE CASCADE,
  keep_paths TEXT COLLATE "C" NOT NULL DEFAULT '[]',
  drop_paths TEXT COLLATE "C" NOT NULL DEFAULT '[]',
  max_items BIGINT,
  max_bytes BIGINT);
