ALTER TABLE content.boards
    ADD COLUMN image_limit integer NOT NULL DEFAULT 0 CHECK (image_limit BETWEEN 0 AND 1001);

-- Rows survive file/post deletion and queue retention. A consumed capability
-- cannot be reused after either cleanup or deletion of its original post.
CREATE TABLE content.post_media (
    post_id bigint PRIMARY KEY REFERENCES content.posts(id),
    job_id text NOT NULL UNIQUE CHECK (job_id ~ '^[0-9a-f]{32}$'),
    asset_id text NOT NULL UNIQUE CHECK (asset_id ~ '^[0-9a-f]{32}$'),
    filename text NOT NULL CHECK (octet_length(filename) BETWEEN 1 AND 255),
    bytes bigint NOT NULL CHECK (bytes BETWEEN 1 AND 5242880),
    width integer NOT NULL CHECK (width BETWEEN 1 AND 1024),
    height integer NOT NULL CHECK (height BETWEEN 1 AND 1024),
    spoiler boolean NOT NULL,
    file_deleted boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON content.post_media FROM PUBLIC;

GRANT USAGE ON SCHEMA content, media, media_intake TO board_attachment_owner;
GRANT SELECT(slug, image_limit) ON content.boards TO board_attachment_owner;
GRANT UPDATE(slug) ON content.boards TO board_attachment_owner;
GRANT SELECT(id, board, deleted, closed, archived_at) ON content.threads TO board_attachment_owner;
GRANT UPDATE(id, modified_at) ON content.threads TO board_attachment_owner;
GRANT SELECT ON content.visible_threads TO board_attachment_owner;
GRANT SELECT(id, board, thread_id, deleted) ON content.posts TO board_attachment_owner;
GRANT INSERT(id, board, thread_id, name, subject, comment) ON content.posts TO board_attachment_owner;
GRANT SELECT, INSERT ON content.post_media TO board_attachment_owner;
GRANT UPDATE(file_deleted) ON content.post_media TO board_attachment_owner;
GRANT SELECT(id, state, filename, created_at) ON media.jobs TO board_attachment_owner;
GRANT UPDATE(id) ON media.jobs TO board_attachment_owner;
GRANT SELECT(job_id, capability_hash) ON media_intake.handles TO board_attachment_owner;
GRANT SELECT(id, job_id, state, bytes, width, height) ON media.assets TO board_attachment_owner;

-- Insert a NEW post, never attach to an existing post whose author the caller
-- might not control. The enclosing transaction owns thread/bump/password writes.
CREATE FUNCTION content.insert_post_attachment(
    p_id bigint, p_board text, p_thread bigint, p_name text, p_subject text,
    p_comment text, p_job text, p_capability text, p_spoiler boolean
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_limit integer;
    v_state text;
    v_created timestamptz;
    v_filename text;
    v_hash bytea;
    v_asset record;
BEGIN
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Attachment insertion requires Read Committed.' USING ERRCODE = '22023';
    END IF;
    IF p_job IS NULL OR octet_length(p_job) <> 32 OR p_job !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$'
       OR p_spoiler IS NULL THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    -- Same lock order as posting and deletion: board, thread, then media job.
    SELECT b.image_limit INTO v_limit FROM content.boards b WHERE b.slug = p_board FOR UPDATE;
    IF v_limit IS NULL THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    PERFORM t.id FROM content.threads t WHERE t.board = p_board AND t.id = p_thread
        AND NOT t.deleted AND NOT t.closed AND t.archived_at IS NULL FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.state, j.created_at, j.filename INTO v_state, v_created, v_filename
        FROM media.jobs j WHERE j.id = p_job FOR UPDATE;
    SELECT h.capability_hash INTO v_hash FROM media_intake.handles h WHERE h.job_id = p_job;
    -- Recheck expiry after the lock wait. Capabilities are never stored in content.
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability, 'UTF8'))
       OR v_created + interval '2 hours' <= clock_timestamp() THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    IF v_state <> 'published' OR EXISTS (SELECT 1 FROM content.post_media m WHERE m.job_id = p_job) THEN
        RAISE EXCEPTION 'Attachment is not ready or was already used.' USING ERRCODE = 'P0001';
    END IF;
    SELECT a.id, a.bytes, a.width, a.height INTO v_asset FROM media.assets a
        WHERE a.job_id = p_job AND a.state = 'approved' ORDER BY a.id LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE = 'P0001';
    END IF;
    IF v_limit = 0 OR (SELECT count(*) FROM content.post_media m
        JOIN content.posts p ON p.id = m.post_id WHERE p.board = p_board
        AND p.thread_id = p_thread AND NOT p.deleted AND NOT m.file_deleted) >= v_limit THEN
        RAISE EXCEPTION 'This board or thread cannot accept another image.' USING ERRCODE = 'P0001';
    END IF;
    INSERT INTO content.posts(id, board, thread_id, name, subject, comment)
        VALUES (p_id, p_board, p_thread, p_name, p_subject, p_comment);
    INSERT INTO content.post_media(post_id, job_id, asset_id, filename, bytes, width, height, spoiler)
        VALUES (p_id, p_job, v_asset.id, v_filename, v_asset.bytes, v_asset.width, v_asset.height, p_spoiler);
END
$$;

-- Caller checks the post deletion password or staff session before this call,
-- just as for whole-post deletion. This function cannot restore a deleted file.
CREATE FUNCTION content.delete_post_attachment(p_board text, p_post bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_thread bigint;
BEGIN
    PERFORM b.slug FROM content.boards b WHERE b.slug = p_board FOR UPDATE;
    SELECT p.thread_id INTO v_thread FROM content.posts p
        JOIN content.visible_threads t ON t.id = p.thread_id AND t.board = p.board
        WHERE p.board = p_board AND p.id = p_post AND NOT p.deleted;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    -- Thread lock also serializes staff whole-thread removal.
    PERFORM t.id FROM content.threads t WHERE t.id = v_thread AND NOT t.deleted FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    UPDATE content.post_media SET file_deleted = true WHERE post_id = p_post AND NOT file_deleted;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    UPDATE content.threads SET modified_at = clock_timestamp() WHERE id = v_thread;
END
$$;

REVOKE ALL ON FUNCTION content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean),
    content.delete_post_attachment(text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean)
    TO board_public;
GRANT EXECUTE ON FUNCTION content.delete_post_attachment(text,bigint) TO board_public, board_staff;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
ALTER FUNCTION content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean)
    OWNER TO board_attachment_owner;
ALTER FUNCTION content.delete_post_attachment(text,bigint) OWNER TO board_attachment_owner;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;

-- Public/staff metadata excludes private job IDs and upload authorization.
CREATE VIEW content.visible_post_media WITH (security_barrier = true) AS
    SELECT m.post_id, m.asset_id, m.filename, m.bytes, m.width, m.height, m.spoiler,
        (m.file_deleted OR a.id IS NULL) AS file_deleted
    FROM content.post_media m JOIN content.posts p ON p.id = m.post_id
    JOIN content.visible_threads t ON t.id = p.thread_id AND t.board = p.board
    LEFT JOIN media.assets a ON a.id = m.asset_id AND a.state = 'approved'
    WHERE NOT p.deleted;
REVOKE ALL ON content.visible_post_media FROM PUBLIC;
GRANT SELECT ON content.visible_post_media TO board_public, board_staff;

-- Both opaque and future legacy media URLs use this reader view. Once attached,
-- an asset is hidden by file deletion, post deletion, or thread disappearance.
-- Unattached approvals remain available to the existing private qualification.
CREATE OR REPLACE VIEW media.approved_assets WITH (security_barrier = true) AS
    SELECT a.id, a.sha256, a.bytes, a.width, a.height FROM media.assets a
    WHERE a.state = 'approved' AND NOT EXISTS (
        SELECT 1 FROM content.post_media m WHERE m.asset_id = a.id AND (
            m.file_deleted OR NOT EXISTS (
                SELECT 1 FROM content.posts p JOIN content.visible_threads t
                    ON t.id = p.thread_id AND t.board = p.board
                WHERE p.id = m.post_id AND NOT p.deleted
            )
        )
    );
