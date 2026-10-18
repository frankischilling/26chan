-- A range refusal reissues fresh or resumed anonymous state without recording
-- a successful action. Runtime callers cannot choose counters or verification.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before range cookie support'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before range cookie support'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA post_secrets TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION post_secrets.resume_anonymous_refusal(
 p_token bytea,p_network bytea,p_address bytea,p_environment bytea,p_minted boolean,p_now bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE s post_secrets.anonymous_sessions%ROWTYPE; v_limit integer; v_last bigint;
 v_clock bigint:=floor(extract(epoch FROM clock_timestamp()))::bigint;
BEGIN
 IF p_token IS NULL OR octet_length(p_token)<>32
    OR p_network IS NULL OR octet_length(p_network)<>32
    OR p_address IS NULL OR octet_length(p_address)<>32
    OR p_environment IS NULL OR octet_length(p_environment)<>32
    OR p_minted IS NULL OR p_now IS NULL OR p_now<=0 OR p_now NOT BETWEEN v_clock-30 AND v_clock+30 THEN
  RAISE EXCEPTION 'Invalid anonymous refusal context.' USING ERRCODE='23514'; END IF;
 SELECT a.* INTO s FROM post_secrets.anonymous_sessions a WHERE a.token_hash=p_token FOR UPDATE;
 IF NOT FOUND THEN
  IF NOT p_minted THEN
   RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000'; END IF;
  SELECT session_limit INTO v_limit FROM post_secrets.anonymous_policy WHERE singleton FOR UPDATE;
  IF NOT FOUND THEN
   RAISE EXCEPTION 'Anonymous session policy is unavailable.' USING ERRCODE='55000'; END IF;
  v_clock:=floor(extract(epoch FROM clock_timestamp()))::bigint;
  IF p_now NOT BETWEEN v_clock-30 AND v_clock+30 THEN
   RAISE EXCEPTION 'Anonymous refusal context expired.' USING ERRCODE='23514'; END IF;
  DELETE FROM post_secrets.anonymous_sessions a WHERE a.token_hash IN (
   SELECT old.token_hash FROM post_secrets.anonymous_sessions old
   WHERE old.expires_at<=v_clock ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED
  );
  IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.anonymous_sessions LIMIT v_limit) bounded)>=v_limit THEN
   RAISE EXCEPTION 'Anonymous session capacity is exhausted.' USING ERRCODE='53300'; END IF;
  INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
   created_at,network_at,address_at,environment_at,expires_at)
  VALUES(p_token,p_network,p_address,p_environment,p_now,p_now,p_now,p_now,p_now+31536000);
  RETURN;
 END IF;
 v_clock:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF p_minted OR s.expires_at<=v_clock THEN
  RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000'; END IF;
 IF p_now NOT BETWEEN v_clock-30 AND v_clock+30 THEN
  RAISE EXCEPTION 'Anonymous refusal context expired.' USING ERRCODE='23514'; END IF;
 v_last:=CASE WHEN s.activity_at>0 THEN s.activity_at ELSE s.created_at END;
 IF greatest(0,p_now-v_last)>=604800 THEN
  s.created_at:=p_now; s.network_at:=p_now; s.address_at:=p_now;
  s.environment_at:=p_now; s.action_at:=p_now; s.activity_at:=0;
  s.verified_level:=0; s.posts:=0; s.images:=0; s.threads:=0;
  s.reports:=0; s.pending:=0; s.change_score:=0;
 ELSE
  IF s.environment_hash<>p_environment THEN s.environment_at:=p_now; END IF;
  IF s.network_hash<>p_network THEN
   s.network_at:=p_now; s.address_at:=p_now;
  ELSIF s.address_hash<>p_address THEN s.address_at:=p_now;
  END IF;
 END IF;
 UPDATE post_secrets.anonymous_sessions a SET
  network_hash=p_network,address_hash=p_address,environment_hash=p_environment,
  created_at=s.created_at,network_at=s.network_at,address_at=s.address_at,
  environment_at=s.environment_at,activity_at=s.activity_at,action_at=s.action_at,
  expires_at=p_now+31536000,verified_level=s.verified_level,posts=s.posts,
  images=s.images,threads=s.threads,reports=s.reports,pending=s.pending,change_score=s.change_score
 WHERE a.token_hash=p_token;
END $$;
REVOKE ALL ON FUNCTION post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint) TO board_restriction_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA content,restrictions TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE FUNCTION content.refresh_automatic_range_cookie(
 p_id bigint,p_board text,p_parent bigint,p_prefix bytea,p_browser text,
 p_token bytea,p_network bytea,p_address bytea,p_environment bytea,p_minted boolean,p_now bigint
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now bigint; v_invoker text:=restrictions.invoker();
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Automatic range refresh requires read committed isolation.' USING ERRCODE='P0186'; END IF;
 IF v_invoker NOT IN('board_public','board_staff') THEN
  RAISE EXCEPTION 'Automatic range cookie refresh is unavailable.' USING ERRCODE='42501'; END IF;
 IF p_id IS NULL OR p_id<1 OR p_board IS NULL OR p_parent IS NULL OR p_parent<0
    OR p_prefix IS NULL OR octet_length(p_prefix)<>32 OR p_browser IS NULL OR p_browser !~ '^1[0-9a-f]{8}$' THEN
  RAISE EXCEPTION 'Invalid automatic range cookie context.' USING ERRCODE='P0186'; END IF;
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Automatic range policy is unavailable.' USING ERRCODE='P0186'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=p_board AND (NOT b.staff_only OR v_invoker='board_staff')) THEN
  RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002'; END IF;
 PERFORM content.lock_automatic_range_policy(false);
 v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF NOT EXISTS(SELECT 1 FROM restrictions.automatic_ranges e WHERE e.id=p_id AND e.board=p_board
  AND e.parent=p_parent AND e.prefix=p_prefix AND e.browser=p_browser AND e.created_at>v_now-7200) THEN
  RETURN false;
 END IF;
 PERFORM post_secrets.resume_anonymous_refusal(p_token,p_network,p_address,p_environment,p_minted,p_now);
 RETURN true;
END $$;
REVOKE ALL ON FUNCTION content.refresh_automatic_range_cookie(bigint,text,bigint,bytea,text,bytea,bytea,bytea,bytea,boolean,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.refresh_automatic_range_cookie(bigint,text,bigint,bytea,text,bytea,bytea,bytea,bytea,boolean,bigint) TO board_public,board_staff;
CREATE OR REPLACE FUNCTION restrictions.catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='restrictions' OR c.oid='post_secrets.posting_history'::regclass
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
      has_function_privilege('board_restriction_owner',p.oid,'EXECUTE')) ORDER BY p.oid::regprocedure::text)
   FROM pg_proc p WHERE p.oid IN('staff_identity.has_board_access(bigint,text)'::regprocedure,
      'content.lock_posting_actor(bytea,boolean)'::regprocedure,
      'post_secrets.lookup_automatic_identity(bytea,bigint)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
      'content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),restrictions.acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
   FROM pg_namespace n WHERE n.nspname='restrictions'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,restrictions FROM board_restriction_owner;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Range cookie support changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Range cookie support changed restriction authority'; END IF; END $$;
RESET ROLE;
