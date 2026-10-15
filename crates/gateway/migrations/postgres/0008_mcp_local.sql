-- MCP servers that run as a program on the gateway host (transport 'stdio'),
-- and the catalog entry a server was made from. A stdio server has no URL
-- (stored as an empty string); its command and arguments are exactly what is
-- run, with no shell. Environment values are encrypted by the caller, as a
-- JSON object of name to encrypted value. This is SQLite migration 0022.
ALTER TABLE mcp_servers ADD COLUMN transport TEXT COLLATE "C" NOT NULL DEFAULT 'http'
  CHECK (transport IN ('http','stdio'));
ALTER TABLE mcp_servers ADD COLUMN command TEXT COLLATE "C";
ALTER TABLE mcp_servers ADD COLUMN args TEXT COLLATE "C" NOT NULL DEFAULT '[]';
ALTER TABLE mcp_servers ADD COLUMN env_enc TEXT COLLATE "C" NOT NULL DEFAULT '{}';
ALTER TABLE mcp_servers ADD COLUMN catalog_id TEXT COLLATE "C";

-- The alert rule kind 'mcp_down' (a health probe finds an MCP server down).
DO $$
DECLARE c text;
BEGIN
  FOR c IN
    SELECT conname FROM pg_constraint
    WHERE conrelid = 'alert_rules'::regclass AND contype = 'c'
      AND pg_get_constraintdef(oid) LIKE '%circuit_open%'
  LOOP
    EXECUTE format('ALTER TABLE alert_rules DROP CONSTRAINT %I', c);
  END LOOP;
END $$;
ALTER TABLE alert_rules ADD CONSTRAINT alert_rules_kind_check
  CHECK (kind IN ('budget','error_rate','circuit_open','mcp_down'));
