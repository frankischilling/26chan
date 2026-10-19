-- Source duplicate-image reply timing, bound to the approved upload capability.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before duplicate-image cooldowns'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before duplicate-image cooldowns'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before duplicate-image cooldowns'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before duplicate-image cooldowns'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before duplicate-image cooldowns'; END IF; END $$;
CREATE INDEX posting_history_actor_posts ON post_secrets.posting_history(board,actor_hash,post_id DESC);
GRANT SELECT(post_id,normalized_md5) ON content.post_media TO board_posting_cooldown_owner;
GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner;
SET LOCAL ROLE board_posting_cooldown_owner;
CREATE FUNCTION content.pass_duplicate_cooldown_core(p_actor bytea,p_board text,p_thread bigint,p_request bigint,p_digest bytea,p_bypass boolean)
RETURNS TABLE(kind text,remaining_seconds bigint) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE previous bigint; delay integer;
BEGIN
 IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$'
 OR p_thread IS NULL OR p_thread<=0 OR p_request IS NULL OR p_request NOT BETWEEN 0 AND 9223372036854689407
 OR p_digest IS NULL OR octet_length(p_digest)<>16 OR p_bypass IS NULL THEN
 RAISE EXCEPTION 'Invalid duplicate-image context' USING ERRCODE='23514'; END IF;
 -- Every supplied board inherits RENZOKU_DUPE=300. The source selects the
 -- newest post number, not the greatest timestamp; an OP has resto=0.
 SELECT h.request_at INTO previous FROM post_secrets.posting_history h
 JOIN content.post_media m ON m.post_id=h.post_id
 WHERE h.actor_hash=p_actor AND h.board=p_board AND m.normalized_md5=p_digest
 AND CASE WHEN h.post_id=h.thread_id THEN 0 ELSE h.thread_id END<>p_thread
 ORDER BY h.post_id DESC LIMIT 1;
 delay:=CASE WHEN p_bypass THEN 150 ELSE 300 END;
 IF previous>p_request-delay THEN
 kind:='duplicate_image'; remaining_seconds:=previous+delay-p_request; RETURN NEXT;
 END IF;
END $$;
REVOKE ALL ON FUNCTION content.pass_duplicate_cooldown_core(bytea,text,bigint,bigint,bytea,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_duplicate_cooldown_core(bytea,text,bigint,bigint,bytea,boolean) TO board_anonymous_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner;
SET LOCAL ROLE board_attachment_owner;
GRANT EXECUTE ON FUNCTION content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean) TO board_anonymous_owner;
RESET ROLE;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION verification_private.pass_checked_post_bypass(p_board text,p_thread bigint) RETURNS boolean
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE enabled boolean; bypass boolean; raw text; proof jsonb; key text;
BEGIN
 PERFORM verification_private.pass_gate();
 IF p_thread IS NULL OR p_thread<0 THEN RAISE EXCEPTION 'Invalid posting target' USING ERRCODE='23514'; END IF;
 SELECT b.captcha_enabled INTO enabled FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found' USING ERRCODE='P0002'; END IF;
 bypass:=NOT enabled;
 raw:=current_setting('board.challenge_context',true);
 IF raw IS NOT NULL AND raw<>'' THEN
  IF octet_length(raw)>1024 THEN RAISE EXCEPTION 'Invalid challenge context' USING ERRCODE='P0166'; END IF;
  BEGIN proof:=raw::jsonb;
  EXCEPTION WHEN invalid_text_representation THEN RAISE EXCEPTION 'Invalid challenge context' USING ERRCODE='P0166'; END;
  IF jsonb_typeof(proof)<>'object' THEN RAISE EXCEPTION 'Invalid challenge context' USING ERRCODE='P0166'; END IF;
  IF (SELECT count(*) FROM jsonb_object_keys(proof))<>4 THEN RAISE EXCEPTION 'Invalid challenge context' USING ERRCODE='P0166'; END IF;
  FOREACH key IN ARRAY ARRAY['receipt','peer','session','context'] LOOP
   IF jsonb_typeof(proof->key) IS DISTINCT FROM 'string' OR octet_length(proof->>key)<>64 OR (proof->>key) !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'Invalid challenge context' USING ERRCODE='P0166'; END IF;
  END LOOP;
  IF EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=decode(proof->>'receipt','hex')) THEN
   PERFORM verification_private.pass_checked_receipt(decode(proof->>'receipt','hex'),decode(proof->>'peer','hex'),
    decode(proof->>'session','hex'),decode(proof->>'context','hex'),CASE WHEN p_thread=0 THEN 'thread' ELSE 'reply' END,p_board,p_thread);
   bypass:=true;
  END IF;
 END IF;
 RETURN bypass;
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_checked_post_bypass(text,bigint) FROM PUBLIC;
CREATE OR REPLACE FUNCTION content.pass_check_posting_cooldown(p_actor bytea,p_board text,p_thread bigint,p_has_attachment boolean,p_request_at bigint)
RETURNS TABLE(kind text,remaining_seconds bigint) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_public' THEN
 RAISE EXCEPTION 'Public cooldown authority unavailable' USING ERRCODE='42501'; END IF;
 RETURN QUERY SELECT * FROM content.check_posting_cooldown_core(p_actor,p_board,p_thread,p_has_attachment,p_request_at,
 verification_private.pass_checked_post_bypass(p_board,p_thread));
END $$;
CREATE FUNCTION content.pass_check_duplicate_image(p_actor bytea,p_board text,p_thread bigint,p_request bigint,p_job text,p_capability text)
RETURNS TABLE(kind text,remaining_seconds bigint) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE bypass boolean; digest text;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_public' THEN
 RAISE EXCEPTION 'Public cooldown authority unavailable' USING ERRCODE='42501'; END IF;
 bypass:=verification_private.pass_checked_post_bypass(p_board,p_thread);
 SELECT m.normalized_md5 INTO STRICT digest FROM content.attachment_upload_filter_metadata(p_job,p_capability,p_board,p_thread,false,false) m;
 IF digest IS NULL THEN RETURN; END IF;
 IF octet_length(digest)<>32 OR digest !~ '^[0-9a-f]{32}$' THEN
 RAISE EXCEPTION 'Attachment digest unavailable' USING ERRCODE='55000'; END IF;
 RETURN QUERY SELECT * FROM content.pass_duplicate_cooldown_core(p_actor,p_board,p_thread,p_request,decode(digest,'hex'),bypass);
END $$;
REVOKE ALL ON FUNCTION content.pass_check_duplicate_image(bytea,text,bigint,bigint,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_check_duplicate_image(bytea,text,bigint,bigint,text,text) TO board_public;
CREATE FUNCTION content.pass_check_janitor_duplicate(p_actor bytea,p_board text,p_thread bigint,p_request bigint,p_job text,p_capability text)
RETURNS TABLE(kind text,remaining_seconds bigint) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE digest text;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_staff' THEN
 RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000'; END IF;
 PERFORM verification_private.pass_gate();
 SELECT m.normalized_md5 INTO STRICT digest FROM content.attachment_upload_filter_metadata(p_job,p_capability,p_board,p_thread,false,false) m;
 IF digest IS NULL THEN RETURN; END IF;
 IF octet_length(digest)<>32 OR digest !~ '^[0-9a-f]{32}$' THEN
 RAISE EXCEPTION 'Attachment digest unavailable' USING ERRCODE='55000'; END IF;
 RETURN QUERY SELECT * FROM content.pass_duplicate_cooldown_core(p_actor,p_board,p_thread,p_request,decode(digest,'hex'),true);
END $$;
REVOKE ALL ON FUNCTION content.pass_check_janitor_duplicate(bytea,text,bigint,bigint,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_check_janitor_duplicate(bytea,text,bigint,bigint,text,text) TO board_staff;
CREATE OR REPLACE FUNCTION verification_private.pass_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='verification_private' AND c.relname LIKE 'pass\_%' ESCAPE '\'
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE (n.nspname IN('verification_private','content') AND p.proname LIKE 'pass\_%' ESCAPE '\')
   OR (n.nspname='content' AND p.proname='public_pass_ready')
   OR p.oid='content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean)'::regprocedure
   OR p.oid='content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean)'::regprocedure
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
    r.relrowsecurity,r.relforcerowsecurity,r.reloptions,
    verification_private.pass_acl(coalesce(r.relacl,acldefault(CASE WHEN r.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,r.relowner))),
    CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
      pg_get_expr(d.adbin,d.adrelid),verification_private.pass_acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated) ORDER BY k.conname)
     FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    verification_private.pass_acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
    ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(r.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)) ORDER BY r.identity,t.tgname)
    FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
   ORDER BY t.tgrelid::regclass::text,t.tgname) FROM pg_trigger t WHERE t.tgfoid='verification_private.pass_erase_post_identity()'::regprocedure),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
   ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype,t.tgconstrrelid::regclass::text)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(
    SELECT k.oid FROM pg_constraint k JOIN relations r ON r.oid=k.conrelid WHERE k.contype='f')),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),verification_private.pass_acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
    FROM pg_namespace n WHERE n.nspname='verification_private'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Duplicate-image authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Duplicate-image cooldowns changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Duplicate-image cooldowns changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Duplicate-image cooldowns changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Duplicate-image cooldowns changed posting authority'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Duplicate-image cooldowns changed report profile'; END IF; END $$;
