-- Rate limits of the gateway, a team, a user or a key. Each limit is
-- optional: NULL is no limit of that kind.
CREATE TABLE rate_limits (
    id                  INTEGER PRIMARY KEY,
    org_id              INTEGER NOT NULL DEFAULT 1,
    scope               TEXT    NOT NULL CHECK (scope IN ('gateway', 'team', 'user', 'key')),
    -- NULL for the gateway, the id of the team, user or key otherwise.
    scope_id            INTEGER,
    requests_per_minute INTEGER CHECK (requests_per_minute IS NULL OR requests_per_minute > 0),
    tokens_per_minute   INTEGER CHECK (tokens_per_minute IS NULL OR tokens_per_minute > 0),
    concurrent          INTEGER CHECK (concurrent IS NULL OR concurrent > 0),
    created_at          TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (org_id, scope, scope_id),
    CHECK ((scope = 'gateway') = (scope_id IS NULL))
);

-- NULLs are distinct in a UNIQUE constraint, so the gateway's single
-- limit needs this index to stay single.
CREATE UNIQUE INDEX idx_rate_limits_subject ON rate_limits (org_id, scope, COALESCE(scope_id, 0));

-- The scope is not a foreign key (it points at one of three tables), so
-- what a deleted team or user leaves behind is removed here.
CREATE TRIGGER rate_limits_team_deleted AFTER DELETE ON teams
BEGIN
    DELETE FROM rate_limits WHERE scope = 'team' AND scope_id = OLD.id;
END;

CREATE TRIGGER rate_limits_user_deleted AFTER DELETE ON users
BEGIN
    DELETE FROM rate_limits WHERE scope = 'user' AND scope_id = OLD.id;
END;
