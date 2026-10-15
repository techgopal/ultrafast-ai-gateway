-- OAuth toward an MCP server: the gateway is the client and gets a token for
-- the server itself (client credentials, or RFC 8693 token exchange, with the
-- server's URL as the RFC 8707 resource). A server that signs in this way has
-- `auth_kind` 'none' here and an `oauth_grant`; the API shows it as 'oauth'.
-- The secret is encrypted by the caller. Tokens are cached encrypted, so that
-- gateways sharing the database do not each ask for one on every start.
ALTER TABLE mcp_servers ADD COLUMN oauth_token_url TEXT;
ALTER TABLE mcp_servers ADD COLUMN oauth_client_id TEXT;
ALTER TABLE mcp_servers ADD COLUMN oauth_client_secret_enc TEXT;
ALTER TABLE mcp_servers ADD COLUMN oauth_scopes TEXT;
ALTER TABLE mcp_servers ADD COLUMN oauth_grant TEXT
  CHECK (oauth_grant IS NULL OR oauth_grant IN ('client_credentials','token_exchange'));
CREATE TABLE mcp_tokens (
  server_id INTEGER PRIMARY KEY REFERENCES mcp_servers(id) ON DELETE CASCADE,
  token_enc TEXT NOT NULL,
  expires_at TEXT NOT NULL);
