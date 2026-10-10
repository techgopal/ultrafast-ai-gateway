-- What an invite link is for: a new user choosing a password ('invite'), or a
-- user who signs in through the identity provider getting a password too
-- ('set_password', made by an admin). This is SQLite migration 0020.
ALTER TABLE invites ADD COLUMN kind TEXT COLLATE "C" NOT NULL DEFAULT 'invite' CHECK (kind IN ('invite', 'set_password'));
