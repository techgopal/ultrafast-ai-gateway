CREATE TABLE users (
    id             INTEGER PRIMARY KEY,
    org_id         INTEGER NOT NULL DEFAULT 1,
    email          TEXT    NOT NULL,
    name           TEXT    NOT NULL,
    role           TEXT    NOT NULL CHECK (role IN ('admin', 'member')),
    status         TEXT    NOT NULL CHECK (status IN ('active', 'invited', 'disabled')),
    password_hash  TEXT,
    auth_provider  TEXT    NOT NULL DEFAULT 'password',
    external_id    TEXT,
    created_at     TEXT    NOT NULL DEFAULT (datetime('now')),
    last_active_at TEXT,
    UNIQUE (org_id, email)
);

CREATE TABLE invites (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT    NOT NULL UNIQUE,
    expires_at  TEXT    NOT NULL,
    used_at     TEXT,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE teams (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    name        TEXT    NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (org_id, name)
);

CREATE TABLE team_members (
    org_id   INTEGER NOT NULL DEFAULT 1,
    team_id  INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role     TEXT    NOT NULL CHECK (role IN ('lead', 'member')),
    PRIMARY KEY (team_id, user_id)
);

CREATE TABLE sessions (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    id_hash     TEXT    NOT NULL UNIQUE,
    csrf_token  TEXT    NOT NULL,
    expires_at  TEXT    NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE access_tokens (
    id           INTEGER PRIMARY KEY,
    org_id       INTEGER NOT NULL DEFAULT 1,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT    NOT NULL,
    token_hash   TEXT    NOT NULL UNIQUE,
    display      TEXT    NOT NULL,
    expires_at   TEXT,
    revoked_at   TEXT,
    last_used_at TEXT,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE audit_log (
    id             INTEGER PRIMARY KEY,
    org_id         INTEGER NOT NULL DEFAULT 1,
    at             TEXT    NOT NULL DEFAULT (datetime('now')),
    actor_user_id  INTEGER,
    actor_email    TEXT    NOT NULL,
    action         TEXT    NOT NULL,
    target_type    TEXT    NOT NULL,
    target_id      INTEGER,
    summary        TEXT    NOT NULL
);

ALTER TABLE virtual_keys ADD COLUMN user_id INTEGER REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE virtual_keys ADD COLUMN team_id INTEGER REFERENCES teams(id) ON DELETE SET NULL;

CREATE INDEX idx_keys_user ON virtual_keys(user_id);
CREATE INDEX idx_keys_team ON virtual_keys(team_id);
CREATE INDEX idx_sessions_user ON sessions(user_id);
CREATE INDEX idx_tokens_user ON access_tokens(user_id);
CREATE INDEX idx_audit_at ON audit_log(at);
