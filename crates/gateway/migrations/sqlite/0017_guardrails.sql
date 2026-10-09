-- Guardrails: rules that block, redact or flag the text going to and coming
-- from models, or an external webhook that decides. A guardrail is attached
-- to routes and keys (in order), or made a gateway-wide default.
CREATE TABLE guardrails (
  id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1,
  name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
  kind TEXT NOT NULL CHECK (kind IN ('rules','external')),
  rules TEXT NOT NULL DEFAULT '[]',     -- JSON array of rules (kind 'rules')
  -- kind 'external': where to post (encrypted; only the host is shown), the
  -- signing secret (encrypted), how long to wait, what to do when it fails
  -- and which directions it is asked about.
  external_url_enc BLOB, external_url_host TEXT, external_secret_enc BLOB,
  timeout_ms INTEGER NOT NULL DEFAULT 3000,
  fail_mode TEXT NOT NULL DEFAULT 'open' CHECK (fail_mode IN ('open','closed')),
  directions TEXT NOT NULL DEFAULT 'both' CHECK (directions IN ('input','output','both')),
  enabled INTEGER NOT NULL DEFAULT 1,
  is_default INTEGER NOT NULL DEFAULT 0,  -- applies to every call of the gateway
  created_at TEXT NOT NULL,
  UNIQUE (org_id, name));
CREATE TABLE route_guardrails (
  route_id INTEGER NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
  guardrail_id INTEGER NOT NULL REFERENCES guardrails(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  PRIMARY KEY (route_id, guardrail_id));
CREATE INDEX route_guardrails_guardrail ON route_guardrails (guardrail_id);
CREATE TABLE key_guardrails (
  key_id INTEGER NOT NULL REFERENCES virtual_keys(id) ON DELETE CASCADE,
  guardrail_id INTEGER NOT NULL REFERENCES guardrails(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  PRIMARY KEY (key_id, guardrail_id));
CREATE INDEX key_guardrails_guardrail ON key_guardrails (guardrail_id);
-- What the guardrails did to a call: a JSON outcome of ids, actions and
-- counts. Never the text that matched.
ALTER TABLE request_logs ADD COLUMN guardrails TEXT;
