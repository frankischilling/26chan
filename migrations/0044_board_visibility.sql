ALTER TABLE content.boards
    ADD COLUMN source_order integer NOT NULL DEFAULT 1000 CHECK(source_order BETWEEN 0 AND 1000),
    ADD COLUMN catalog_enabled boolean NOT NULL DEFAULT true,
    ADD COLUMN json_enabled boolean NOT NULL DEFAULT true,
    ADD COLUMN staff_only boolean NOT NULL DEFAULT false,
    ADD COLUMN upload_board boolean NOT NULL DEFAULT false;
ALTER TABLE content.boards DROP CONSTRAINT boards_threads_per_page_check;
ALTER TABLE content.boards ADD CONSTRAINT boards_threads_per_page_check
    CHECK(threads_per_page BETWEEN 1 AND 30);

-- Private board content stays outside public and attachment credentials even
-- when those credentials issue their own queries rather than using handlers.
ALTER TABLE content.boards ENABLE ROW LEVEL SECURITY;
CREATE POLICY board_visibility ON content.boards USING (
    NOT staff_only OR current_user IN ('board_staff','board_migrator')
);
ALTER TABLE content.threads ENABLE ROW LEVEL SECURITY;
CREATE POLICY thread_visibility ON content.threads USING (
    EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=threads.board)
);
ALTER TABLE content.posts ENABLE ROW LEVEL SECURITY;
CREATE POLICY post_visibility ON content.posts USING (
    EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=posts.board)
);
ALTER TABLE content.reports ENABLE ROW LEVEL SECURITY;
CREATE POLICY report_visibility ON content.reports USING (
    EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=reports.board)
);
ALTER TABLE post_secrets.deletion ENABLE ROW LEVEL SECURITY;
CREATE POLICY deletion_visibility ON post_secrets.deletion USING (
    EXISTS(SELECT 1 FROM content.posts p WHERE p.id=deletion.post_id)
);

-- A view's owner can bypass RLS, so enforce visibility inside this shared
-- projection too. Media projections already join this view.
CREATE OR REPLACE VIEW content.visible_threads WITH (security_barrier = true) AS
    SELECT t.* FROM content.threads t JOIN content.boards b ON b.slug=t.board
    WHERE (NOT b.staff_only OR current_user IN ('board_staff','board_migrator'))
      AND NOT t.deleted AND (t.archived_at IS NULL OR
        (b.archive_retention_seconds > 0 AND t.archive_expires_at > transaction_timestamp()));
