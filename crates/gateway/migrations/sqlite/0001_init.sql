CREATE TABLE providers (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    name        TEXT    NOT NULL,
    kind        TEXT    NOT NULL,
    base_url    TEXT    NOT NULL,
    credential  BLOB,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (org_id, name)
);

CREATE TABLE virtual_keys (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    name        TEXT    NOT NULL,
    key_hash    TEXT    NOT NULL UNIQUE,
    display     TEXT    NOT NULL,
    expires_at  TEXT,
    revoked_at  TEXT,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
