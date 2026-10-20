-- Private report identity foundation. Public report bypass is not enabled here.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass report identity'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass report identity'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass report identity'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass report identity'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass report identity'; END IF; END $$;
SET LOCAL ROLE board_report_admission_owner;
GRANT REFERENCES(report_id) ON post_secrets.report_membership TO board_anonymous_owner;
RESET ROLE;
DO $$ BEGIN IF has_schema_privilege('board_report_admission_owner','verification_private','USAGE') THEN
 RAISE EXCEPTION 'Unexpected report owner namespace grant'; END IF; END $$;
GRANT USAGE ON SCHEMA verification_private TO board_report_admission_owner;
GRANT CREATE ON SCHEMA verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE TABLE verification_private.pass_reports (
 report_id bigint PRIMARY KEY CHECK(report_id>0) REFERENCES post_secrets.report_membership(report_id) ON DELETE CASCADE,
 identity_hash bytea NOT NULL CHECK(octet_length(identity_hash)=32)
);
CREATE INDEX pass_reports_identity ON verification_private.pass_reports(identity_hash,report_id);
CREATE TABLE verification_private.pass_report_handoffs (
 registration_xid xid8 PRIMARY KEY,
 actor_hash bytea NOT NULL CHECK(octet_length(actor_hash)=32),
 session_hash bytea NOT NULL CHECK(octet_length(session_hash)=32),
 board text NOT NULL CHECK(board ~ '^[a-z0-9]{1,10}$'),
 post_id bigint NOT NULL CHECK(post_id>0),
 identity_hash bytea NOT NULL CHECK(octet_length(identity_hash)=32)
);
REVOKE ALL ON verification_private.pass_reports,verification_private.pass_report_handoffs FROM PUBLIC;
CREATE FUNCTION verification_private.pass_finish_report_handoff() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_TABLE_SCHEMA<>'verification_private' OR TG_TABLE_NAME<>'pass_report_handoffs' OR TG_OP<>'INSERT'
 OR TG_LEVEL<>'ROW' OR TG_WHEN<>'AFTER' OR TG_NARGS<>0 THEN
 RAISE EXCEPTION 'Invalid report membership cleanup' USING ERRCODE='23514'; END IF;
 DELETE FROM verification_private.pass_report_handoffs WHERE registration_xid=NEW.registration_xid AND registration_xid=pg_current_xact_id();
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_finish_report_handoff() FROM PUBLIC;
CREATE CONSTRAINT TRIGGER pass_report_handoff_cleanup AFTER INSERT ON verification_private.pass_report_handoffs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION verification_private.pass_finish_report_handoff();
-- Only the private owner stages this handoff until all admission checks exist.
CREATE FUNCTION verification_private.pass_consume_report_receipt(
 p_actor bytea,p_board text,p_post bigint,p_token bytea,p_receipt bytea,p_peer bytea,p_context bytea
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint; identity bytea;
BEGIN
 IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
 RAISE EXCEPTION 'Invalid report membership actor' USING ERRCODE='23514'; END IF;
 SELECT entitlement_id INTO STRICT account FROM verification_private.pass_take_receipt(p_receipt,p_peer,p_token,p_context,'report',p_board,p_post);
 SELECT token_hash INTO STRICT identity FROM verification_private.pass_entitlements WHERE id=account;
 INSERT INTO verification_private.pass_report_handoffs VALUES(pg_current_xact_id(),p_actor,p_token,p_board,p_post,identity);
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_consume_report_receipt(bytea,text,bigint,bytea,bytea,bytea,bytea) FROM PUBLIC;
CREATE FUNCTION verification_private.pass_capture_report_identity() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE identity bytea;
BEGIN
 IF TG_TABLE_SCHEMA<>'post_secrets' OR TG_TABLE_NAME<>'report_membership' OR TG_OP<>'INSERT'
 OR TG_LEVEL<>'ROW' OR TG_WHEN<>'AFTER' OR TG_NARGS<>0 THEN
 RAISE EXCEPTION 'Invalid report membership capture' USING ERRCODE='23514'; END IF;
 SELECT h.identity_hash INTO identity FROM verification_private.pass_report_handoffs h
 WHERE h.registration_xid=pg_current_xact_id() AND h.actor_hash=NEW.actor_hash AND h.board=NEW.board AND h.post_id=NEW.post_id;
 IF NOT FOUND THEN RETURN NULL; END IF;
 PERFORM verification_private.pass_gate();
 IF NEW.registration_xid<>pg_current_xact_id() THEN RAISE EXCEPTION 'Report admission provenance differs' USING ERRCODE='23514'; END IF;
 INSERT INTO verification_private.pass_reports VALUES(NEW.report_id,identity);
 DELETE FROM verification_private.pass_report_handoffs WHERE registration_xid=pg_current_xact_id();
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_capture_report_identity() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION verification_private.pass_capture_report_identity() TO board_report_admission_owner;
RESET ROLE;
SET LOCAL ROLE board_report_admission_owner;
REVOKE REFERENCES(report_id) ON post_secrets.report_membership FROM board_anonymous_owner;
CREATE TRIGGER pass_report_identity_capture AFTER INSERT ON post_secrets.report_membership
 FOR EACH ROW EXECUTE FUNCTION verification_private.pass_capture_report_identity();
RESET ROLE;
REVOKE USAGE ON SCHEMA verification_private FROM board_report_admission_owner;
SET LOCAL ROLE board_anonymous_owner;
REVOKE EXECUTE ON FUNCTION verification_private.pass_capture_report_identity() FROM board_report_admission_owner;
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
