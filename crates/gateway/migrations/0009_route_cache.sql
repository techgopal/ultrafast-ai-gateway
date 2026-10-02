-- The exact-match response cache of a route: whether it is on, how long an
-- answer is kept, and whom an answer is kept for (`team`, `key` or `user`).
ALTER TABLE routes ADD COLUMN cache_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE routes ADD COLUMN cache_ttl_s   INTEGER NOT NULL DEFAULT 300;
ALTER TABLE routes ADD COLUMN cache_scope   TEXT    NOT NULL DEFAULT 'team'
    CHECK (cache_scope IN ('team', 'key', 'user'));
