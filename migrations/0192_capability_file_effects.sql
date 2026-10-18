-- A fresh-transaction finalizer for an already matched, unbadged file attempt.
-- The posting caller must roll back its original transaction before this call.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before capability file effects'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before capability file effects'; END IF;
END $$;
RESET ROLE;
-- Reject drift in the newly attested cross-owner dependencies before grants
-- change. Pins include owner, security mode, path, ACL and implementation.
SET LOCAL ROLE board_restriction_owner;
DO $dependencies$ DECLARE dependency record; actual text; BEGIN
 FOR dependency IN SELECT * FROM (VALUES
  ('admission.automatic_ban_exempt(inet)','ba6a78e67bf8381db133ca36c5637936c420d5c00b1257d4f46978629bb44817'),
  ('admission.file_effect_dmca(bytea)','a547ede753b00319e30e331c777bc171c397588f03b2f9b8a2f474f4c1ea6f51'),
  ('admission.file_effect_exempt(inet)','2c4bf398984ef92e57ff03f12f3024c85be83869c8358d274a1e0615add7eee2'),
  ('admission.record_dmca_warning(bytea,timestamptz)','0fb3247f3721d83906f343b0a8a405e197043f90bd594bdc4e9812a5f9b25791'),
  ('admission.resolve_file_effect_rule(text,bigint,bytea,bytea,boolean)','91f6c02ec322fd2f2f1f003468243c629c611115c543eb6f30f71875a265e875'),
  ('content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean)','0e0074a4bf0940e4c24deff9ce8cc2e9cd958c62b33006748ffa4aa5f6575402'),
  ('content.check_attachment_board_policy(text,bigint,bigint,integer,integer,text,text,bigint,boolean)','163c372ce6ff377add383374931bf10fba78ee0aabce78c762d2e338a1c7ca36'),
  ('content.check_attachment_upload(text,text)','5143fe66b48a331d5036b598238de31d402b01276758608d586875a07a359c3a'),
  ('content.revalidate_staff_refusal(bytea,bytea,integer,text)','801bc10e6e1e399077b8f569ed15a442ca5f51cab36fd37906e9138fb396e11e'),
  ('content.text_thread_transfer_ready()','9e55efb3dfc8b14a48b6bc86a01acb8a1ac93961112687c2e93c8a8d83386ebb')
 ) expected(signature,digest) LOOP
  SELECT encode(sha256(convert_to(jsonb_build_array(pg_get_userbyid(p.proowner),p.prosecdef,p.proconfig,
   restrictions.acl(coalesce(p.proacl,acldefault('f',p.proowner))),p.prosrc)::text,'UTF8')),'hex') INTO actual
   FROM pg_proc p WHERE p.oid=dependency.signature::regprocedure;
  IF actual IS DISTINCT FROM dependency.digest THEN
   RAISE EXCEPTION 'File effect dependency drifted: %',dependency.signature; END IF;
 END LOOP;
END $dependencies$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
GRANT EXECUTE ON FUNCTION content.text_thread_transfer_ready() TO board_restriction_owner;
RESET ROLE;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE FUNCTION content.file_effect_upload_metadata(
 p_job text,p_capability text,p_board text,p_parent bigint,p_authorized boolean
) RETURNS TABLE(filename text,normalized_md5 text,original_md5 bytea,original_eligible boolean,audit_filename text)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE m record; extension text;
BEGIN
 -- Exact-file effects are unbadged. A caller cannot claim a badge exemption or
 -- use the separate capcode-only /p/ image context through this entry point.
 SELECT * INTO STRICT m FROM content.attachment_upload_filter_metadata(p_job,p_capability,p_board,p_parent,p_authorized,false);
 SELECT CASE a.output_format WHEN 'jpeg' THEN 'jpg' ELSE a.output_format END INTO extension
  FROM media.assets a WHERE a.job_id=p_job AND a.state='approved' ORDER BY a.id LIMIT 1;
 IF NOT FOUND THEN RAISE EXCEPTION 'Approved file is unavailable.' USING ERRCODE='P0002'; END IF;
 RETURN QUERY SELECT m.filename,m.normalized_md5,m.original_md5,m.original_eligible,
  regexp_replace(m.filename,'\.[a-zA-Z0-9]+$','')||'.'||extension;
END $$;
REVOKE ALL ON FUNCTION content.file_effect_upload_metadata(text,text,text,bigint,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.file_effect_upload_metadata(text,text,text,bigint,boolean) TO board_restriction_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
SET LOCAL ROLE board_staff_post_owner;
GRANT EXECUTE ON FUNCTION content.revalidate_staff_refusal(bytea,bytea,integer,text) TO board_restriction_owner;
RESET ROLE;
GRANT CREATE ON SCHEMA restrictions,content TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE TABLE restrictions.file_clock (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 timezone text CHECK(timezone IS NULL OR octet_length(timezone) BETWEEN 1 AND 64)
);
-- The supplied source does not establish its deployment timezone. An operator
-- must select it before a non-exempt file ban can be issued.
INSERT INTO restrictions.file_clock DEFAULT VALUES;
REVOKE ALL ON restrictions.file_clock FROM PUBLIC;
CREATE FUNCTION restrictions.file_audit_reason(p_digest bytea,p_filename text) RETURNS text
LANGUAGE sql IMMUTABLE STRICT SET search_path=pg_catalog,pg_temp AS $$
 SELECT 'Blacklisted md5 - '||encode(p_digest,'hex')||' - Filename: '||
 replace(replace(replace(replace(replace(p_filename,'&','&amp;'),'<','&lt;'),'>','&gt;'),'"','&quot;'),'''','&#039;')
$$;
REVOKE ALL ON FUNCTION restrictions.file_audit_reason(bytea,text) FROM PUBLIC;
CREATE FUNCTION restrictions.file_quiet_post(p_board text,p_parent bigint) RETURNS bigint
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT CASE WHEN p_parent=0 THEN
  coalesce((SELECT max(thread_id) FROM content.posts WHERE board=p_board AND id<>thread_id AND NOT deleted),0)
 ELSE coalesce((SELECT CASE WHEN max(id)<9223372036854775807 THEN max(id)+1 ELSE 0 END
  FROM content.posts WHERE board=p_board AND NOT deleted),0) END
$$;
REVOKE ALL ON FUNCTION restrictions.file_quiet_post(text,bigint) FROM PUBLIC;
CREATE FUNCTION content.finalize_file_filter(
 p_actor bytea,p_peer inet,p_token bytea,p_board text,p_parent bigint,p_job text,p_capability text,
 p_revision bigint,p_rule bigint,p_digest bytea,p_expected jsonb,
 p_session bytea,p_csrf bytea,p_idle integer,p_cookie jsonb
) RETURNS TABLE(response text,diagnostic text,quiet_post bigint,cookie_refreshed boolean)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE m record; again record; r record; staff_role text; current_role_name text; authorized boolean:=false;
 v_repeat boolean:=false; v_exempt boolean; v_zone text; private_reason text; public_reason text;
 v_response text; v_diagnostic text; v_quiet bigint; v_cookie boolean:=false;
BEGIN
 IF session_user NOT IN('board_public','board_staff')
    OR current_setting('transaction_isolation')<>'read committed'
    OR p_actor IS NULL OR octet_length(p_actor)<>32
    OR p_peer IS NULL OR masklen(p_peer)<>(CASE family(p_peer) WHEN 4 THEN 32 ELSE 128 END)
    OR (p_token IS NOT NULL AND octet_length(p_token)<>32)
    OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_parent IS NULL OR p_parent<0
    OR p_rule IS NULL OR p_rule<1 OR p_digest IS NULL OR octet_length(p_digest)<>16
    OR p_expected IS NULL OR jsonb_typeof(p_expected)<>'object' OR octet_length(p_expected::text)>4096
    OR (p_cookie IS NOT NULL AND (jsonb_typeof(p_cookie)<>'object' OR octet_length(p_cookie::text)>1024)) THEN
  RAISE EXCEPTION 'Invalid file refusal context.' USING ERRCODE='23514'; END IF;
 IF session_user='board_public' AND (p_session IS NOT NULL OR p_csrf IS NOT NULL OR p_idle IS NOT NULL)
    OR session_user='board_staff' AND (p_token IS NOT NULL OR p_cookie IS NOT NULL) THEN
  RAISE EXCEPTION 'Invalid file refusal authority.' USING ERRCODE='28000'; END IF;
 PERFORM content.lock_posting_actor(p_actor,false);
 PERFORM restrictions.gate(true);
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'File refusal authority catalog is unavailable.' USING ERRCODE='55000'; END IF;
 PERFORM slug FROM content.boards WHERE slug=p_board AND NOT staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 -- Resolve against the original snapshot first to acquire the policy lock
 -- before media locks. It will be resolved again from actual upload metadata.
 SELECT * INTO r FROM admission.resolve_file_effect_rule(p_board,p_revision,
  decode(p_expected->>'normalized','hex'),decode(p_expected->>'original','hex'),(p_expected->>'eligible')::boolean);
 IF NOT FOUND OR r.rule_id<>p_rule OR r.digest<>p_digest THEN
  RAISE EXCEPTION 'File refusal policy changed.' USING ERRCODE='55000'; END IF;
 IF session_user='board_staff' THEN
  SELECT a.role INTO staff_role FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id
   WHERE s.token_hash=p_session;
  IF NOT FOUND THEN RAISE EXCEPTION 'Staff refusal authority changed.' USING ERRCODE='28000'; END IF;
  authorized:=staff_role IN('moderator','manager','admin');
 END IF;
 SELECT * INTO STRICT m FROM content.file_effect_upload_metadata(p_job,p_capability,p_board,p_parent,authorized);
 IF jsonb_build_object('filename',m.filename,'normalized',m.normalized_md5,
      'original',encode(m.original_md5,'hex'),'eligible',m.original_eligible) IS DISTINCT FROM p_expected THEN
  RAISE EXCEPTION 'File refusal upload changed.' USING ERRCODE='55000'; END IF;
 SELECT * INTO r FROM admission.resolve_file_effect_rule(p_board,p_revision,
  decode(m.normalized_md5,'hex'),m.original_md5,m.original_eligible);
 IF NOT FOUND OR r.rule_id<>p_rule OR r.digest<>p_digest THEN
  RAISE EXCEPTION 'File refusal match changed.' USING ERRCODE='55000'; END IF;
 IF session_user='board_staff' THEN
  PERFORM content.revalidate_staff_refusal(p_session,p_csrf,p_idle,p_board);
  SELECT a.role INTO current_role_name FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id
   WHERE s.token_hash=p_session;
  IF current_role_name IS DISTINCT FROM staff_role THEN
   RAISE EXCEPTION 'Staff refusal limits changed.' USING ERRCODE='28000'; END IF;
 END IF;
 private_reason:=restrictions.file_audit_reason(r.digest,m.audit_filename);
 -- Never disclose the private audit reason when the source's optional
 -- public reason is empty (including PHP's false string "0").
 public_reason:=CASE WHEN r.public_reason IN('','0') THEN 'Posting a blocked file.' ELSE r.public_reason END;
 IF r.mode=2 THEN
  v_repeat:=admission.file_effect_dmca(p_actor);
  v_response:=CASE WHEN v_repeat THEN 'banned' ELSE 'dmca' END;
  IF v_repeat THEN
   private_reason:='DMCA complaint from '||r.description||' (blacklist ID: '||r.rule_id||')';
   public_reason:='Repeatedly uploading image(s) subject to a copyright infringement claim.';
  END IF;
 ELSIF r.mode=0 AND p_board='test' THEN
  v_response:='diagnostic';
  -- Plain text for the normal escaping response renderer; stored audit text
  -- remains source-escaped and is never used as trusted response markup.
  v_diagnostic:='Blacklisted md5 - '||encode(r.digest,'hex')||' - Filename: '||m.audit_filename;
 ELSE
  v_response:=CASE WHEN r.quiet THEN 'quiet' ELSE 'failed' END;
 END IF;
 IF r.mode=1 OR (r.mode=2 AND v_repeat) THEN
  v_exempt:=admission.file_effect_exempt(p_peer);
  IF NOT v_exempt THEN
   SELECT timezone INTO v_zone FROM restrictions.file_clock WHERE singleton FOR SHARE;
   IF NOT FOUND OR v_zone IS NULL THEN
    RAISE EXCEPTION 'File ban timezone is not configured.' USING ERRCODE='55000'; END IF;
   PERFORM restrictions.record_file_ban(p_actor,p_token,p_board,r.rule_id,r.digest,
    CASE WHEN r.mode=2 THEN 3 ELSE r.ban_days END,r.mode=2,public_reason,private_reason,v_zone);
  END IF;
 END IF;
 IF v_response='quiet' THEN v_quiet:=restrictions.file_quiet_post(p_board,p_parent); END IF;
 IF p_cookie IS NOT NULL THEN
  BEGIN
   PERFORM post_secrets.resume_anonymous_refusal(p_token,decode(p_cookie->>'network','hex'),
    decode(p_cookie->>'address','hex'),decode(p_cookie->>'environment','hex'),
    (p_cookie->>'minted')::boolean,(p_cookie->>'now')::bigint);
   v_cookie:=true;
  EXCEPTION WHEN OTHERS THEN
   -- A failed optional refresh does not erase a definite file refusal.
   v_cookie:=false;
  END;
 END IF;
 -- Both time-bound capabilities are checked again after any effect/cookie wait.
 SELECT * INTO STRICT again FROM content.file_effect_upload_metadata(p_job,p_capability,p_board,p_parent,authorized);
 IF to_jsonb(again) IS DISTINCT FROM to_jsonb(m) THEN
  RAISE EXCEPTION 'File refusal upload changed before commit.' USING ERRCODE='55000'; END IF;
 IF session_user='board_staff' THEN
  PERFORM content.revalidate_staff_refusal(p_session,p_csrf,p_idle,p_board);
 END IF;
 RETURN QUERY SELECT v_response,v_diagnostic,v_quiet,v_cookie;
END $$;
REVOKE ALL ON FUNCTION content.finalize_file_filter(bytea,inet,bytea,text,bigint,text,text,bigint,bigint,bytea,jsonb,bytea,bytea,integer,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.finalize_file_filter(bytea,inet,bytea,text,bigint,text,text,bigint,bigint,bytea,jsonb,bytea,bytea,integer,jsonb) TO board_public,board_staff;
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
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Capability file effects changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Capability file effects changed restriction authority'; END IF; END $$;
RESET ROLE;
