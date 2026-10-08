-- The model catalog: which models each provider offers, which of them are
-- enabled, and who may call them.

CREATE TABLE models (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(provider_id, name)
);

-- A row with neither team nor user grants the model to everyone.
CREATE TABLE model_grants (
    id       INTEGER PRIMARY KEY,
    org_id   INTEGER NOT NULL DEFAULT 1,
    model_id INTEGER NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    team_id  INTEGER REFERENCES teams(id) ON DELETE CASCADE,
    user_id  INTEGER REFERENCES users(id) ON DELETE CASCADE,
    CHECK (team_id IS NULL OR user_id IS NULL)
);

CREATE UNIQUE INDEX model_grants_unique
    ON model_grants (model_id, IFNULL(team_id, 0), IFNULL(user_id, 0));

-- A JSON array of model and route names the key may call; NULL is no
-- allowlist.
ALTER TABLE virtual_keys ADD COLUMN allowed TEXT;

-- The API version of an Azure provider.
ALTER TABLE providers ADD COLUMN api_version TEXT;
