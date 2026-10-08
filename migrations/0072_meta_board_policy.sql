-- META_BOARD controls public projections separately from JANITOR_BOARD access.
-- The pinned global default and every supplied board override leave it off.
ALTER TABLE content.boards ADD COLUMN meta_board boolean NOT NULL DEFAULT false;
