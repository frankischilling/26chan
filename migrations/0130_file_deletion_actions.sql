-- Source imgboard.php:2545-2582, 2613-2667; json.php:380-406.
-- File deletion is a live-post action, including repeats and text-only posts.
-- Existing media flags remain effective; no historical sweep or fake media.
ALTER TABLE content.posts ADD COLUMN file_deleted boolean NOT NULL DEFAULT false;
GRANT SELECT(file_deleted),UPDATE(file_deleted) ON content.posts TO board_attachment_owner;

-- Extend 0129's canonical erased payload without rewriting migration history.
ALTER TABLE content.posts DROP CONSTRAINT posts_erased_payload;
ALTER TABLE content.posts ADD CONSTRAINT posts_erased_payload CHECK (
    NOT content_erased OR (deleted AND NOT file_deleted AND name='' AND subject='' AND comment=''
        AND trip IS NULL AND poster_id IS NULL AND json_op_poster_id IS NULL AND capcode IS NULL
        AND country IS NULL AND country_name IS NULL AND board_flag IS NULL AND flag_name IS NULL
        AND wordfilter_payload IS NULL AND wordfilter_search IS NULL
        AND drawing_time_seconds IS NULL AND drawing_source_post_id IS NULL
        AND dice_result IS NULL AND fortune_text IS NULL AND fortune_color IS NULL
        AND comment_format=0 AND NOT staff_authorized_limits AND NOT image_spoiler
        AND board_flag_type='pol' AND created_at='epoch'::timestamptz));

GRANT CREATE ON SCHEMA post_secrets TO board_content_erasure_owner;
SET ROLE board_content_erasure_owner;
CREATE OR REPLACE FUNCTION post_secrets.guard_post_erasure() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE parent_erased boolean; invoker text;
BEGIN
    IF TG_OP='INSERT' THEN
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content insertion requires Read Committed.' USING ERRCODE='22023';
        END IF;
        IF NEW.content_erased OR NEW.deleted THEN
            RAISE EXCEPTION 'Cannot insert deleted content.' USING ERRCODE='23514';
        END IF;
        -- Serialize even direct runtime inserts with thread erasure. Existing
        -- posting already holds this lock before thread/post/secret tuples.
        PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE;
        IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
        IF EXISTS(SELECT 1 FROM content.threads t WHERE t.board=NEW.board
            AND t.id=NEW.thread_id AND t.content_erased) THEN
            RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    -- Post numbers are durable identities, including media consumption links.
    IF NEW.id<>OLD.id THEN
        RAISE EXCEPTION 'Post identity cannot be reassigned.' USING ERRCODE='23514';
    END IF;
    IF NEW.board<>OLD.board OR NEW.thread_id<>OLD.thread_id THEN
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content reassignment requires Read Committed.' USING ERRCODE='22023';
        END IF;
        PERFORM b.slug FROM content.boards b WHERE b.slug IN(OLD.board,NEW.board) ORDER BY b.slug FOR UPDATE NOWAIT;
        IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
        IF EXISTS(SELECT 1 FROM content.threads t WHERE t.board=NEW.board
            AND t.id=NEW.thread_id AND t.content_erased) THEN
            RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE='23514';
        END IF;
    END IF;
    IF OLD.content_erased THEN
        IF NOT NEW.content_erased OR NOT NEW.deleted OR NEW.id<>OLD.id
            OR NEW.board<>OLD.board OR NEW.thread_id<>OLD.thread_id
            OR to_jsonb(NEW) IS DISTINCT FROM to_jsonb(OLD) THEN
            RAISE EXCEPTION 'Erased content cannot be restored.' USING ERRCODE='23514';
        END IF;
    ELSE
        IF current_setting('transaction_isolation')<>'read committed'
            AND ((NOT OLD.deleted AND NEW.deleted) OR NEW.content_erased) THEN
            RAISE EXCEPTION 'Content erasure requires Read Committed.' USING ERRCODE='22023';
        END IF;
        -- A tuple-first operator write must fail rather than wait backwards.
        IF NEW.deleted THEN
            PERFORM b.slug FROM content.boards b WHERE b.slug=OLD.board FOR UPDATE NOWAIT;
            IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
            SELECT t.content_erased INTO parent_erased FROM content.threads t
                WHERE t.board=OLD.board AND t.id=OLD.thread_id;
        END IF;
        IF NOT ((NOT OLD.deleted AND NEW.deleted) OR (NEW.deleted AND coalesce(parent_erased,false))) THEN
            IF NEW.content_erased THEN
                RAISE EXCEPTION 'Erasure requires a whole-deletion transition.' USING ERRCODE='23514';
            END IF;
            RETURN NEW;
        END IF;
        IF current_setting('transaction_isolation')<>'read committed' THEN
            RAISE EXCEPTION 'Content erasure requires Read Committed.' USING ERRCODE='22023';
        END IF;
        IF NEW.id<>OLD.id OR NEW.board<>OLD.board OR NEW.thread_id<>OLD.thread_id THEN
            RAISE EXCEPTION 'Deletion cannot reassign content.' USING ERRCODE='23514';
        END IF;
        invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
        IF EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=OLD.board AND b.staff_only)
            AND invoker NOT IN ('board_staff','board_migrator') THEN
            RAISE EXCEPTION 'Content is unavailable.' USING ERRCODE='42501';
        END IF;
        DELETE FROM post_secrets.deletion WHERE post_id=OLD.id;
        DELETE FROM post_secrets.anonymous_posts WHERE post_id=OLD.id;
        DELETE FROM post_secrets.op_replies WHERE post_id=OLD.id;
        DELETE FROM post_secrets.poster_contexts WHERE post_id=OLD.id;
        DELETE FROM post_secrets.posting_history WHERE post_id=OLD.id;
        DELETE FROM staff_identity.discussion_posts WHERE post_id=OLD.id;
        IF OLD.id=OLD.thread_id THEN DELETE FROM post_secrets.op_peers WHERE thread_id=OLD.id; END IF;
    END IF;
    NEW.content_erased:=true;
    NEW.name:=''; NEW.subject:=''; NEW.comment:='';
    NEW.trip:=NULL; NEW.poster_id:=NULL; NEW.json_op_poster_id:=NULL; NEW.capcode:=NULL;
    NEW.country:=NULL; NEW.country_name:=NULL; NEW.board_flag:=NULL; NEW.flag_name:=NULL;
    NEW.wordfilter_payload:=NULL; NEW.wordfilter_search:=NULL;
    NEW.drawing_time_seconds:=NULL; NEW.drawing_source_post_id:=NULL;
    NEW.dice_result:=NULL; NEW.fortune_text:=NULL; NEW.fortune_color:=NULL;
    NEW.file_deleted:=false;
    NEW.comment_format:=0; NEW.staff_authorized_limits:=false; NEW.image_spoiler:=false;
    NEW.board_flag_type:='pol'; NEW.created_at:='epoch'::timestamptz;
    RETURN NEW;
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_content_erasure_owner;

GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET ROLE board_attachment_owner;
CREATE OR REPLACE FUNCTION content.delete_post_attachment(p_board text,p_post bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_thread bigint; v_invoker text;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'File deletion requires Read Committed.' USING ERRCODE='22023';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
        RAISE EXCEPTION 'File deletion is unavailable.' USING ERRCODE='42501';
    END IF;
    -- Retain the attachment owner's RLS boundary, including private boards.
    -- Caller already checked password/session or current administrative authority.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='P0002'; END IF;
    SELECT p.thread_id INTO v_thread FROM content.posts p
        WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='P0002'; END IF;
    -- Board -> thread -> post; recheck visibility and wall-clock expiry after waits.
    PERFORM t.id FROM content.threads t JOIN content.visible_threads v
        ON v.id=t.id AND v.board=t.board
        WHERE t.board=p_board AND t.id=v_thread AND NOT t.deleted FOR UPDATE OF t;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='P0002'; END IF;
    PERFORM p.id FROM content.posts p WHERE p.board=p_board AND p.id=p_post
        AND p.thread_id=v_thread AND NOT p.deleted FOR UPDATE;
    IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM content.visible_threads v
        WHERE v.board=p_board AND v.id=v_thread
          AND (v.archived_at IS NULL OR v.archive_expires_at>clock_timestamp())) THEN
        RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='P0002';
    END IF;
    UPDATE content.posts SET file_deleted=true WHERE board=p_board AND id=p_post;
    UPDATE content.post_media SET file_deleted=true WHERE post_id=p_post;
    -- Also refresh the OP for reply actions; every successful repeat is a mutation.
    UPDATE content.threads SET modified_at=clock_timestamp() WHERE board=p_board AND id=v_thread;
END $$;

CREATE OR REPLACE FUNCTION content.staff_delete_post_attachment(p_board text,p_post bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Staff attachment removal is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report retirement requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Explicit administrative action. Source staff-self requires password/host
    -- proof; account authorship is not such proof and is not inferred here.
    PERFORM content.delete_post_attachment(p_board,p_post);
    PERFORM post_secrets.retire_staff_file_report_membership(p_board,p_post);
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;

GRANT CREATE ON SCHEMA post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
CREATE OR REPLACE FUNCTION post_secrets.retire_staff_file_report_membership(p_board text,p_post bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Staff report retirement is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report retirement requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- The wrapper already owns board -> thread -> post. No runtime receives
    -- private DELETE or EXECUTE; the existing narrow owner bridge is retained.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE NOWAIT;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report board is unavailable.' USING ERRCODE='23514'; END IF;
    DELETE FROM content.reports WHERE board=p_board AND post_id=p_post;
    -- Cascades retire membership, anonymous ownership and weight evidence;
    -- membership retirement removes the empty aggregate. Target report rows and
    -- their clear history are removed; independent moderation audit remains.
    -- Include old reports with no membership and all report disposition states.
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_report_admission_owner;

CREATE OR REPLACE VIEW content.visible_post_media WITH (security_barrier=true) AS
    SELECT m.post_id,m.asset_id,m.filename,m.bytes,m.width,m.height,m.spoiler,
        (p.file_deleted OR m.file_deleted OR a.id IS NULL) AS file_deleted,
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
                    ON t.id=p.thread_id AND t.board=p.board WHERE p.id=m.post_id AND NOT p.deleted AND NOT p.file_deleted
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
        (NOT m.file_deleted AND NOT p.file_deleted AND NOT p.deleted AND a.id IS NOT NULL
         AND EXISTS (SELECT 1 FROM content.visible_threads t
                     WHERE t.id=p.thread_id AND t.board=p.board)) AS available,
        CASE WHEN NOT m.file_deleted AND NOT p.file_deleted AND NOT p.deleted AND a.id IS NOT NULL
             AND EXISTS (SELECT 1 FROM content.visible_threads t
                         WHERE t.id=p.thread_id AND t.board=p.board
                           AND (t.archived_at IS NULL OR t.archive_expires_at>clock_timestamp()))
             AND a.md5 ~ '^[0-9a-f]{32}$' AND octet_length(a.md5)=32
             THEN a.md5 END AS md5,coalesce(a.output_format,'png') AS output_format
    FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
    LEFT JOIN media.assets a ON a.id=m.asset_id AND a.state='approved';
