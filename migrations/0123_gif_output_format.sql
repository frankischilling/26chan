-- Format is durable authority for the host-encoded file and its fixed suffix.
-- Existing output and thumbnail bytes remain PNG.
ALTER TABLE media.assets
    ADD COLUMN output_format text NOT NULL DEFAULT 'png'
        CHECK (output_format IN ('png','gif')),
    DROP CONSTRAINT assets_bytes_check,
    ADD CONSTRAINT assets_bytes_check CHECK (
        bytes BETWEEN 1 AND CASE output_format WHEN 'gif' THEN 20971520 ELSE 5242880 END),
    ADD CONSTRAINT gif_manifest_check CHECK (
        output_format<>'gif' OR (md5 IS NOT NULL AND thumbnail_sha256 IS NOT NULL
            AND source_profile IS NULL));
ALTER TABLE media.jobs DROP CONSTRAINT jobs_output_bytes_check,
    ADD CONSTRAINT jobs_output_bytes_check CHECK (output_bytes BETWEEN 1 AND 20971520);
ALTER TABLE content.post_media DROP CONSTRAINT post_media_bytes_check,
    ADD CONSTRAINT post_media_bytes_check CHECK (bytes BETWEEN 1 AND 20971520);

CREATE FUNCTION media.guard_output_format() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
    IF NEW.output_format IS DISTINCT FROM OLD.output_format THEN
        RAISE EXCEPTION 'Media reservation format is immutable.' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_output_format() FROM PUBLIC;
CREATE TRIGGER media_output_format_immutable BEFORE UPDATE ON media.assets
    FOR EACH ROW EXECUTE FUNCTION media.guard_output_format();

CREATE OR REPLACE VIEW content.visible_post_media WITH (security_barrier=true) AS
    SELECT m.post_id,m.asset_id,m.filename,m.bytes,m.width,m.height,m.spoiler,
        (m.file_deleted OR a.id IS NULL) AS file_deleted,
        m.tim,encode(decode(a.md5,'hex'),'base64') AS md5,
        a.thumbnail_width,a.thumbnail_height,coalesce(a.output_format,'png') AS output_format
    FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
    JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board
    LEFT JOIN media.assets a ON a.id=m.asset_id AND a.state='approved'
    WHERE NOT p.deleted;

CREATE OR REPLACE VIEW media.approved_assets WITH (security_barrier=true) AS
    SELECT a.id,a.sha256,a.bytes,a.width,a.height,
        a.thumbnail_sha256,a.thumbnail_bytes,a.thumbnail_width,a.thumbnail_height,
        a.output_format
    FROM media.assets a WHERE a.state='approved' AND NOT EXISTS (
        SELECT 1 FROM content.post_media m WHERE m.asset_id=a.id AND (
            m.file_deleted OR NOT EXISTS (
                SELECT 1 FROM content.posts p JOIN content.visible_threads t
                    ON t.id=p.thread_id AND t.board=p.board WHERE p.id=m.post_id AND NOT p.deleted
            )
        )
    );
CREATE OR REPLACE VIEW media.approved_post_assets WITH (security_barrier=true) AS
    SELECT p.board,m.tim,a.* FROM content.post_media m
    JOIN content.posts p ON p.id=m.post_id
    JOIN media.approved_assets a ON a.id=m.asset_id;

CREATE OR REPLACE VIEW content.staff_post_media WITH (security_barrier=true) AS
    SELECT m.post_id,m.filename,m.bytes,m.width,m.height,m.spoiler,m.tim,
        a.thumbnail_width,a.thumbnail_height,
        (NOT m.file_deleted AND NOT p.deleted AND a.id IS NOT NULL
         AND EXISTS (SELECT 1 FROM content.visible_threads t
                     WHERE t.id=p.thread_id AND t.board=p.board)) AS available,
        CASE WHEN NOT m.file_deleted AND NOT p.deleted AND a.id IS NOT NULL
             AND EXISTS (SELECT 1 FROM content.visible_threads t
                         WHERE t.id=p.thread_id AND t.board=p.board
                           AND (t.archived_at IS NULL OR t.archive_expires_at>clock_timestamp()))
             AND a.md5 ~ '^[0-9a-f]{32}$' AND octet_length(a.md5)=32
             THEN a.md5 END AS md5,coalesce(a.output_format,'png') AS output_format
    FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
    LEFT JOIN media.assets a ON a.id=m.asset_id AND a.state='approved';

GRANT SELECT(output_format) ON media.assets TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE OR REPLACE FUNCTION content.consume_staff_attachment_receipt(
    p_id bigint,p_board text,p_thread bigint,p_job text,p_capability_hash bytea,
    p_spoiler boolean,p_authorized boolean
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE b record; t record; j record; a record; v_hash bytea;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Attachment insertion requires Read Committed.' USING ERRCODE='22023';
    END IF;
    IF p_job IS NULL OR octet_length(p_job)<>32 OR p_job !~ '^[0-9a-f]{32}$'
        OR p_capability_hash IS NULL OR octet_length(p_capability_hash)<>32
        OR p_spoiler IS NULL OR p_authorized IS NULL THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    SELECT image_limit,text_only,staff_only,upload_board,comment_spoiler_cleanup INTO b
        FROM content.boards WHERE slug=p_board FOR UPDATE;
    IF NOT FOUND OR b.staff_only OR b.image_limit=0 THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    SELECT sticky,undead,closed INTO t FROM content.threads
        WHERE board=p_board AND id=p_thread AND NOT deleted AND archived_at IS NULL FOR UPDATE;
    IF NOT FOUND OR (t.closed AND NOT p_authorized) THEN
        RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=p_id AND board=p_board
        AND thread_id=p_thread AND NOT deleted) THEN
        RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF p_id<>p_thread AND (b.text_only OR b.upload_board) THEN
        RAISE EXCEPTION 'You cannot upload files on this board' USING ERRCODE='23514';
    END IF;
    SELECT state,created_at,filename,input_bytes INTO j FROM media.jobs WHERE id=p_job FOR UPDATE;
    SELECT capability_hash INTO v_hash FROM media_intake.handles WHERE job_id=p_job;
    -- Check the wall clock after waiting for the job, not the statement clock.
    IF v_hash IS NULL OR v_hash<>p_capability_hash OR j.created_at IS NULL
        OR j.created_at+interval '2 hours'<=clock_timestamp() THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF j.state<>'published' OR EXISTS(SELECT 1 FROM content.post_media WHERE job_id=p_job) THEN
        RAISE EXCEPTION 'Attachment is not ready or was already used.' USING ERRCODE='P0001';
    END IF;
    SELECT id,bytes,width,height,output_format INTO a FROM media.assets
        WHERE job_id=p_job AND state='approved' ORDER BY id LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE='P0001';
    END IF;
    -- Staff rank never bypasses the existing intake and published-file bounds.
    IF j.input_bytes IS NULL OR j.input_bytes NOT BETWEEN 1 AND 8388608
        OR a.bytes NOT BETWEEN 1 AND (CASE a.output_format WHEN 'gif' THEN 20971520 ELSE 5242880 END) THEN
        RAISE EXCEPTION 'Attachment is too large.' USING ERRCODE='22023';
    END IF;
    IF NOT p_authorized AND NOT t.sticky AND NOT t.undead AND p_id<>p_thread
        AND (SELECT count(*) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
            WHERE p.board=p_board AND p.thread_id=p_thread AND p.id<>p_thread
            AND NOT p.deleted AND NOT m.file_deleted)>=b.image_limit THEN
        RAISE EXCEPTION 'This board or thread cannot accept another image.' USING ERRCODE='P0001';
    END IF;
    INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
        VALUES(p_id,p_job,a.id,j.filename,a.bytes,a.width,a.height,p_spoiler AND b.comment_spoiler_cleanup);
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
