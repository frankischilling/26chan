-- 4chan-old 545b7812: global_config.ini:217–223,290–292 and board overrides.
-- New columns leave existing content and unrelated operator policy untouched.
ALTER TABLE content.boards
    ADD COLUMN deletion_no_op boolean NOT NULL DEFAULT false,
    ADD COLUMN deletion_no_reply boolean NOT NULL DEFAULT false,
    ADD COLUMN deletion_known_min_seconds integer NOT NULL DEFAULT 60,
    ADD COLUMN deletion_unknown_min_seconds integer NOT NULL DEFAULT 600,
    ADD COLUMN deletion_max_seconds integer NOT NULL DEFAULT 1800,
    ADD CONSTRAINT board_public_deletion_bounds CHECK (
        deletion_known_min_seconds BETWEEN 0 AND 86400
        AND deletion_unknown_min_seconds BETWEEN deletion_known_min_seconds AND 86400
        AND deletion_max_seconds BETWEEN 1 AND 86400
        AND deletion_unknown_min_seconds < deletion_max_seconds);
UPDATE content.boards SET deletion_no_op=true WHERE slug IN
    ('a','bant','his','int','jp','pol','pw','qa','qst','sp','tv','v','vip','vm','vmg','vrpg','vst','vt');
-- qa's NO_DELETE_REPLY is commented out. vg's OP veto is in imgboard.php.
-- Runtime roles retain SELECT but never receive policy INSERT/UPDATE grants.
