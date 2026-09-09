ALTER TABLE staff_identity.sessions ADD COLUMN last_activity_at timestamptz;
UPDATE staff_identity.sessions SET last_activity_at=authenticated_at WHERE last_activity_at IS NULL;
ALTER TABLE staff_identity.sessions ALTER COLUMN last_activity_at SET NOT NULL;
ALTER TABLE staff_identity.sessions ALTER COLUMN last_activity_at SET DEFAULT clock_timestamp();
GRANT UPDATE(last_activity_at) ON staff_identity.sessions TO board_auth;
