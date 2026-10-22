-- Coding agents: cache token counts and what the agent said about itself on
-- each request log row, cache prices on models.
--
-- input_tokens stays the uncached input; cache reads and writes are counted
-- apart. NULL: the call has no usage (as for input_tokens).
ALTER TABLE request_logs ADD COLUMN cache_read_tokens INTEGER;
ALTER TABLE request_logs ADD COLUMN cache_write_tokens INTEGER;
-- The agent's own session, agent, prompt and request class (from its
-- headers), and which agent it is: claude-code or codex. Text cut to 128
-- printable characters; never a credential.
ALTER TABLE request_logs ADD COLUMN agent_session TEXT;
ALTER TABLE request_logs ADD COLUMN agent_id TEXT;
ALTER TABLE request_logs ADD COLUMN agent_prompt TEXT;
ALTER TABLE request_logs ADD COLUMN agent_class TEXT;
ALTER TABLE request_logs ADD COLUMN agent_client TEXT;
-- A JSON array of what translation left out of the request (field, tool and
-- block names); NULL when nothing.
ALTER TABLE request_logs ADD COLUMN dropped TEXT;
CREATE INDEX request_logs_agent_session ON request_logs (agent_session, at);
-- Prices per one million tokens, in millionths of a dollar. NULL: the input
-- price is used.
ALTER TABLE models ADD COLUMN cache_read_price_micros INTEGER;
ALTER TABLE models ADD COLUMN cache_write_price_micros INTEGER;
