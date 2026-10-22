-- Coding agents: see SQLite migration 0025.
ALTER TABLE request_logs ADD COLUMN cache_read_tokens BIGINT;
ALTER TABLE request_logs ADD COLUMN cache_write_tokens BIGINT;
ALTER TABLE request_logs ADD COLUMN agent_session TEXT COLLATE "C";
ALTER TABLE request_logs ADD COLUMN agent_id TEXT COLLATE "C";
ALTER TABLE request_logs ADD COLUMN agent_prompt TEXT COLLATE "C";
ALTER TABLE request_logs ADD COLUMN agent_class TEXT COLLATE "C";
ALTER TABLE request_logs ADD COLUMN agent_client TEXT COLLATE "C";
ALTER TABLE request_logs ADD COLUMN dropped TEXT COLLATE "C";
CREATE INDEX request_logs_agent_session ON request_logs (agent_session, at);
ALTER TABLE models ADD COLUMN cache_read_price_micros BIGINT;
ALTER TABLE models ADD COLUMN cache_write_price_micros BIGINT;
