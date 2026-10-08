-- One user per identity at an external sign-in provider. The columns exist
-- since 0002; password users keep external_id NULL and are not constrained.
CREATE UNIQUE INDEX users_external ON users (org_id, auth_provider, external_id)
    WHERE external_id IS NOT NULL;
