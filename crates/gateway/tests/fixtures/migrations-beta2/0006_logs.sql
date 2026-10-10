-- Request logs: one row per authenticated /v1 call, metadata only. No prompt
-- and no answer is stored. Rows are deleted after the retention setting.
CREATE TABLE request_logs (
    id            INTEGER PRIMARY KEY,
    org_id        INTEGER NOT NULL DEFAULT 1,
    at            TEXT NOT NULL,
    key_id        INTEGER,
    user_id       INTEGER,
    team_id       INTEGER,
    requested     TEXT NOT NULL,
    endpoint      TEXT NOT NULL,
    stream        INTEGER NOT NULL,
    status        INTEGER NOT NULL,
    provider      TEXT,
    model         TEXT,
    input_tokens  INTEGER,
    output_tokens INTEGER,
    cost_micros   INTEGER NOT NULL DEFAULT 0,
    priced        INTEGER NOT NULL DEFAULT 0,
    cached        INTEGER NOT NULL DEFAULT 0,
    duration_ms   INTEGER NOT NULL,
    -- A JSON array of the targets tried, in order.
    attempts      TEXT NOT NULL
);

CREATE INDEX request_logs_at ON request_logs (at);
CREATE INDEX request_logs_key ON request_logs (key_id, at);
CREATE INDEX request_logs_user ON request_logs (user_id, at);
CREATE INDEX request_logs_team ON request_logs (team_id, at);

-- Prices per one million tokens, in millionths of a dollar. NULL: unknown.
ALTER TABLE models ADD COLUMN input_price_micros INTEGER;
ALTER TABLE models ADD COLUMN output_price_micros INTEGER;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO settings (key, value) VALUES ('log_retention_days', '30');
