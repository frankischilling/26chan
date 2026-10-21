-- Bind public VIP badges to a private receipt and the completed anonymous post.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before VIP posting'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before VIP posting'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before VIP posting'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before VIP posting'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before VIP posting'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before posting integration'; END IF;
END $$;
RESET ROLE;
ALTER TABLE content.posts DROP CONSTRAINT posts_capcode_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_capcode_check CHECK (
 capcode IS NULL OR capcode IN ('mod','admin','admin_highlight','manager','developer','founder','verified'));
ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_snapshot_capcode_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_snapshot_capcode_check CHECK (
 snapshot_capcode IN ('mod','admin','admin_highlight','manager','developer','founder','verified'));
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE TABLE verification_private.vip_post_handoffs (
 post_id bigint PRIMARY KEY CHECK(post_id>0),
 session_hash bytea NOT NULL CHECK(octet_length(session_hash)=32),
 registration_xid xid8 NOT NULL
);
REVOKE ALL ON verification_private.vip_post_handoffs FROM PUBLIC;

CREATE FUNCTION verification_private.vip_checked_post(p_board text,p_target bigint,p_post bigint)
RETURNS verification_private.vip_receipts LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE raw text; proof jsonb; key text; candidate verification_private.vip_receipts%ROWTYPE;
 receipt verification_private.vip_receipts%ROWTYPE; account verification_private.vip_accounts%ROWTYPE;
 enabled boolean; pass_proof jsonb;
BEGIN
 raw:=current_setting('board.vip_context',true);
 IF raw IS NULL OR raw='' THEN RETURN NULL; END IF;
 PERFORM verification_private.vip_gate();
 IF p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_target IS NULL OR p_target<0
  OR (p_post IS NOT NULL AND p_post<=0) OR octet_length(raw)>1024 THEN
  RAISE EXCEPTION 'VIP verification changed. Try again.' USING ERRCODE='P0239'; END IF;
 BEGIN proof:=raw::jsonb;
 EXCEPTION WHEN invalid_text_representation THEN
  RAISE EXCEPTION 'VIP verification changed. Try again.' USING ERRCODE='P0239'; END;
 IF jsonb_typeof(proof)<>'object' THEN RAISE EXCEPTION 'Invalid VIP context' USING ERRCODE='P0239'; END IF;
 IF (SELECT count(*) FROM jsonb_object_keys(proof))<>4 THEN RAISE EXCEPTION 'Invalid VIP context' USING ERRCODE='P0239'; END IF;
 FOREACH key IN ARRAY ARRAY['receipt','peer','session','context'] LOOP
  IF jsonb_typeof(proof->key) IS DISTINCT FROM 'string' OR octet_length(proof->>key)<>64
   OR (proof->>key) !~ '^[0-9a-f]{64}$' THEN RAISE EXCEPTION 'Invalid VIP context' USING ERRCODE='P0239'; END IF;
 END LOOP;
 SELECT b.captcha_enabled INTO enabled FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found' USING ERRCODE='P0002'; END IF;
 IF p_post IS NULL THEN
  IF NOT verification_private.pass_checked_post_bypass(p_board,p_target) THEN
   RAISE EXCEPTION 'VIP verification changed. Try again.' USING ERRCODE='P0239'; END IF;
  IF enabled THEN
   pass_proof:=current_setting('board.challenge_context',true)::jsonb;
   IF pass_proof->>'session' IS DISTINCT FROM proof->>'session'
    OR pass_proof->>'peer' IS DISTINCT FROM proof->>'peer' THEN
    RAISE EXCEPTION 'VIP verification context differs' USING ERRCODE='P0239'; END IF;
  END IF;
 ELSIF enabled AND NOT EXISTS(
  SELECT 1 FROM verification_private.pass_post_handoffs h
  JOIN verification_private.post_handoffs p ON p.post_id=h.post_id
  WHERE h.post_id=p_post AND h.session_hash=decode(proof->>'session','hex')
   AND p.session_hash=h.session_hash AND p.peer_hash=decode(proof->>'peer','hex')
   AND h.registration_xid=pg_current_xact_id() AND p.registration_xid=pg_current_xact_id()) THEN
  RAISE EXCEPTION 'VIP posting requires current CAPTCHA bypass' USING ERRCODE='P0239';
 END IF;
 SELECT * INTO candidate FROM verification_private.vip_receipts WHERE token_hash=decode(proof->>'receipt','hex');
 IF NOT FOUND THEN RAISE EXCEPTION 'VIP receipt unavailable' USING ERRCODE='P0239'; END IF;
 SELECT * INTO account FROM verification_private.vip_accounts WHERE id=candidate.account_id FOR SHARE;
 IF NOT FOUND OR NOT account.active OR account.credential_version<>candidate.credential_version THEN
  RAISE EXCEPTION 'VIP credentials changed' USING ERRCODE='P0239'; END IF;
 SELECT * INTO receipt FROM verification_private.vip_receipts WHERE token_hash=candidate.token_hash FOR UPDATE;
 IF NOT FOUND OR receipt IS DISTINCT FROM candidate OR receipt.expires_at<=clock_timestamp()
  OR receipt.peer<>decode(proof->>'peer','hex') OR receipt.anonymous_session<>decode(proof->>'session','hex')
  OR receipt.context_hash<>decode(proof->>'context','hex') OR receipt.board<>p_board OR receipt.target<>p_target THEN
  RAISE EXCEPTION 'VIP receipt context changed' USING ERRCODE='P0239'; END IF;
 RETURN receipt;
END $$;

CREATE FUNCTION content.vip_posting_name(p_board text,p_target bigint,p_session bytea)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE receipt verification_private.vip_receipts%ROWTYPE;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_public' THEN
  RAISE EXCEPTION 'Public VIP admission unavailable' USING ERRCODE='42501'; END IF;
 receipt:=verification_private.vip_checked_post(p_board,p_target,NULL);
 IF receipt.token_hash IS NULL THEN RETURN NULL; END IF;
 IF p_session IS NULL OR receipt.anonymous_session<>p_session THEN
  RAISE EXCEPTION 'VIP anonymous context differs' USING ERRCODE='P0239'; END IF;
 RETURN (SELECT name FROM verification_private.vip_accounts WHERE id=receipt.account_id);
END $$;

CREATE FUNCTION content.vip_apply_capcode() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog,pg_temp AS $$
DECLARE receipt verification_private.vip_receipts%ROWTYPE; invoker text;
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_TABLE_NAME<>'posts' OR TG_OP<>'INSERT'
  OR TG_LEVEL<>'ROW' OR TG_WHEN<>'BEFORE' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid VIP posting trigger' USING ERRCODE='23514'; END IF;
 invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 IF invoker<>'board_public' THEN RETURN NEW; END IF;
 -- Independent of the staff trigger: a raw public capcode never grants a badge.
 NEW.capcode:=NULL;
 receipt:=verification_private.vip_checked_post(NEW.board,CASE WHEN NEW.id=NEW.thread_id THEN 0 ELSE NEW.thread_id END,NEW.id);
 PERFORM set_config('board.vip_context','',true);
 IF receipt.token_hash IS NULL THEN RETURN NEW; END IF;
 SELECT name INTO STRICT NEW.name FROM verification_private.vip_accounts WHERE id=receipt.account_id;
 NEW.capcode:='verified';
 NEW.staff_authorized_limits:=false;
 PERFORM set_config('board.post_trip','',true);
 DELETE FROM verification_private.vip_receipts WHERE token_hash=receipt.token_hash;
 INSERT INTO verification_private.vip_post_handoffs VALUES(NEW.id,receipt.anonymous_session,pg_current_xact_id());
 RETURN NEW;
END $$;

CREATE FUNCTION verification_private.vip_finish_post_handoff() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_TABLE_SCHEMA<>'verification_private' OR TG_TABLE_NAME<>'vip_post_handoffs' OR TG_OP<>'INSERT'
  OR TG_LEVEL<>'ROW' OR TG_WHEN<>'AFTER' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid VIP handoff trigger' USING ERRCODE='23514'; END IF;
 IF NEW.registration_xid<>pg_current_xact_id() OR NOT EXISTS(
  SELECT 1 FROM post_secrets.anonymous_posts p WHERE p.post_id=NEW.post_id AND p.token_hash=NEW.session_hash) THEN
  RAISE EXCEPTION 'VIP post lacks matching anonymous registration' USING ERRCODE='P0239'; END IF;
 DELETE FROM verification_private.vip_post_handoffs WHERE post_id=NEW.post_id AND registration_xid=pg_current_xact_id();
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER vip_post_handoff_cleanup AFTER INSERT ON verification_private.vip_post_handoffs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION verification_private.vip_finish_post_handoff();
REVOKE ALL ON FUNCTION verification_private.vip_checked_post(text,bigint,bigint),
 content.vip_posting_name(text,bigint,bytea),content.vip_apply_capcode(),verification_private.vip_finish_post_handoff() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.vip_posting_name(text,bigint,bytea) TO board_public;
GRANT EXECUTE ON FUNCTION content.vip_apply_capcode() TO board_migrator;
RESET ROLE;
CREATE TRIGGER ab_apply_vip_capcode BEFORE INSERT ON content.posts
 FOR EACH ROW EXECUTE FUNCTION content.vip_apply_capcode();
SET LOCAL ROLE board_anonymous_owner;
REVOKE EXECUTE ON FUNCTION content.vip_apply_capcode() FROM board_migrator;
RESET ROLE;

CREATE OR REPLACE FUNCTION content.apply_forced_anonymous() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_forced boolean;
BEGIN
    SELECT forced_anon INTO v_forced FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
    IF v_forced THEN
        IF NEW.capcode IS DISTINCT FROM 'verified' AND NOT (current_user='board_staff' AND coalesce(current_setting('board.staff_is_admin',true),'false')='true') THEN
            NEW.name:='Anonymous';
        END IF;
        NEW.subject:='';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION content.apply_poster_id() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_enabled boolean; v_meta boolean; v_no_heaven boolean; v_network text;
BEGIN
    NEW.json_op_poster_id:=NULL;
    SELECT user_ids,meta_board,poster_id_no_heaven INTO v_enabled,v_meta,v_no_heaven
        FROM content.boards WHERE slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503';
    END IF;
    IF NEW.capcode IS NOT NULL THEN
        NEW.poster_id:=CASE WHEN v_enabled THEN CASE NEW.capcode
            WHEN 'mod' THEN 'Mod'
            WHEN 'admin' THEN 'Admin'
            WHEN 'admin_highlight' THEN 'Admin'
            WHEN 'manager' THEN 'Manager'
            WHEN 'developer' THEN 'Developer'
            WHEN 'founder' THEN 'Founder'
            WHEN 'verified' THEN 'Verified'
            ELSE NULL END ELSE NULL END;
        RETURN NEW;
    END IF;
    IF NOT v_enabled THEN NEW.poster_id:=NULL; RETURN NEW; END IF;
    v_network:=nullif(current_setting('board.poster_id',true),'');
    IF v_network IS NULL OR v_network !~ '^[+/0-9A-Za-z]{8}$' THEN
        RAISE EXCEPTION 'Poster identity is unavailable.' USING ERRCODE='23514';
    END IF;
    NEW.poster_id:=CASE WHEN current_setting('board.post_sage',true)='true'
        AND NOT v_meta AND NOT v_no_heaven THEN 'Heaven' ELSE v_network END;
    IF NEW.id=NEW.thread_id THEN NEW.json_op_poster_id:=v_network; END IF;
    RETURN NEW;
END $$;

SET LOCAL ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION verification_private.vip_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
WITH relations AS (
    SELECT c.*,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname='verification_private' AND c.relname LIKE 'vip\_%' ESCAPE '\'
), funcs AS (
    SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname IN('verification_private','content')
        AND (p.proname LIKE 'vip\_%' ESCAPE '\' OR p.proname='public_vip_ready'
            OR p.oid IN('content.apply_forced_anonymous()'::regprocedure,'content.apply_poster_id()'::regprocedure))
)
SELECT sha256(convert_to(jsonb_build_array(
    (SELECT jsonb_agg(jsonb_build_array(t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)) ORDER BY t.tgname)
     FROM pg_trigger t WHERE t.tgfoid='content.vip_apply_capcode()'::regprocedure),
    (SELECT jsonb_agg(jsonb_build_array(k.conrelid::regclass::text,k.conname,pg_get_constraintdef(k.oid,true),k.convalidated) ORDER BY k.conname)
     FROM pg_constraint k WHERE (k.conrelid='content.posts'::regclass AND k.conname='posts_capcode_check')
      OR (k.conrelid='content.moderation_audit'::regclass AND k.conname='moderation_audit_snapshot_capcode_check')),
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
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'VIP posting changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'VIP posting changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'VIP posting changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'VIP posting changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'VIP posting changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'VIP posting changed challenge authority'; END IF; END $$;
RESET ROLE;
