-- Keep sticky as the authoritative protection/admission flag. Rank affects
-- active display order only; existing rows and legacy writes default to zero.
-- Use an explicit bounded integer instead of the source timestamp encoding,
-- including rank 60 whose legacy timestamp persistence is not established.
ALTER TABLE content.threads
    ADD COLUMN sticky_rank smallint NOT NULL DEFAULT 0
        CHECK (sticky_rank BETWEEN 0 AND 60);
GRANT UPDATE(sticky_rank) ON content.threads TO board_staff;

-- CREATE OR REPLACE appends the column without dropping the view, its grants,
-- or dependents. Preserve the existing visibility predicate and security barrier.
CREATE OR REPLACE VIEW content.visible_threads WITH (security_barrier = true) AS
    SELECT t.* FROM content.threads t JOIN content.boards b ON b.slug=t.board
    WHERE (NOT b.staff_only OR current_user IN ('board_staff','board_migrator'))
      AND NOT t.deleted AND (t.archived_at IS NULL OR
        (b.archive_retention_seconds > 0 AND t.archive_expires_at > transaction_timestamp()));

DROP INDEX content.thread_board_order;
CREATE INDEX thread_board_order ON content.threads
    (board, sticky DESC, (CASE WHEN sticky THEN sticky_rank ELSE 0 END) DESC, bumped_at DESC, id DESC)
    WHERE NOT deleted;
DROP INDEX content.thread_active_order;
CREATE INDEX thread_active_order ON content.threads
    (board, sticky DESC, (CASE WHEN sticky THEN sticky_rank ELSE 0 END) DESC, bumped_at DESC, id DESC)
    WHERE NOT deleted AND archived_at IS NULL;

-- Historical isolated actions remain valid and retain NULL masks. Existing
-- board_staff table-level INSERT already covers these new columns; narrow
-- staff-post-owner column grants do not expand.
ALTER TABLE content.moderation_audit
    ADD COLUMN before_mask smallint,
    ADD COLUMN after_mask smallint,
    ADD CONSTRAINT moderation_audit_thread_options_masks CHECK (
        (action = 'thread-options' AND before_mask IS NOT NULL AND after_mask IS NOT NULL
         AND before_mask BETWEEN 0 AND 31 AND after_mask BETWEEN 0 AND 31
         AND before_mask <> after_mask)
        OR (action <> 'thread-options' AND before_mask IS NULL AND after_mask IS NULL)
    );
ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check
    CHECK(action IN ('close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
        'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post','spoiler','unspoiler',
        'undead','unundead','thread-options'));

-- Forward-only additive migration: do not remove rank or grouped audit evidence
-- when rolling application code back. Older readers continue to ignore rank.
