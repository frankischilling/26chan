-- Request-only template configuration and an exact-weight unlock predicate.
-- No runtime request writer or new cross-board grant is installed here.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction authority drifted before janitor requests'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report weight authority drifted before janitor requests'; END IF; END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA post_secrets TO board_report_admission_owner;
SET LOCAL ROLE board_report_admission_owner;

CREATE FUNCTION post_secrets.janitor_request_unlocked(p_board text,p_post bigint)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE is_op boolean; ids bigint[]; known bigint; weight double precision;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed'
  OR content.report_profile_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Request weight evidence is unavailable' USING ERRCODE='P0206'; END IF;
 IF p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$' OR p_post IS NULL OR p_post<=0 THEN
  RAISE EXCEPTION 'Invalid request target' USING ERRCODE='23514'; END IF;
 -- Reenter the caller's board lock. This also serializes report admissions and
 -- reporter retirement; no report admission/session lock is acquired afterward.
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RETURN false; END IF;
 SELECT p.id=p.thread_id INTO is_op FROM content.posts p
  JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
  WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted AND NOT t.deleted
   AND t.archived_at IS NULL;
 IF NOT FOUND THEN RETURN false; END IF;
 -- Source reads retained reports, including group-cleared reports. Native
 -- reporter retirement corresponds to source removal, so exclude those rows.
 SELECT array_agg(r.id ORDER BY r.id) INTO ids FROM (
  SELECT id FROM content.reports WHERE board=p_board AND post_id=p_post
   AND reporter_cleared_at IS NULL ORDER BY id LIMIT 10001
 ) r;
 IF ids IS NULL THEN RETURN false; END IF;
 IF cardinality(ids)>10000 THEN
  RAISE EXCEPTION 'Request report evidence exceeds its bound' USING ERRCODE='54000'; END IF;
 SELECT count(e.effective_weight),sum(e.effective_weight) INTO known,weight
  FROM post_secrets.report_weight_evidence e WHERE e.report_id=ANY(ids);
 IF known<>cardinality(ids) OR weight IS NULL
  OR weight IN('Infinity'::double precision,'-Infinity'::double precision,'NaN'::double precision) THEN
  RETURN false;
 END IF;
 -- Current persisted proofs are exactly 0.5, so both the sum and OP multiplier
 -- are exact binary values. Do not substitute category bases or displayed counts.
 RETURN ceil(weight*CASE WHEN is_op THEN 1.25 ELSE 1.0 END)>=1500;
END $$;
REVOKE ALL ON FUNCTION post_secrets.janitor_request_unlocked(text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION post_secrets.janitor_request_unlocked(text,bigint) TO board_restriction_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_report_admission_owner;

GRANT CREATE ON SCHEMA restrictions TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
ALTER TABLE restrictions.templates
 ADD COLUMN request_can_warn boolean NOT NULL DEFAULT false,
 ADD COLUMN request_save_policy text CHECK(request_save_policy IN('none','json_only','everything'));
-- NULL means unconfigured. A future request writer must require an explicit
-- policy rather than infer private source configuration from an existing ban.

CREATE OR REPLACE FUNCTION restrictions.catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='restrictions' OR c.oid IN('post_secrets.posting_history'::regclass,
    'admission.policy'::regclass,'admission.file_rules'::regclass,
    'admission.automatic_ban_exemptions'::regclass,'admission.dmca_warnings'::regclass,
    'admission.file_effect_capacity'::regclass,'content.posts'::regclass,'content.threads'::regclass,
    'content.reports'::regclass,'post_secrets.report_weight_evidence'::regclass)
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
    OR p.oid IN('post_secrets.janitor_request_unlocked(text,bigint)'::regprocedure,
       'content.report_profile_ready()'::regprocedure,
       'content.lock_automatic_range_policy(boolean)'::regprocedure,
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
REVOKE CREATE ON SCHEMA restrictions FROM board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Janitor request foundation changed restriction authority'; END IF; END $$;
RESET ROLE;
