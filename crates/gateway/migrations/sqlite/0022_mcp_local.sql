-- MCP servers that run as a program on the gateway host (transport 'stdio'),
-- and the catalog entry a server was made from. A stdio server has no URL
-- (stored as an empty string); its command and arguments are exactly what is
-- run, with no shell. Environment values are encrypted by the caller, as a
-- JSON object of name to encrypted value.
ALTER TABLE mcp_servers ADD COLUMN transport TEXT NOT NULL DEFAULT 'http'
  CHECK (transport IN ('http','stdio'));
ALTER TABLE mcp_servers ADD COLUMN command TEXT;
ALTER TABLE mcp_servers ADD COLUMN args TEXT NOT NULL DEFAULT '[]';
ALTER TABLE mcp_servers ADD COLUMN env_enc TEXT NOT NULL DEFAULT '{}';
ALTER TABLE mcp_servers ADD COLUMN catalog_id TEXT;

-- The alert rule kind 'mcp_down' (a health probe finds an MCP server down).
-- SQLite cannot change a CHECK, so alert_rules is made again. Dropping it
-- would delete what hangs off it (channels, states) and clear the rule of
-- its events, so those are put aside and put back.
CREATE TEMP TABLE _arc AS SELECT rule_id, channel_id FROM alert_rule_channels;
CREATE TEMP TABLE _ast AS
  SELECT rule_id, subject, firing, since, owner, seen_at FROM alert_state;
CREATE TEMP TABLE _aev AS SELECT id, rule_id FROM alert_events WHERE rule_id IS NOT NULL;
CREATE TABLE alert_rules_new (
  id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('budget','error_rate','circuit_open','mcp_down')),
  params TEXT NOT NULL,            -- JSON, validated per kind
  enabled INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL,
  UNIQUE (org_id, name));
INSERT INTO alert_rules_new (id, org_id, name, kind, params, enabled, created_at)
  SELECT id, org_id, name, kind, params, enabled, created_at FROM alert_rules;
DROP TABLE alert_rules;
ALTER TABLE alert_rules_new RENAME TO alert_rules;
INSERT INTO alert_rule_channels (rule_id, channel_id) SELECT rule_id, channel_id FROM _arc;
INSERT INTO alert_state (rule_id, subject, firing, since, owner, seen_at)
  SELECT rule_id, subject, firing, since, owner, seen_at FROM _ast;
UPDATE alert_events
  SET rule_id = (SELECT rule_id FROM _aev WHERE _aev.id = alert_events.id)
  WHERE id IN (SELECT id FROM _aev);
DROP TABLE _arc;
DROP TABLE _ast;
DROP TABLE _aev;
