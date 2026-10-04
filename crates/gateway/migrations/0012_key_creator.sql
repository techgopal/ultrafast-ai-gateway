-- Who made the key: the user whose session or token created it. NULL for a
-- key made before this column and for a key made by the CLI. A key a
-- non-admin made for another user acts for its team only (see access.rs).
-- No foreign key on purpose: a creator who is deleted must not turn such a
-- key into one with its owner's full reach.
ALTER TABLE virtual_keys ADD COLUMN created_by INTEGER;
