-- The classifier catalog seals board columns and their privileges too.
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Classifier history catalog is missing'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Classifier history drifted before Pass cooldowns'; END IF; END $$;
RESET ROLE;
-- Preserve the source Pass cooldown gate using consumed private authority.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before Pass cooldown admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Pass catalog drifted before post consumption'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before Pass cooldown admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_challenge_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Public challenge authority is unavailable'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid='content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean)'::regprocedure
  AND proowner='board_posting_cooldown_owner'::regrole AND prosecdef
  AND proconfig=ARRAY['search_path=pg_catalog, pg_temp']::text[]
  AND encode(sha256(convert_to(prosrc,'UTF8')),'hex')='1d2e54d9fe999f6e98e9998f1cdd83847df6befb37205414dae32994a47ff3e6'
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(proacl,acldefault('f',proowner))) a WHERE a.grantee<>proowner)) THEN
  RAISE EXCEPTION 'Cooldown core drifted before membership admission'; END IF;
END $$;
SET LOCAL ROLE board_posting_cooldown_owner;
GRANT EXECUTE ON FUNCTION content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean) TO board_anonymous_owner;
RESET ROLE;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION verification_private.pass_checked_receipt(
 p_receipt bytea,p_peer bytea,p_anonymous bytea,p_context bytea,p_purpose text,p_board text,p_target bigint
) RETURNS verification_private.pass_receipts LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE receipt verification_private.pass_receipts%ROWTYPE; stamp timestamptz; account bigint;
BEGIN
 PERFORM verification_private.pass_gate();
 IF p_receipt IS NULL OR octet_length(p_receipt)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
  OR p_anonymous IS NULL OR octet_length(p_anonymous)<>32 OR p_context IS NULL OR octet_length(p_context)<>32
  OR p_purpose IS NULL OR p_board IS NULL OR p_target IS NULL THEN
  RAISE EXCEPTION 'Invalid membership receipt context' USING ERRCODE='23514'; END IF;
 -- This first read grants nothing. Re-read under all write locks below.
 SELECT * INTO receipt FROM verification_private.pass_receipts WHERE token_hash=p_receipt;
 IF NOT FOUND THEN RAISE EXCEPTION 'Membership receipt unavailable' USING ERRCODE='P0166'; END IF;
 PERFORM verification_private.pass_lock_write_scope(receipt.pass_session,p_anonymous,p_board);
 SELECT * INTO receipt FROM verification_private.pass_receipts WHERE token_hash=p_receipt FOR UPDATE;
 stamp:=clock_timestamp();
 IF NOT FOUND OR receipt.peer<>p_peer OR receipt.anonymous_session<>p_anonymous
  OR receipt.context_hash<>p_context OR receipt.purpose<>p_purpose OR receipt.board<>p_board
  OR receipt.target<>p_target OR receipt.expires_at<=stamp THEN
  RAISE EXCEPTION 'Membership receipt unavailable' USING ERRCODE='P0166'; END IF;
 -- The private helper cannot invoke an issuer-only wrapper as board_public.
 -- Repeat its source order against the account and session held above.
 SELECT s.entitlement_id INTO account FROM verification_private.pass_sessions s
 JOIN verification_private.pass_entitlements e ON e.id=s.entitlement_id
 WHERE s.token_hash=receipt.pass_session AND s.expires_at>stamp
  AND s.credential_version=e.credential_version AND e.expires_at>stamp AND e.status=0
  AND (e.posting_scope IS NULL OR e.posting_scope IS NOT DISTINCT FROM receipt.posting_scope
    OR e.last_used<=date_trunc('second',stamp)-interval '1800 seconds');
 IF NOT FOUND THEN RAISE EXCEPTION 'Membership authorization changed' USING ERRCODE='P0166'; END IF;
 RETURN receipt;
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_checked_receipt(bytea,bytea,bytea,bytea,text,text,bigint) FROM PUBLIC;
CREATE OR REPLACE FUNCTION verification_private.pass_take_receipt(
 p_receipt bytea,p_peer bytea,p_anonymous bytea,p_context bytea,p_purpose text,p_board text,p_target bigint
) RETURNS TABLE(entitlement_id bigint,purchase_year integer) LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE receipt verification_private.pass_receipts%ROWTYPE; stamp timestamptz; account bigint;
BEGIN
 receipt:=verification_private.pass_checked_receipt(p_receipt,p_peer,p_anonymous,p_context,p_purpose,p_board,p_target);
 stamp:=clock_timestamp();
 SELECT s.entitlement_id INTO STRICT account FROM verification_private.pass_sessions s WHERE s.token_hash=receipt.pass_session;
 DELETE FROM verification_private.pass_receipts WHERE token_hash=p_receipt;
 UPDATE verification_private.pass_entitlements SET last_peer=receipt.peer,login_scope=receipt.login_scope,
  posting_scope=receipt.posting_scope,last_used=date_trunc('second',stamp) WHERE id=account;
 RETURN QUERY SELECT e.id,extract(year FROM e.purchased_at AT TIME ZONE 'America/New_York')::integer
  FROM verification_private.pass_entitlements e WHERE e.id=account;
END $$;
-- The caller has taken actor/restriction gates and board/session locks. This
-- read retains private receipt/account locks without consuming the receipt.
CREATE FUNCTION content.pass_check_posting_cooldown(
 p_actor bytea,p_board text,p_thread bigint,p_has_attachment boolean,p_request_at bigint
) RETURNS TABLE(kind text,remaining_seconds bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE enabled boolean; bypass boolean; raw text; proof jsonb; key text;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_public' THEN
  RAISE EXCEPTION 'Public cooldown authority unavailable' USING ERRCODE='42501'; END IF;
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
 RETURN QUERY SELECT * FROM content.check_posting_cooldown_core(p_actor,p_board,p_thread,p_has_attachment,p_request_at,bypass);
END $$;
REVOKE ALL ON FUNCTION content.pass_check_posting_cooldown(bytea,text,bigint,boolean,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_check_posting_cooldown(bytea,text,bigint,boolean,bigint) TO board_public;
CREATE OR REPLACE FUNCTION verification_private.pass_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='verification_private' AND c.relname LIKE 'pass\_%' ESCAPE '\'
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE (n.nspname IN('verification_private','content') AND p.proname LIKE 'pass\_%' ESCAPE '\')
   OR (n.nspname='content' AND p.proname='public_pass_ready')
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
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),verification_private.pass_acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
    FROM pg_namespace n WHERE n.nspname='verification_private'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass post authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Pass cooldown admission changed transfer authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Pass cooldown admission changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_challenge_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Pass cooldown admission changed challenge authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Pass cooldown admission changed restriction authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_classifier_history_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog
 SET definition_hash=decode(current_setting('board.migration_classifier_history_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Pass cooldowns changed classifier history authority'; END IF; END $$;
RESET ROLE;
