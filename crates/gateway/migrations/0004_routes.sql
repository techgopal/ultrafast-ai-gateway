-- Routes: a name a client can call that spreads requests over models, with
-- fallbacks, timeouts and a circuit breaker.

CREATE TABLE routes (
    id                     INTEGER PRIMARY KEY,
    org_id                 INTEGER NOT NULL DEFAULT 1,
    name                   TEXT NOT NULL,
    retries                INTEGER NOT NULL DEFAULT 2,
    first_token_timeout_ms INTEGER NOT NULL DEFAULT 30000,
    total_timeout_ms       INTEGER NOT NULL DEFAULT 300000,
    breaker_failures       INTEGER NOT NULL DEFAULT 5,
    breaker_window_s       INTEGER NOT NULL DEFAULT 60,
    breaker_open_s         INTEGER NOT NULL DEFAULT 30,
    created_at             TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(org_id, name)
);

-- A deleted model leaves its targets with it.
CREATE TABLE route_targets (
    id       INTEGER PRIMARY KEY,
    route_id INTEGER NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
    model_id INTEGER NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    tier     TEXT NOT NULL CHECK (tier IN ('primary','fallback')),
    weight   INTEGER NOT NULL DEFAULT 1,
    position INTEGER NOT NULL
);

CREATE INDEX route_targets_route ON route_targets (route_id);
CREATE INDEX route_targets_model ON route_targets (model_id);

-- No rows: every user may use the route.
CREATE TABLE route_grants (
    route_id INTEGER NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
    team_id  INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    PRIMARY KEY (route_id, team_id)
);
