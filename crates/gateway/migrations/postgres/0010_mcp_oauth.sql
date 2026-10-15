-- OAuth toward an MCP server: see SQLite migration 0024.
ALTER TABLE mcp_servers ADD COLUMN oauth_token_url TEXT COLLATE "C";
ALTER TABLE mcp_servers ADD COLUMN oauth_client_id TEXT COLLATE "C";
ALTER TABLE mcp_servers ADD COLUMN oauth_client_secret_enc TEXT COLLATE "C";
ALTER TABLE mcp_servers ADD COLUMN oauth_scopes TEXT COLLATE "C";
ALTER TABLE mcp_servers ADD COLUMN oauth_grant TEXT COLLATE "C"
  CHECK (oauth_grant IS NULL OR oauth_grant IN ('client_credentials','token_exchange'));
CREATE TABLE mcp_tokens (
  server_id BIGINT PRIMARY KEY REFERENCES mcp_servers(id) ON DELETE CASCADE,
  token_enc TEXT COLLATE "C" NOT NULL,
  expires_at TEXT COLLATE "C" NOT NULL);
