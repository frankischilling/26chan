-- API image_limit counts image replies, not the OP attachment.
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE OR REPLACE FUNCTION content.insert_post_attachment(
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
    IF v_limit = 0 OR (p_id <> p_thread AND (SELECT count(*) FROM content.post_media m
        JOIN content.posts p ON p.id = m.post_id WHERE p.board = p_board
        AND p.thread_id = p_thread AND p.id <> p_thread AND NOT p.deleted AND NOT m.file_deleted) >= v_limit) THEN
        RAISE EXCEPTION 'This board or thread cannot accept another image.' USING ERRCODE = 'P0001';
    END IF;
    INSERT INTO content.posts(id, board, thread_id, name, subject, comment)
        VALUES (p_id, p_board, p_thread, p_name, p_subject, p_comment);
    INSERT INTO content.post_media(post_id, job_id, asset_id, filename, bytes, width, height, spoiler)
        VALUES (p_id, p_job, v_asset.id, v_filename, v_asset.bytes, v_asset.width, v_asset.height, p_spoiler);
END
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
