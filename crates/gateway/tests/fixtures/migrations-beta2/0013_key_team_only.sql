-- A key that a non-admin made for another user acts for its team only.
-- Decided when the key is made and kept here, so promoting or demoting
-- anyone later does not change what the key may call.
ALTER TABLE virtual_keys ADD COLUMN team_only INTEGER NOT NULL DEFAULT 0;

-- Keys made before `created_by` was kept: their maker from the audit log,
-- the first `key.create` entry for the key.
UPDATE virtual_keys
SET created_by = (
    SELECT a.actor_user_id FROM audit_log a
    WHERE a.action = 'key.create' AND a.target_type = 'key' AND a.target_id = virtual_keys.id
    ORDER BY a.id LIMIT 1
)
WHERE created_by IS NULL;

-- A key in a team that someone other than its owner made, who is not an
-- admin, is a team key. Whether the maker was an admin when they made it
-- is not recorded: whether they are one now is the best available
-- approximation. A maker who is gone counts as no admin.
UPDATE virtual_keys
SET team_only = 1
WHERE created_by IS NOT NULL
  AND user_id IS NOT NULL
  AND created_by <> user_id
  AND team_id IS NOT NULL
  AND NOT EXISTS (
      SELECT 1 FROM users u
      WHERE u.id = virtual_keys.created_by AND u.role = 'admin'
  );
