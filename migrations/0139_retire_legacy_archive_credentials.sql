-- archive_thread clears deletion passwords on the OP and every reply. 0091
-- enforces that rule for new archive transitions; reconcile older archives now.
-- This retires password authority, not post bodies, audit history or backups.
DO $retire_legacy_archive_credentials$
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Archive credential retirement requires Read Committed.'
            USING ERRCODE='22023';
    END IF;
    -- Existing deletion guards take NOWAIT board locks after the secret tuple.
    -- Acquire every affected board first, in their shared canonical order.
    PERFORM b.slug FROM content.boards b
    WHERE EXISTS (
        SELECT 1 FROM content.threads t JOIN content.posts p
            ON p.board=t.board AND p.thread_id=t.id
        JOIN post_secrets.deletion d ON d.post_id=p.id
        WHERE t.board=b.slug AND t.archived_at IS NOT NULL
    ) ORDER BY b.slug FOR UPDATE;
    DELETE FROM post_secrets.deletion d USING content.posts p,content.threads t
    WHERE d.post_id=p.id AND t.board=p.board AND t.id=p.thread_id
        AND t.archived_at IS NOT NULL;
END $retire_legacy_archive_credentials$;
