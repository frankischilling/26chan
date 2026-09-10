ALTER TABLE content.boards
    ADD COLUMN archive_retention_seconds integer NOT NULL DEFAULT 0
        CHECK (archive_retention_seconds BETWEEN 0 AND 2592000),
    ADD COLUMN archive_limit integer NOT NULL DEFAULT 1000
        CHECK (archive_limit BETWEEN 1 AND 1000);

ALTER TABLE content.threads
    ADD COLUMN archived_at timestamptz,
    ADD COLUMN archive_expires_at timestamptz,
    ADD CONSTRAINT thread_archive_times CHECK (
        (archived_at IS NULL AND archive_expires_at IS NULL) OR
        (archived_at IS NOT NULL AND archive_expires_at IS NOT NULL
         AND archive_expires_at > archived_at AND NOT sticky)
    );
CREATE INDEX thread_active_order ON content.threads(board, sticky DESC, bumped_at DESC, id DESC)
    WHERE NOT deleted AND archived_at IS NULL;
CREATE INDEX thread_archive_order ON content.threads(board, archived_at, id)
    WHERE NOT deleted AND archived_at IS NOT NULL;

CREATE VIEW content.visible_threads WITH (security_barrier = true) AS
    SELECT t.* FROM content.threads t JOIN content.boards b ON b.slug=t.board
    WHERE NOT t.deleted AND (t.archived_at IS NULL OR
        (b.archive_retention_seconds > 0 AND t.archive_expires_at > transaction_timestamp()));
REVOKE ALL ON content.visible_threads FROM PUBLIC;
GRANT SELECT ON content.visible_threads TO board_public;
GRANT UPDATE(archived_at, archive_expires_at) ON content.threads TO board_public;
-- Closed/sticky and board policy remain outside public mutation authority.
