-- Prior-ban upload admission runs before content filters and again at attach.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before upload history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Content catalog drifted before upload history'; END IF;
END $$;
RESET ROLE;
GRANT SELECT(worksafe) ON content.boards TO board_restriction_owner;
SET LOCAL ROLE board_thread_transfer_owner;
GRANT EXECUTE ON FUNCTION content.text_thread_transfer_ready() TO board_attachment_owner;
RESET ROLE;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
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
    IF j.input_bytes IS NULL OR j.input_bytes NOT BETWEEN 1 AND (CASE WHEN a.still_profile='still-v6' THEN 10485760 ELSE 8388608 END)
        OR a.bytes NOT BETWEEN 1 AND (CASE WHEN a.still_profile='still-v6' THEN 10485760 WHEN a.output_format='gif' THEN 20971520 WHEN a.output_format IN ('pdf','webm','mp4') THEN 8388608 ELSE 5242880 END) THEN
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

CREATE FUNCTION content.prior_ban_upload_digest(
 p_job text,p_capability text,p_board text,p_parent bigint,p_authorized boolean,p_badged boolean
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE m record;
BEGIN
 SELECT * INTO STRICT m FROM content.attachment_upload_filter_metadata(
  p_job,p_capability,p_board,p_parent,p_authorized,p_badged);
 IF m.normalized_md5 IS NULL THEN RETURN NULL; END IF;
 IF octet_length(m.normalized_md5)<>32 OR m.normalized_md5 !~ '^[0-9a-f]{32}$' THEN
  RAISE EXCEPTION 'Normalized upload identity is unavailable' USING ERRCODE='P196U'; END IF;
 RETURN decode(m.normalized_md5,'hex');
END $$;
REVOKE ALL ON FUNCTION content.prior_ban_upload_digest(text,text,text,bigint,boolean,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.prior_ban_upload_digest(text,text,text,bigint,boolean,boolean) TO board_restriction_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
GRANT CREATE ON SCHEMA restrictions,content TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
DROP INDEX restrictions.restriction_upload_history;
CREATE INDEX restriction_upload_history ON restrictions.bans(template_id,upload_observation,normalized_upload_md5)
 WHERE template_id IN(6,226);

-- NULL means neither allow nor reject can be established from retained facts.
CREATE FUNCTION restrictions.prior_upload_decision(p_known boolean,p_worksafe boolean,
 p_m226 integer,p_k226 integer,p_u226 integer,p_m6 integer,p_k6 integer,p_u6 integer)
RETURNS boolean LANGUAGE plpgsql IMMUTABLE SET search_path=pg_catalog,pg_temp AS $$
DECLARE matches integer; unknowns integer; knowns integer;
BEGIN
 IF p_known IS NULL OR p_worksafe IS NULL
  OR array_position(ARRAY[p_m226,p_k226,p_u226,p_m6,p_k6,p_u6],NULL) IS NOT NULL
  OR EXISTS(SELECT 1 FROM unnest(ARRAY[p_m226,p_k226,p_u226,p_m6,p_k6,p_u6]) n WHERE n<0 OR n>3)
  OR p_m226>p_k226 OR p_m6>p_k6 OR (NOT p_known AND (p_m226<>0 OR p_m6<>0)) THEN
  RAISE EXCEPTION 'Invalid upload-history evidence' USING ERRCODE='23514'; END IF;
 matches:=p_m226+CASE WHEN p_worksafe THEN p_m6 ELSE 0 END;
 unknowns:=p_u226+CASE WHEN p_worksafe THEN p_u6 ELSE 0 END;
 knowns:=p_k226+CASE WHEN p_worksafe THEN p_k6 ELSE 0 END;
 IF p_known THEN
  IF matches>=3 THEN RETURN true; END IF;
  IF matches+unknowns<3 THEN RETURN false; END IF;
 ELSIF knowns+unknowns<3 THEN RETURN false;
 END IF;
 RETURN NULL;
END $$;
CREATE FUNCTION restrictions.prior_upload_history(p_digest bytea,p_worksafe boolean)
RETURNS boolean LANGUAGE plpgsql STABLE SET search_path=pg_catalog,pg_temp AS $$
DECLARE m226 integer; k226 integer; u226 integer; m6 integer:=0; k6 integer:=0; u6 integer:=0;
BEGIN
 IF p_worksafe IS NULL OR (p_digest IS NOT NULL AND octet_length(p_digest)<>16) THEN
  RAISE EXCEPTION 'Invalid upload-history context' USING ERRCODE='23514'; END IF;
 -- All queries share this statement snapshot. No active, expiry, warning,
 -- board or DISTINCT filter: each retained source-template event contributes.
 SELECT
  (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans WHERE template_id=226 AND upload_observation='known' AND normalized_upload_md5=p_digest LIMIT 3) s),
  (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans WHERE template_id=226 AND upload_observation='known' LIMIT 3) s),
  (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans WHERE template_id=226 AND upload_observation='unknown' LIMIT 3) s)
 INTO m226,k226,u226;
 IF p_worksafe THEN
  SELECT
   (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans WHERE template_id=6 AND upload_observation='known' AND normalized_upload_md5=p_digest LIMIT 3) s),
   (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans WHERE template_id=6 AND upload_observation='known' LIMIT 3) s),
   (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans WHERE template_id=6 AND upload_observation='unknown' LIMIT 3) s)
  INTO m6,k6,u6;
 END IF;
 RETURN restrictions.prior_upload_decision(p_digest IS NOT NULL,p_worksafe,m226,k226,u226,m6,k6,u6);
END $$;

CREATE FUNCTION content.prior_upload_banned(
 p_job text,p_capability text,p_board text,p_parent bigint,p_authorized boolean,p_badged boolean
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE digest bytea; worksafe boolean;
BEGIN
 IF session_user NOT IN('board_public','board_staff') THEN
  RAISE EXCEPTION 'Upload-history admission is unavailable' USING ERRCODE='42501'; END IF;
 PERFORM restrictions.gate(false);
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Upload-history authority is unavailable' USING ERRCODE='P196U'; END IF;
 digest:=content.prior_ban_upload_digest(p_job,p_capability,p_board,p_parent,p_authorized,p_badged);
 IF p_board='b' THEN RETURN false; END IF;
 SELECT b.worksafe INTO worksafe FROM content.boards b WHERE b.slug=p_board;
 IF NOT FOUND THEN RAISE EXCEPTION 'Upload board is unavailable' USING ERRCODE='P0002'; END IF;
 RETURN restrictions.prior_upload_history(digest,worksafe);
END $$;

CREATE FUNCTION content.assert_prior_upload_history() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE board text; worksafe boolean; blocked boolean;
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_TABLE_NAME<>'post_media' OR TG_OP<>'INSERT'
  OR TG_WHEN<>'BEFORE' OR TG_LEVEL<>'ROW' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid upload-history trigger' USING ERRCODE='23514'; END IF;
 SELECT p.board,b.worksafe INTO board,worksafe FROM content.posts p JOIN content.boards b ON b.slug=p.board
  WHERE p.id=NEW.post_id;
 IF NOT FOUND THEN RAISE EXCEPTION 'Upload target is unavailable' USING ERRCODE='P0002'; END IF;
 IF board='b' THEN RETURN NEW; END IF;
 -- Normal posting owns this shared gate before its board and media locks.
 -- Direct attachment writers use NOWAIT, so they cannot wait backwards.
 PERFORM restrictions.gate(false);
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Upload-history authority is unavailable' USING ERRCODE='P196U'; END IF;
 blocked:=restrictions.prior_upload_history(NEW.normalized_md5,worksafe);
 IF blocked IS NULL THEN RAISE EXCEPTION 'Upload history is unavailable' USING ERRCODE='P196U'; END IF;
 IF blocked THEN RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='P196B'; END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION restrictions.prior_upload_decision(boolean,boolean,integer,integer,integer,integer,integer,integer),
 restrictions.prior_upload_history(bytea,boolean),content.prior_upload_banned(text,text,text,bigint,boolean,boolean),
 content.assert_prior_upload_history() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.prior_upload_banned(text,text,text,bigint,boolean,boolean) TO board_public,board_staff;
GRANT EXECUTE ON FUNCTION content.assert_prior_upload_history() TO board_migrator;
RESET ROLE;
CREATE TRIGGER zzz_prior_upload_history BEFORE INSERT ON content.post_media
 FOR EACH ROW EXECUTE FUNCTION content.assert_prior_upload_history();
SET LOCAL ROLE board_restriction_owner;
REVOKE EXECUTE ON FUNCTION content.assert_prior_upload_history() FROM board_migrator;

CREATE OR REPLACE FUNCTION restrictions.catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='restrictions' OR c.oid IN('post_secrets.posting_history'::regclass,
    'admission.policy'::regclass,'admission.file_rules'::regclass,
    'admission.automatic_ban_exemptions'::regclass,'admission.dmca_warnings'::regclass,
    'admission.file_effect_capacity'::regclass)
 ), triggers AS (
  SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'post_secrets.posting_history'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='post_secrets.posting_history'::regclass AND t.tgname='source_restriction_guard' AND NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'content.reports'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='content.reports'::regclass AND t.tgname='source_report_restriction_guard' AND NOT t.tgisinternal
 ), funcs AS (
  SELECT p.* FROM pg_proc p WHERE p.proowner='board_restriction_owner'::regrole
    OR p.oid IN(SELECT t.tgfoid FROM triggers t)
    OR p.oid IN('content.lock_automatic_range_policy(boolean)'::regprocedure,
       'content.record_inserted_posting_history()'::regprocedure,
       'content.lock_report_network_policy()'::regprocedure,
       'content.report_network_rules(text,bigint,boolean)'::regprocedure,
       'post_secrets.report_network_activity(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
       'post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
       'post_secrets.assert_report_network(text,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
    r.relrowsecurity,r.relforcerowsecurity,r.reloptions,restrictions.acl(coalesce(r.relacl,acldefault(CASE WHEN r.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,r.relowner))),
    CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
       pg_get_expr(d.adbin,d.adrelid),restrictions.acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated)
       ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    restrictions.acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
    ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(t.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
    ORDER BY t.identity,t.tgname) FROM triggers t),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),pg_get_functiondef(p.oid),
      restrictions.acl(coalesce(p.proacl,acldefault('f',p.proowner))),
      has_function_privilege('board_restriction_owner',p.oid,'EXECUTE')) ORDER BY p.oid::regprocedure::text)
   FROM pg_proc p WHERE p.oid IN('staff_identity.has_board_access(bigint,text)'::regprocedure,
      'admission.resolve_file_effect_rule(text,bigint,bytea,bytea,boolean)'::regprocedure,
      'admission.file_effect_exempt(inet)'::regprocedure,
      'admission.file_effect_dmca(bytea)'::regprocedure,
      'admission.automatic_ban_exempt(inet)'::regprocedure,
      'admission.record_dmca_warning(bytea,timestamptz)'::regprocedure,
      'content.file_effect_upload_metadata(text,text,text,bigint,boolean)'::regprocedure,
      'content.prior_ban_upload_digest(text,text,text,bigint,boolean,boolean)'::regprocedure,
      'content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean)'::regprocedure,
      'content.check_attachment_upload(text,text)'::regprocedure,
      'content.check_attachment_board_policy(text,bigint,bigint,integer,integer,text,text,bigint,boolean)'::regprocedure,
      'content.revalidate_staff_refusal(bytea,bytea,integer,text)'::regprocedure,
      'content.text_thread_transfer_ready()'::regprocedure,
      'content.lock_posting_actor(bytea,boolean)'::regprocedure,
      'post_secrets.lookup_automatic_identity(bytea,bigint)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
      'content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),restrictions.acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
   FROM pg_namespace n WHERE n.nspname='restrictions'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions,content FROM board_restriction_owner;
GRANT CREATE ON SCHEMA post_secrets TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;
CREATE OR REPLACE FUNCTION post_secrets.transfer_catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
    SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE c.relkind='r' AND (
      (n.nspname='post_secrets' AND (c.relname LIKE 'thread_transfer_%'
        OR c.relname IN('deletion','op_peers','op_replies','posting_history','anonymous_posts','anonymous_sessions','poster_contexts')))
      OR (n.nspname='content' AND c.relname IN('boards','threads','posts','post_media','moderation_audit'))
      OR (n.nspname='verification_private' AND c.relname='post_ownership')
      OR (n.nspname='staff_identity' AND c.relname IN('accounts','sessions','credentials')))
 ), triggers AS (
    SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
 ), funcs AS (
    SELECT p.* FROM pg_proc p WHERE p.proowner='board_thread_transfer_owner'::regrole
      OR p.oid IN(SELECT t.tgfoid FROM triggers t)
      OR p.oid IN('content.erasure_visibility_ready()'::regprocedure,
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
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Upload history changed content authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Upload history changed restriction authority'; END IF; END $$;
RESET ROLE;
