-- Add upload-board admissions under the existing authority preflights.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before upload-board admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before upload-board admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before upload-board admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before upload-board admission'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before upload-board admission'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before upload-board admission'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE OR REPLACE FUNCTION content.check_attachment_board_policy(
    p_board text,p_input_bytes bigint,p_bytes bigint,p_width integer,p_height integer,
    p_format text,p_filename text,p_duration bigint,p_audio boolean
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE b record; suffix text;
BEGIN
    SELECT upload_board,max_image_bytes,video_enabled,video_audio,max_video_bytes,max_video_duration,max_video_dimension
        INTO b FROM content.boards WHERE slug=p_board;
    IF NOT FOUND OR p_input_bytes IS NULL OR p_bytes IS NULL OR p_format IS NULL OR p_filename IS NULL THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF b.upload_board OR p_format='swf' THEN
        IF NOT b.upload_board OR p_format<>'swf' OR p_width IS NULL OR p_height IS NULL
         OR p_width NOT BETWEEN 1 AND 107374182 OR p_height NOT BETWEEN 1 AND 107374182
         OR p_duration IS NOT NULL OR p_audio IS NOT NULL OR left(p_filename,1)='.' THEN
            RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514'; END IF;
        IF p_input_bytes NOT BETWEEN 8 AND least(b.max_image_bytes,10485760) OR p_bytes<>p_input_bytes THEN
            RAISE EXCEPTION 'Error: File too large.' USING ERRCODE='23514'; END IF;
        RETURN;
    END IF;
    IF p_input_bytes<1 OR p_input_bytes>least(b.max_image_bytes,CASE WHEN p_format IN ('png','jpeg') THEN 10485760 ELSE 8388608 END) THEN
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
    IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Upload-history authority is unavailable' USING ERRCODE='P196U';
    END IF;
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
    -- Board/thread locks precede this relation lock, as in deletion. This
    -- conflicts with trigger DDL and already covers our eventual INSERT.
    LOCK TABLE content.post_media IN ROW EXCLUSIVE MODE NOWAIT;
    IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Upload-history authority is unavailable' USING ERRCODE='P196U';
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
    SELECT a.id, a.bytes, a.width, a.height, a.output_format,a.still_profile,a.video_duration_micros,a.video_has_audio,a.thumbnail_width,a.thumbnail_height INTO v_asset FROM media.assets a
        WHERE a.job_id = p_job AND a.state = 'approved' ORDER BY a.id LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE = 'P0001';
    END IF;
    PERFORM content.check_attachment_board_policy(p_board,v_input_bytes,v_asset.bytes,v_asset.width,v_asset.height,v_asset.output_format,v_filename,v_asset.video_duration_micros,v_asset.video_has_audio);
    IF v_asset.output_format IN ('webm','mp4') AND p_id<>p_thread
        AND least(v_asset.thumbnail_width,v_asset.thumbnail_height)*125/greatest(125,v_asset.thumbnail_width,v_asset.thumbnail_height)=0 THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
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
    IF v_asset.still_profile IS NOT NULL THEN
        PERFORM content.check_still_attachment_context(p_job,p_board,p_id,p_thread,v_asset.output_format);
    END IF;
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
    IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Upload-history authority is unavailable' USING ERRCODE='P196U';
    END IF;
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
    -- Board/thread locks precede this relation lock, as in deletion. This
    -- conflicts with trigger DDL and already covers our eventual INSERT.
    LOCK TABLE content.post_media IN ROW EXCLUSIVE MODE NOWAIT;
    IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Upload-history authority is unavailable' USING ERRCODE='P196U';
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
    SELECT id,bytes,width,height,output_format,still_profile,video_duration_micros,video_has_audio,thumbnail_width,thumbnail_height INTO a FROM media.assets
        WHERE job_id=p_job AND state='approved' ORDER BY id LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE='P0001';
    END IF;
    IF a.still_profile IS NOT NULL THEN
        PERFORM content.check_still_attachment_context(p_job,p_board,p_id,p_thread,a.output_format);
    END IF;
    PERFORM content.check_attachment_board_policy(p_board,j.input_bytes,a.bytes,a.width,a.height,a.output_format,j.filename,a.video_duration_micros,a.video_has_audio);
    IF a.output_format IN ('webm','mp4') AND p_id<>p_thread
        AND least(a.thumbnail_width,a.thumbnail_height)*125/greatest(125,a.thumbnail_width,a.thumbnail_height)=0 THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
    IF (a.output_format='pdf' AND lower(right(j.filename,4))<>'.pdf')
        OR (b.pdf_enabled AND lower(right(j.filename,4))='.pdf' AND a.output_format<>'pdf') THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
    -- Staff rank never bypasses the existing intake and published-file bounds.
    IF j.input_bytes IS NULL OR j.input_bytes NOT BETWEEN 1 AND (CASE WHEN a.output_format='swf' OR a.still_profile='still-v6' THEN 10485760 ELSE 8388608 END)
        OR a.bytes NOT BETWEEN 1 AND (CASE WHEN a.output_format='swf' OR a.still_profile='still-v6' THEN 10485760 WHEN a.output_format='gif' THEN 20971520 WHEN a.output_format IN ('pdf','webm','mp4') THEN 8388608 ELSE 5242880 END) THEN
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
    IF v_invoker='board_public' AND EXISTS(SELECT 1 FROM content.boards WHERE slug=p_board AND upload_board) THEN
        RAISE EXCEPTION 'It doesn''t make any sense to do a file-only delete on a file board!' USING ERRCODE='23514';
    END IF;
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

RESET ROLE;
ALTER TABLE content.post_media DROP CONSTRAINT post_media_width_check, DROP CONSTRAINT post_media_height_check,
 ADD CONSTRAINT post_media_width_check CHECK(width BETWEEN 1 AND 107374182),
 ADD CONSTRAINT post_media_height_check CHECK(height BETWEEN 1 AND 107374182);
CREATE TABLE content.upload_board_names (
 board text NOT NULL REFERENCES content.boards(slug),
 name text COLLATE "C" NOT NULL CHECK(octet_length(name) BETWEEN 5 AND 259),
 post_id bigint NOT NULL UNIQUE REFERENCES content.posts(id) ON DELETE CASCADE,
 PRIMARY KEY(board,name)
);
REVOKE ALL ON content.upload_board_names FROM PUBLIC;
ALTER TABLE content.upload_board_names OWNER TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE FUNCTION content.sync_upload_board_name(p_post bigint,p_require_context boolean) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE entry record; stored_name text;
BEGIN
 SELECT p.board,p.id,p.thread_id,p.deleted,p.file_deleted AS post_file_deleted,
  m.file_deleted,m.filename,m.width,m.height,m.bytes,m.job_id,a.output_format,
  t.deleted AS thread_deleted,a.width AS asset_width,a.height AS asset_height,a.bytes AS asset_bytes,a.state AS asset_state,b.upload_board
 INTO entry FROM content.posts p JOIN content.boards b ON b.slug=p.board
 JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
 JOIN content.post_media m ON m.post_id=p.id JOIN media.assets a ON a.id=m.asset_id
 WHERE p.id=p_post FOR UPDATE OF b;
 IF FOUND AND entry.output_format<>'swf' AND (entry.width>10000 OR entry.height>10000) THEN
  RAISE EXCEPTION 'Error: Image resolution is too large.' USING ERRCODE='23514'; END IF;
 IF NOT FOUND OR entry.output_format<>'swf' OR entry.thread_deleted OR entry.deleted OR entry.post_file_deleted OR entry.file_deleted THEN
  DELETE FROM content.upload_board_names WHERE post_id=p_post;
  RETURN;
 END IF;
 IF NOT entry.upload_board OR entry.id<>entry.thread_id OR entry.asset_state<>'approved' OR left(entry.filename,1)='.'
  OR (entry.width,entry.height,entry.bytes) IS DISTINCT FROM (entry.asset_width,entry.asset_height,entry.asset_bytes)
  OR (p_require_context AND NOT EXISTS(SELECT 1 FROM media.image_context c WHERE c.job_id=entry.job_id AND c.board=entry.board AND c.thread_id=0)) THEN
  RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514'; END IF;
 stored_name:=regexp_replace(entry.filename,'\.[a-zA-Z0-9]+$','')||'.swf';
 BEGIN
  INSERT INTO content.upload_board_names(board,name,post_id) VALUES(entry.board,stored_name,p_post)
   ON CONFLICT(post_id) DO UPDATE SET board=EXCLUDED.board,name=EXCLUDED.name;
 EXCEPTION WHEN unique_violation THEN
  RAISE EXCEPTION 'Filename already exists.' USING ERRCODE='23514';
 END;
END $$;
REVOKE ALL ON FUNCTION content.sync_upload_board_name(bigint,boolean) FROM PUBLIC;
CREATE FUNCTION content.sync_upload_board_media() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 PERFORM content.sync_upload_board_name(CASE WHEN TG_OP='DELETE' THEN OLD.post_id ELSE NEW.post_id END,
  TG_OP='INSERT' OR (TG_OP='UPDATE' AND (NEW.job_id,NEW.asset_id,NEW.post_id) IS DISTINCT FROM (OLD.job_id,OLD.asset_id,OLD.post_id)));
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION content.sync_upload_board_media() FROM PUBLIC;
CREATE FUNCTION content.sync_upload_board_post() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 PERFORM content.sync_upload_board_name(NEW.id,false);
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION content.sync_upload_board_post() FROM PUBLIC;
CREATE FUNCTION content.sync_upload_board_thread() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF NEW.deleted IS DISTINCT FROM OLD.deleted AND EXISTS(SELECT 1 FROM content.boards WHERE slug=NEW.board AND upload_board) THEN
  PERFORM content.sync_upload_board_name(NEW.id,false);
 END IF;
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION content.sync_upload_board_thread() FROM PUBLIC;
CREATE FUNCTION content.require_upload_board_file() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE entry record;
BEGIN
 SELECT p.id,p.thread_id,p.deleted,p.file_deleted,b.upload_board INTO entry
 FROM content.posts p JOIN content.boards b ON b.slug=p.board WHERE p.id=NEW.id;
 IF NOT FOUND OR NOT entry.upload_board OR entry.deleted THEN RETURN NULL; END IF;
 IF entry.id=entry.thread_id THEN
  IF entry.file_deleted OR NOT EXISTS(SELECT 1 FROM content.post_media m JOIN media.assets a ON a.id=m.asset_id
    WHERE m.post_id=entry.id AND NOT m.file_deleted AND a.state='approved' AND a.output_format='swf') THEN
   RAISE EXCEPTION 'Error: You must attach a file.' USING ERRCODE='23514'; END IF;
 ELSIF EXISTS(SELECT 1 FROM content.post_media WHERE post_id=entry.id) THEN
  RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
 END IF;
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION content.require_upload_board_file() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.sync_upload_board_media(),content.sync_upload_board_post(),content.sync_upload_board_thread(),content.require_upload_board_file() TO board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
CREATE CONSTRAINT TRIGGER upload_board_required_file AFTER INSERT ON content.posts
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION content.require_upload_board_file();
CREATE TRIGGER upload_board_media_name AFTER INSERT OR UPDATE OR DELETE ON content.post_media
 FOR EACH ROW EXECUTE FUNCTION content.sync_upload_board_media();
CREATE TRIGGER upload_board_thread_name AFTER UPDATE OF deleted ON content.threads
 FOR EACH ROW EXECUTE FUNCTION content.sync_upload_board_thread();
CREATE TRIGGER upload_board_post_name AFTER UPDATE OF deleted,file_deleted ON content.posts
 FOR EACH ROW EXECUTE FUNCTION content.sync_upload_board_post();
SET LOCAL ROLE board_attachment_owner;
REVOKE EXECUTE ON FUNCTION content.sync_upload_board_media(),content.sync_upload_board_post(),content.sync_upload_board_thread(),content.require_upload_board_file() FROM board_migrator;
RESET ROLE;

-- Public filename metadata follows the same approval/deletion view as bytes.
CREATE OR REPLACE VIEW media.approved_post_assets WITH (security_barrier=true) AS
 SELECT p.board,m.tim,a.*,m.filename FROM content.post_media m
 JOIN content.posts p ON p.id=m.post_id
 JOIN media.approved_assets a ON a.id=m.asset_id;

GRANT CREATE ON SCHEMA post_secrets TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;
CREATE OR REPLACE FUNCTION post_secrets.transfer_catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
    SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE c.relkind='r' AND (
      (n.nspname='post_secrets' AND (c.relname LIKE 'thread_transfer_%'
        OR c.relname IN('deletion','op_peers','op_replies','posting_history','anonymous_posts','anonymous_sessions','poster_contexts','image_upload_history','image_history_scopes','comment_history')))
      OR (n.nspname='content' AND c.relname IN('boards','threads','posts','post_media','moderation_audit','upload_board_names'))
      OR (n.nspname='media' AND c.relname IN('assets','image_context','input_receipts'))
      OR (n.nspname='verification_private' AND c.relname IN('post_ownership','pass_posts'))
      OR (n.nspname='staff_identity' AND c.relname IN('accounts','sessions','credentials')))
 ), triggers AS (
    SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
 ), funcs AS (
    SELECT p.* FROM pg_proc p WHERE p.proowner='board_thread_transfer_owner'::regrole
      OR p.oid IN(SELECT t.tgfoid FROM triggers t)
      OR (p.pronamespace=(SELECT oid FROM pg_namespace WHERE nspname='media')
          AND p.proname IN('record_input_receipt','require_input_receipt'))
      OR p.oid IN('content.delete_post_attachment(text,bigint)'::regprocedure,
        'content.sync_upload_board_name(bigint,boolean)'::regprocedure,
        'content.check_attachment_board_policy(text,bigint,bigint,integer,integer,text,text,bigint,boolean)'::regprocedure,
        'content.revalidate_staff_quota_refusal(bytea,bytea,integer,text)'::regprocedure,
        'content.comment_duplicate_remaining(bytea,text,bytea,bigint)'::regprocedure,
        'content.record_comment_projection(bigint,bytea,bytea)'::regprocedure,
        'content.expire_comment_history()'::regprocedure,
        'content.image_duplicate_candidate(text,text,text,bigint,boolean,boolean)'::regprocedure,
        'content.expire_image_upload_history()'::regprocedure,
        'content.erasure_visibility_ready()'::regprocedure,
        'staff_identity.has_board_access(bigint,text)'::regprocedure,
        'content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean,timestamptz)'::regprocedure,
        'content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean)'::regprocedure,
        'content.consume_staff_attachment_receipt(bigint,text,bigint,text,bytea,boolean,boolean)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
    (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner)::text,
       r.relkind,r.relpersistence,r.relrowsecurity,r.relforcerowsecurity,
       post_secrets.transfer_acl(coalesce(r.relacl,acldefault('r',r.relowner))),
       (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,
          a.attidentity,a.attgenerated,pg_get_expr(d.adbin,d.adrelid),post_secrets.transfer_acl(a.attacl)) ORDER BY a.attnum)
        FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
        WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
       (SELECT jsonb_agg(jsonb_build_array(k.conname,k.contype,k.convalidated,k.condeferrable,k.condeferred,
           pg_get_constraintdef(k.oid,true)) ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=r.oid),
       (SELECT jsonb_agg(jsonb_build_array(p.polname,p.polcmd,p.polpermissive,
          (SELECT jsonb_agg(CASE WHEN x=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x)::text END
             ORDER BY CASE WHEN x=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x)::text END) FROM unnest(p.polroles) x),
          pg_get_expr(p.polqual,p.polrelid),pg_get_expr(p.polwithcheck,p.polrelid)) ORDER BY p.polname)
        FROM pg_policy p WHERE p.polrelid=r.oid)) ORDER BY r.identity) FROM relations r),
    (SELECT jsonb_agg(jsonb_build_array(t.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
       ORDER BY t.identity,t.tgname) FROM triggers t),
    (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner)::text,
       post_secrets.transfer_acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
       ORDER BY p.oid::regprocedure::text) FROM funcs p))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_thread_transfer_owner;

SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_image_classifier_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog SET definition_hash=decode(current_setting('board.migration_image_classifier_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_image_report_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_image_report_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'upload-board admission changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'upload-board admission changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'upload-board admission changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'upload-board admission changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'upload-board admission changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'upload-board admission changed challenge authority'; END IF; END $$;
RESET ROLE;
