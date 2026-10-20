-- Connect purpose-bound Pass receipts to public report admission.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass report admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass report admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass report admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass report admission'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass report admission'; END IF; END $$;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION content.consume_report_challenge(p_board text,p_post bigint,p_actor bytea,p_token bytea,
 p_network bytea,p_address bytea,p_environment bytea,p_request bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE invoker text; enabled boolean; allow_credits boolean; raw text; proof jsonb; key text;
 receipt verification_private.receipts%ROWTYPE; profile record;
BEGIN
 invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 raw:=current_setting('board.challenge_context',true);
 PERFORM set_config('board.challenge_context','',true);
 IF invoker='board_migrator' THEN RETURN; END IF;
 IF invoker<>'board_public' THEN RAISE EXCEPTION 'Report challenge authority is unavailable.' USING ERRCODE='42501'; END IF;
 SELECT b.captcha_enabled,b.captcha_twister AND b.captcha_allow_bypass AND b.slug NOT IN('pol','trash','bant')
 INTO enabled,allow_credits FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 -- A recognized Pass receipt takes precedence over optional CAPTCHA bypass.
 -- Other proof handling, including disabled-board behavior, stays unchanged.
 IF raw IS NOT NULL AND raw<>'' AND octet_length(raw)<=1024 THEN
  BEGIN proof:=raw::jsonb; EXCEPTION WHEN invalid_text_representation THEN proof:=NULL; END;
  IF jsonb_typeof(proof)='object' AND coalesce(proof->>'receipt','') ~ '^[0-9a-f]{64}$' THEN
   IF EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=decode(proof->>'receipt','hex')) THEN
    IF (SELECT count(*) FROM jsonb_object_keys(proof))<>4 THEN RAISE EXCEPTION 'Invalid report membership proof' USING ERRCODE='P0166'; END IF;
    FOREACH key IN ARRAY ARRAY['receipt','peer','session','context'] LOOP
     IF jsonb_typeof(proof->key) IS DISTINCT FROM 'string' OR coalesce(proof->>key,'') !~ '^[0-9a-f]{64}$' THEN
      RAISE EXCEPTION 'Invalid report membership proof' USING ERRCODE='P0166'; END IF;
    END LOOP;
    IF p_token IS NULL OR decode(proof->>'session','hex')<>p_token THEN RAISE EXCEPTION 'Report membership session changed' USING ERRCODE='P0166'; END IF;
    PERFORM verification_private.pass_consume_report_receipt(p_actor,p_board,p_post,p_token,
     decode(proof->>'receipt','hex'),decode(proof->>'peer','hex'),decode(proof->>'context','hex'));
    RETURN;
   END IF;
  END IF;
 END IF;
 IF NOT enabled THEN RETURN; END IF;
 IF verification_private.report_exemption(p_board,p_post,p_actor,p_token,p_network,p_address,p_environment,p_request) THEN RETURN; END IF;
 IF raw IS NULL OR raw='' OR octet_length(raw)>1024 THEN
  RAISE EXCEPTION 'Challenge verification is required.' USING ERRCODE='P0166'; END IF;
 BEGIN proof:=raw::jsonb;
 EXCEPTION WHEN invalid_text_representation THEN
  RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END;
 IF jsonb_typeof(proof)<>'object' THEN RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 IF (SELECT count(*) FROM jsonb_object_keys(proof))<>4 THEN RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 FOREACH key IN ARRAY ARRAY['receipt','peer','session','context'] LOOP
  IF jsonb_typeof(proof->key) IS DISTINCT FROM 'string' OR octet_length(proof->>key)<>64 OR (proof->>key) !~ '^[0-9a-f]{64}$' THEN
   RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 END LOOP;
 IF p_token IS NULL OR decode(proof->>'session','hex')<>p_token THEN
  RAISE EXCEPTION 'Challenge session changed.' USING ERRCODE='P0166'; END IF;
 receipt:=verification_private.take_write_receipt_data(decode(proof->>'receipt','hex'),decode(proof->>'peer','hex'),p_token,
  'report',p_board,p_post,decode(proof->>'context','hex'));
 IF receipt.from_credit THEN
  IF NOT allow_credits OR NOT (SELECT current_score>=80 FROM content.verification_risk(receipt.peer_hash,NULL)) THEN
   RAISE EXCEPTION 'Challenge credit is unavailable.' USING ERRCODE='P0166'; END IF;
  SELECT * INTO profile FROM verification_private.credit_profile(p_token,p_network,p_address,p_environment,p_request);
  IF NOT profile.can_spend OR profile.identity_key<>receipt.credit_identity THEN
   RAISE EXCEPTION 'Challenge credit is unavailable.' USING ERRCODE='P0166'; END IF;
  UPDATE verification_private.credits SET remaining=remaining-1
   WHERE identity_key=profile.identity_key AND remaining>0 AND expires_at>clock_timestamp();
  IF NOT FOUND THEN RAISE EXCEPTION 'Challenge credit is unavailable.' USING ERRCODE='P0166'; END IF;
 END IF;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
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
   OR p.oid='content.consume_report_challenge(text,bigint,bytea,bytea,bytea,bytea,bytea,bigint)'::regprocedure
   OR p.oid='post_secrets.record_current_report_clear(bigint)'::regprocedure
   OR p.oid='post_secrets.check_report_limits(text,bigint,bytea,uuid,timestamp with time zone)'::regprocedure
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
   ORDER BY t.tgrelid::regclass::text,t.tgname) FROM pg_trigger t WHERE t.tgfoid IN('verification_private.pass_erase_post_identity()'::regprocedure,'verification_private.pass_capture_report_identity()'::regprocedure)),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
   ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype,t.tgconstrrelid::regclass::text)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(
    SELECT k.oid FROM pg_constraint k JOIN relations r ON r.oid=k.conrelid WHERE k.contype='f')),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
   ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype,t.tgconstrrelid::regclass::text)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(
    SELECT k.oid FROM pg_constraint k JOIN relations r ON r.oid=k.conrelid WHERE k.contype='f')),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),verification_private.pass_acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
    FROM pg_namespace n WHERE n.nspname='verification_private'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed posting authority'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed report profile'; END IF; END $$;
