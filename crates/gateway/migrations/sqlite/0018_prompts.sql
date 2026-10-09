-- Prompt templates: named, versioned messages with {{variables}}, rendered
-- into a call by name. A version never changes once written; a new text is a
-- new version. Deleting a template deletes its versions; the request log
-- keeps only the text name@version, so it survives.
CREATE TABLE prompt_templates (
  id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1,
  name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
  -- Who made it (a team lead manages the templates they made). The user
  -- going away leaves it to the admins.
  created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
  created_at TEXT NOT NULL,
  UNIQUE (org_id, name));
CREATE TABLE prompt_versions (
  id INTEGER PRIMARY KEY,
  template_id INTEGER NOT NULL REFERENCES prompt_templates(id) ON DELETE CASCADE,
  version INTEGER NOT NULL,
  messages TEXT NOT NULL,                 -- JSON array of {role, content}
  variables TEXT NOT NULL DEFAULT '[]',   -- JSON array of the names the messages use
  model TEXT,                             -- used when the call names none
  params TEXT NOT NULL DEFAULT '{}',      -- JSON: temperature, max_tokens, top_p, response_format
  created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
  created_at TEXT NOT NULL,
  UNIQUE (template_id, version));
-- Immutable: the text of a version cannot be updated, whoever asks.
CREATE TRIGGER prompt_versions_immutable
  BEFORE UPDATE OF template_id, version, messages, variables, model, params ON prompt_versions
  BEGIN SELECT RAISE(ABORT, 'prompt versions never change'); END;
-- The template and version a call used, as name@version. Text, not a
-- reference: it outlives the template.
ALTER TABLE request_logs ADD COLUMN prompt TEXT;
