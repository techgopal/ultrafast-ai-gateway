-- Alerts: where notifications go (channels), when they are made (rules),
-- what a rule has seen (state) and what was sent (events).
CREATE TABLE alert_channels (
  id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1,
  name TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('webhook','slack')),
  url_enc BLOB NOT NULL, url_host TEXT NOT NULL, secret_enc BLOB NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL,
  UNIQUE (org_id, name));
CREATE TABLE alert_rules (
  id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1,
  name TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('budget','error_rate','circuit_open')),
  params TEXT NOT NULL,            -- JSON, validated per kind
  enabled INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL,
  UNIQUE (org_id, name));
CREATE TABLE alert_rule_channels (
  rule_id INTEGER NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
  channel_id INTEGER NOT NULL REFERENCES alert_channels(id) ON DELETE CASCADE,
  PRIMARY KEY (rule_id, channel_id));
CREATE TABLE alert_state (       -- one row per rule and subject while known
  rule_id INTEGER NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
  subject TEXT NOT NULL,         -- e.g. 'budget:12:2026-10-05', 'route:chat', 'target:openai/gpt-4o'
  firing INTEGER NOT NULL, since TEXT NOT NULL,
  PRIMARY KEY (rule_id, subject));
CREATE TABLE alert_events (
  id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1,
  rule_id INTEGER REFERENCES alert_rules(id) ON DELETE SET NULL, rule_name TEXT NOT NULL,
  kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('firing','resolved','test')),
  summary TEXT NOT NULL, details TEXT NOT NULL,   -- JSON, metadata only
  at TEXT NOT NULL, deliveries TEXT NOT NULL DEFAULT '[]');  -- JSON [{channel_id, channel_name, ok, status, tries, error}]
CREATE INDEX alert_events_at ON alert_events (org_id, at);
