-- Source video policy and immutable isolated-processing metadata.
-- Historical files and completed reservations keep their exact bytes and fields.
DO $video_transfer_preflight$
BEGIN
    IF (pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NULL)
        <> (pg_catalog.to_regprocedure('content.text_thread_transfer_ready()') IS NULL) THEN
        RAISE EXCEPTION 'Text transfer catalog is incomplete before video policy.';
    END IF;
    IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
        EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
        PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
        IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Text transfer catalog is not ready for video policy.';
        END IF;
        EXECUTE 'RESET ROLE';
    END IF;
END $video_transfer_preflight$;

ALTER TABLE content.boards
    ADD COLUMN video_enabled boolean NOT NULL DEFAULT false,
    ADD COLUMN video_audio boolean NOT NULL DEFAULT false,
    ADD COLUMN max_video_bytes bigint NOT NULL DEFAULT 4194304 CHECK(max_video_bytes BETWEEN 1 AND 8388608),
    ADD COLUMN max_video_duration integer NOT NULL DEFAULT 120 CHECK(max_video_duration BETWEEN 1 AND 400),
    ADD COLUMN max_video_dimension integer NOT NULL DEFAULT 2048 CHECK(max_video_dimension BETWEEN 1 AND 2048);
UPDATE content.boards SET video_enabled=true WHERE slug IN ('a','aco','c','d','e','f','g','gif','h','his','hr','k','m','n','o','p','r','s','t','u','v','w','wg','i','ic','cm','y','an','cgl','ck','co','fa','fit','jp','mlp','mu','po','sp','tg','toy','trv','tv','x','b','soc','r9k','test','adv','lit','int','sci','3','vp','diy','pol','hc','vg','hm','j','wsg','out','lgbt','vr','gd','s4s','biz','qa','trash','news','wsr','qst','bant','vip','vrpg','vmg','vst','vt','vm','pw','xs','asp','qb');
UPDATE content.boards SET video_audio=true WHERE slug IN ('gif','r','test','wsg','wsr');
UPDATE content.boards SET max_video_bytes=2097152 WHERE slug IN ('b','bant');
UPDATE content.boards SET max_video_bytes=6291456,max_video_duration=400 WHERE slug='wsg';
UPDATE content.boards SET max_video_duration=300 WHERE slug='gif';
ALTER TABLE media.assets
    ADD COLUMN video_duration_micros bigint,
    ADD COLUMN video_has_audio boolean,
    DROP CONSTRAINT assets_output_format_check,
    ADD CONSTRAINT assets_output_format_check CHECK(output_format IN ('png','gif','pdf','webm','mp4')),
    DROP CONSTRAINT assets_bytes_check,
    ADD CONSTRAINT assets_bytes_check CHECK(bytes BETWEEN 1 AND CASE
        WHEN output_format='gif' THEN 20971520 WHEN output_format IN ('pdf','webm','mp4') THEN 8388608 ELSE 5242880 END),
    DROP CONSTRAINT assets_width_check,
    DROP CONSTRAINT assets_height_check,
    ADD CONSTRAINT assets_width_check CHECK(width BETWEEN 1 AND CASE WHEN output_format IN ('webm','mp4') THEN 2048 ELSE 1024 END),
    ADD CONSTRAINT assets_height_check CHECK(height BETWEEN 1 AND CASE WHEN output_format IN ('webm','mp4') THEN 2048 ELSE 1024 END),
    ADD CONSTRAINT video_manifest_check CHECK (
        (output_format IN ('webm','mp4') AND video_duration_micros IS NOT NULL
         AND video_duration_micros BETWEEN 1 AND 400000000 AND video_has_audio IS NOT NULL
         AND md5 IS NOT NULL AND thumbnail_sha256 IS NOT NULL AND source_profile IS NULL)
        OR (output_format NOT IN ('webm','mp4') AND video_duration_micros IS NULL AND video_has_audio IS NULL));
ALTER TABLE media.assets
    DROP CONSTRAINT normalized_manifest,
    ADD CONSTRAINT normalized_manifest CHECK (
        (md5 IS NULL AND thumbnail_sha256 IS NULL AND thumbnail_bytes IS NULL
         AND thumbnail_width IS NULL AND thumbnail_height IS NULL)
        OR (md5 IS NOT NULL AND md5 ~ '^[0-9a-f]{32}$' AND octet_length(md5)=32
         AND thumbnail_sha256 IS NOT NULL AND thumbnail_sha256 ~ '^[0-9a-f]{64}$' AND octet_length(thumbnail_sha256)=64
         AND thumbnail_bytes IS NOT NULL AND thumbnail_bytes BETWEEN 1 AND 5242880
         AND thumbnail_width IS NOT NULL AND thumbnail_width BETWEEN 1 AND 250 AND (output_format IN ('pdf','webm','mp4') OR thumbnail_width <= width)
         AND thumbnail_height IS NOT NULL AND thumbnail_height BETWEEN 1 AND 250 AND (output_format IN ('pdf','webm','mp4') OR thumbnail_height <= height))
    );
ALTER TABLE content.post_media
    DROP CONSTRAINT post_media_width_check, DROP CONSTRAINT post_media_height_check,
    ADD CONSTRAINT post_media_width_check CHECK(width BETWEEN 1 AND 2048),
    ADD CONSTRAINT post_media_height_check CHECK(height BETWEEN 1 AND 2048);
CREATE FUNCTION media.guard_video_metadata() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (NEW.video_duration_micros,NEW.video_has_audio) IS DISTINCT FROM
           (OLD.video_duration_micros,OLD.video_has_audio) THEN
            RAISE EXCEPTION 'Video metadata is immutable.' USING ERRCODE='42501';
        END IF;
    ELSIF NEW.output_format IN ('webm','mp4') THEN
        PERFORM 1 FROM media.jobs WHERE id=NEW.job_id FOR UPDATE;
        IF NEW.state<>'pending' OR NOT EXISTS(SELECT 1 FROM media.jobs
            WHERE id=NEW.job_id AND input_kind='image-v1' AND state='processing'
            AND lease_token=NEW.lease_token AND expires_at>clock_timestamp()) THEN
            RAISE EXCEPTION 'Video metadata requires a current processing lease.' USING ERRCODE='42501';
        END IF;
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_video_metadata() FROM PUBLIC;
CREATE TRIGGER media_video_metadata_immutable BEFORE INSERT OR UPDATE ON media.assets
    FOR EACH ROW EXECUTE FUNCTION media.guard_video_metadata();
GRANT SELECT(video_enabled,video_audio,max_video_bytes,max_video_duration,max_video_dimension)
    ON content.boards TO board_attachment_owner;
GRANT SELECT(video_duration_micros,video_has_audio) ON media.assets TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE OR REPLACE FUNCTION content.check_image_board_policy(
    p_board text,p_input_bytes bigint,p_width integer,p_height integer,p_format text
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE b record;
BEGIN
    SELECT max_image_bytes,min_image_width,min_image_height,max_image_dimension,gif_only,pdf_enabled
        INTO b FROM content.boards WHERE slug=p_board;
    IF NOT FOUND OR p_input_bytes IS NULL OR p_width IS NULL OR p_height IS NULL
        OR p_format IS NULL OR p_format NOT IN ('png','gif','pdf','webm','mp4') THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF p_input_bytes<1 OR p_input_bytes>least(b.max_image_bytes,8388608) THEN
        RAISE EXCEPTION 'Error: File too large.' USING ERRCODE='23514';
    END IF;
    IF p_format='pdf' AND (NOT b.pdf_enabled OR p_width<>1 OR p_height<>1) THEN
        RAISE EXCEPTION 'Error: Corrupted file or unsupported file type.' USING ERRCODE='23514';
    END IF;
    -- The source PDF branch precedes its image-only GIF restriction.
    IF b.gif_only AND p_format NOT IN ('gif','pdf','webm','mp4') THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
    IF p_width<b.min_image_width OR p_height<b.min_image_height THEN
        RAISE EXCEPTION 'Error: Image resolution is too small.' USING ERRCODE='23514';
    END IF;
    IF p_width>b.max_image_dimension OR p_height>b.max_image_dimension THEN
        RAISE EXCEPTION 'Error: Image resolution is too large.' USING ERRCODE='23514';
    END IF;
END $$;
REVOKE ALL ON FUNCTION content.check_image_board_policy(text,bigint,integer,integer,text)
    FROM PUBLIC,board_public,board_staff,board_auth,board_media,board_media_read,board_media_intake,board_monitor;

-- Both attachment consumers call this only after locking the current board.
CREATE FUNCTION content.check_attachment_board_policy(
    p_board text,p_input_bytes bigint,p_bytes bigint,p_width integer,p_height integer,
    p_format text,p_filename text,p_duration bigint,p_audio boolean
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE b record; suffix text;
BEGIN
    SELECT max_image_bytes,video_enabled,video_audio,max_video_bytes,max_video_duration,max_video_dimension
        INTO b FROM content.boards WHERE slug=p_board;
    IF NOT FOUND OR p_input_bytes IS NULL OR p_bytes IS NULL OR p_format IS NULL OR p_filename IS NULL THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF p_input_bytes<1 OR p_input_bytes>least(b.max_image_bytes,8388608) THEN
        RAISE EXCEPTION 'Error: File too large.' USING ERRCODE='23514';
    END IF;
    suffix:=substring(lower(p_filename) from '\.(webm|mp4)$');
    IF (b.video_enabled AND suffix IS NOT NULL AND p_format<>suffix)
        OR (p_format IN ('webm','mp4') AND (NOT b.video_enabled OR suffix IS DISTINCT FROM p_format)) THEN
        RAISE EXCEPTION 'Error: Corrupted file or unsupported file type.' USING ERRCODE='23514';
    END IF;
    IF p_format IN ('webm','mp4') THEN
        IF p_input_bytes>b.max_video_bytes THEN
            RAISE EXCEPTION 'Error: File too large.' USING ERRCODE='23514';
        END IF;
        IF p_duration IS NULL OR p_duration<1 OR p_duration>b.max_video_duration::bigint*1000000 THEN
            RAISE EXCEPTION 'Error: Maximum allowed video duration is % seconds.',b.max_video_duration USING ERRCODE='23514';
        END IF;
        IF p_audio IS NULL THEN
            RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
        END IF;
        IF p_audio AND NOT b.video_audio THEN
            RAISE EXCEPTION 'Error: Audio streams are not allowed.' USING ERRCODE='23514';
        END IF;
        IF p_width>b.max_video_dimension OR p_height>b.max_video_dimension THEN
            RAISE EXCEPTION 'Error: Image resolution is too large.' USING ERRCODE='23514';
        END IF;
    END IF;
    PERFORM content.check_image_board_policy(p_board,p_input_bytes,p_width,p_height,p_format);
    IF p_format IN ('webm','mp4') AND (p_bytes<1 OR p_bytes>b.max_image_bytes
        OR (p_format='webm' AND p_bytes>b.max_video_bytes)) THEN
        RAISE EXCEPTION 'Error: File too large.' USING ERRCODE='23514';
    END IF;
END $$;
REVOKE ALL ON FUNCTION content.check_attachment_board_policy(text,bigint,bigint,integer,integer,text,text,bigint,boolean)
    FROM PUBLIC,board_public,board_staff,board_auth,board_media,board_media_read,board_media_intake,board_monitor;

CREATE OR REPLACE FUNCTION content.insert_post_attachment(
    p_id bigint, p_board text, p_thread bigint, p_name text, p_subject text,
    p_comment text, p_job text, p_capability text, p_spoiler boolean,
    p_posted_at timestamptz
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_limit integer;
    v_pdf boolean;
    v_sticky boolean;
    v_undead boolean;
    v_state text;
    v_created timestamptz;
    v_filename text;
    v_input_bytes bigint;
    v_hash bytea;
    v_asset record;
BEGIN
    IF p_posted_at IS NULL OR NOT isfinite(p_posted_at) THEN
        RAISE EXCEPTION 'Invalid posting timestamp.' USING ERRCODE = '22023';
    END IF;
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Attachment insertion requires Read Committed.' USING ERRCODE = '22023';
    END IF;
    IF p_job IS NULL OR octet_length(p_job) <> 32 OR p_job !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$'
       OR p_spoiler IS NULL THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    -- Same lock order as posting and deletion: board, thread, then media job.
    SELECT b.image_limit,b.pdf_enabled INTO v_limit,v_pdf FROM content.boards b WHERE b.slug = p_board FOR UPDATE;
    IF v_limit IS NULL THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    SELECT t.sticky, t.undead INTO v_sticky, v_undead
        FROM content.threads t WHERE t.board = p_board AND t.id = p_thread
        AND NOT t.deleted AND NOT t.closed AND t.archived_at IS NULL FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.state, j.created_at, j.filename, j.input_bytes INTO v_state, v_created, v_filename, v_input_bytes
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
    SELECT a.id, a.bytes, a.width, a.height, a.output_format,a.video_duration_micros,a.video_has_audio INTO v_asset FROM media.assets a
        WHERE a.job_id = p_job AND a.state = 'approved' ORDER BY a.id LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE = 'P0001';
    END IF;
    PERFORM content.check_attachment_board_policy(p_board,v_input_bytes,v_asset.bytes,v_asset.width,v_asset.height,v_asset.output_format,v_filename,v_asset.video_duration_micros,v_asset.video_has_audio);
    IF (v_asset.output_format='pdf' AND lower(right(v_filename,4))<>'.pdf')
        OR (v_pdf AND lower(right(v_filename,4))='.pdf' AND v_asset.output_format<>'pdf') THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
    IF v_limit = 0 OR (NOT v_sticky AND NOT v_undead AND p_id <> p_thread AND (SELECT count(*) FROM content.post_media m
        JOIN content.posts p ON p.id = m.post_id WHERE p.board = p_board
        AND p.thread_id = p_thread AND p.id <> p_thread AND NOT p.deleted AND NOT m.file_deleted) >= v_limit) THEN
        RAISE EXCEPTION 'This board or thread cannot accept another image.' USING ERRCODE = 'P0001';
    END IF;
    INSERT INTO content.posts(id, board, thread_id, name, subject, comment, created_at)
        VALUES (p_id, p_board, p_thread, p_name, p_subject, p_comment, date_trunc('second', p_posted_at));
    INSERT INTO content.post_media(post_id, job_id, asset_id, filename, bytes, width, height, spoiler)
        VALUES (p_id, p_job, v_asset.id, v_filename, v_asset.bytes, v_asset.width, v_asset.height, p_spoiler);
END
$$;
REVOKE ALL ON FUNCTION content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean,timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean,timestamptz) TO board_public;


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
    SELECT image_limit,text_only,staff_only,upload_board,comment_spoiler_cleanup,pdf_enabled INTO b
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
    SELECT id,bytes,width,height,output_format,video_duration_micros,video_has_audio INTO a FROM media.assets
        WHERE job_id=p_job AND state='approved' ORDER BY id LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE='P0001';
    END IF;
    PERFORM content.check_attachment_board_policy(p_board,j.input_bytes,a.bytes,a.width,a.height,a.output_format,j.filename,a.video_duration_micros,a.video_has_audio);
    IF (a.output_format='pdf' AND lower(right(j.filename,4))<>'.pdf')
        OR (b.pdf_enabled AND lower(right(j.filename,4))='.pdf' AND a.output_format<>'pdf') THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
    -- Staff rank never bypasses the existing intake and published-file bounds.
    IF j.input_bytes IS NULL OR j.input_bytes NOT BETWEEN 1 AND 8388608
        OR a.bytes NOT BETWEEN 1 AND (CASE a.output_format WHEN 'gif' THEN 20971520 WHEN 'pdf' THEN 8388608 WHEN 'webm' THEN 8388608 WHEN 'mp4' THEN 8388608 ELSE 5242880 END) THEN
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
-- Only the trusted migration identity accepts this reviewed catalog transition.
-- The earlier preflight must pass; runtime roles retain no resealing authority.
DO $video_transfer_reseal$
BEGIN
    IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
        EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
        UPDATE post_secrets.thread_transfer_catalog
            SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
        IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Text transfer catalog is not ready after video policy.';
        END IF;
        EXECUTE 'RESET ROLE';
    END IF;
END $video_transfer_reseal$;
