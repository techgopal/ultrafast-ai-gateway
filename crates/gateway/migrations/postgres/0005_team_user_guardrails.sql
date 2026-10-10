-- Guardrails on teams and users: a guardrail attached to a team or a user
-- covers every key of that team or owned by that user, so a new key cannot
-- step around it. Applied after the gateway-wide defaults and before the
-- route's and the key's. This is SQLite migration 0019.
CREATE TABLE team_guardrails (
  team_id BIGINT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
  guardrail_id BIGINT NOT NULL REFERENCES guardrails(id) ON DELETE CASCADE,
  position BIGINT NOT NULL,
  PRIMARY KEY (team_id, guardrail_id));
CREATE INDEX team_guardrails_guardrail ON team_guardrails (guardrail_id);
CREATE TABLE user_guardrails (
  user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  guardrail_id BIGINT NOT NULL REFERENCES guardrails(id) ON DELETE CASCADE,
  position BIGINT NOT NULL,
  PRIMARY KEY (user_id, guardrail_id));
CREATE INDEX user_guardrails_guardrail ON user_guardrails (guardrail_id);
