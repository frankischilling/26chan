ALTER TABLE content.threads
    ADD COLUMN permasage boolean NOT NULL DEFAULT false,
    ADD COLUMN permaage boolean NOT NULL DEFAULT false;
GRANT UPDATE(permasage, permaage) ON content.threads TO board_staff;
-- Public INSERT/UPDATE grants remain column-scoped and exclude both flags.
-- Refresh the expanded projection; CREATE VIEW's original t.* was frozen.
CREATE OR REPLACE VIEW content.visible_threads WITH (security_barrier = true) AS
    SELECT t.* FROM content.threads t JOIN content.boards b ON b.slug=t.board
    WHERE NOT t.deleted AND (t.archived_at IS NULL OR
        (b.archive_retention_seconds > 0 AND t.archive_expires_at > transaction_timestamp()));
REVOKE ALL ON content.visible_threads FROM PUBLIC;
GRANT SELECT ON content.visible_threads TO board_public;

ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check
    CHECK (action IN ('close','reopen','sticky','unsticky','permasage','unpermasage',
                     'permaage','unpermaage','remove-post','remove-thread','remove-file',
                     'resolve','dismiss'));
