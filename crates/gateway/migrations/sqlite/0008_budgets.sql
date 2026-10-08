-- Spending budgets of the gateway, a team, a user or a key, per UTC
-- calendar period. `block` refuses calls once the amount is spent; `alert`
-- allows them and writes one audit row per period.
CREATE TABLE budgets (
    id            INTEGER PRIMARY KEY,
    org_id        INTEGER NOT NULL DEFAULT 1,
    scope         TEXT    NOT NULL CHECK (scope IN ('gateway', 'team', 'user', 'key')),
    -- NULL for the gateway, the id of the team, user or key otherwise.
    scope_id      INTEGER,
    amount_micros INTEGER NOT NULL CHECK (amount_micros > 0),
    period        TEXT    NOT NULL CHECK (period IN ('daily', 'weekly', 'monthly')),
    action        TEXT    NOT NULL CHECK (action IN ('block', 'alert')),
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (org_id, scope, scope_id, period),
    CHECK ((scope = 'gateway') = (scope_id IS NULL))
);

-- NULLs are distinct in a UNIQUE constraint, so the gateway's budget of a
-- period needs this index to stay single.
CREATE UNIQUE INDEX idx_budgets_subject ON budgets (org_id, scope, COALESCE(scope_id, 0), period);

-- What a budget has spent in one period. It is a cache of what the gateway
-- counted: the request logs are the source of truth and the counters are
-- rebuilt from them at start. `period_start` is the UTC date the period
-- began on, as YYYY-MM-DD.
CREATE TABLE budget_usage (
    budget_id    INTEGER NOT NULL REFERENCES budgets (id) ON DELETE CASCADE,
    period_start TEXT    NOT NULL,
    spent_micros INTEGER NOT NULL DEFAULT 0,
    -- 1 once the alert of this period was written.
    alerted      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (budget_id, period_start)
);

-- The scope is not a foreign key (it points at one of three tables), so
-- what a deleted team or user leaves behind is removed here.
CREATE TRIGGER budgets_team_deleted AFTER DELETE ON teams
BEGIN
    DELETE FROM budgets WHERE scope = 'team' AND scope_id = OLD.id;
END;

CREATE TRIGGER budgets_user_deleted AFTER DELETE ON users
BEGIN
    DELETE FROM budgets WHERE scope = 'user' AND scope_id = OLD.id;
END;
