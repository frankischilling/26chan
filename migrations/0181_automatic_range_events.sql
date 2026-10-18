-- Derivative mobile range events are private and independent of target lifetime.
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before automatic events'; END IF;
END $$;
RESET ROLE;

GRANT CREATE ON SCHEMA content TO board_admission_owner;
SET LOCAL ROLE board_admission_owner;
CREATE FUNCTION content.lock_automatic_range_policy(p_write boolean) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF p_write IS NULL THEN RAISE EXCEPTION 'Invalid automatic policy lock.' USING ERRCODE='P0181'; END IF;
 IF p_write THEN
  PERFORM singleton FROM admission.policy WHERE singleton FOR UPDATE;
 ELSE
  PERFORM singleton FROM admission.policy WHERE singleton FOR SHARE;
 END IF;
 IF NOT FOUND THEN RAISE EXCEPTION 'Automatic range policy is unavailable.' USING ERRCODE='P0181'; END IF;
END $$;
REVOKE ALL ON FUNCTION content.lock_automatic_range_policy(boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.lock_automatic_range_policy(boolean) TO board_restriction_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_admission_owner;

GRANT CREATE ON SCHEMA restrictions,content TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE TABLE restrictions.automatic_ranges (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY CHECK(id>0),
 board text NOT NULL CHECK(board ~ '^[a-z0-9]{1,10}$'),
 parent bigint NOT NULL CHECK(parent>=0),
 prefix bytea NOT NULL CHECK(octet_length(prefix)=32),
 browser text NOT NULL CHECK(browser ~ '^1[0-9a-f]{8}$'),
 created_at bigint NOT NULL CHECK(created_at>=0),
 source_ban_id bigint NOT NULL CHECK(source_ban_id>0),
 hit_count bigint NOT NULL DEFAULT 0 CHECK(hit_count>=0),
 last_hit bigint CHECK(last_hit>=0)
);
CREATE INDEX automatic_ranges_match ON restrictions.automatic_ranges(board,parent,prefix,browser,created_at);
CREATE INDEX automatic_ranges_expiry ON restrictions.automatic_ranges(created_at,id);
REVOKE ALL ON restrictions.automatic_ranges,restrictions.automatic_ranges_id_seq FROM PUBLIC;

-- Private groundwork for a future template-qualified issuer. The current
-- custom-ban API does not imply the source global-template predicate and must
-- not call this helper. A qualified caller must own actor -> restriction gate
-- -> board/thread/post locks before invoking it.
CREATE FUNCTION restrictions.record_automatic_range(p_board text,p_post bigint,p_ban bigint) RETURNS void
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_browser text; v_prefix bytea; v_parent bigint; v_now bigint;
BEGIN
 SELECT h.automatic_browser,h.automatic_prefix,CASE WHEN h.post_id=p.thread_id THEN 0 ELSE p.thread_id END
 INTO v_browser,v_prefix,v_parent FROM post_secrets.posting_history h
 JOIN content.posts p ON p.id=h.post_id AND p.board=h.board
 WHERE h.board=p_board AND h.post_id=p_post;
 IF NOT FOUND OR v_browser IS NULL OR v_prefix IS NULL THEN RETURN; END IF;
 PERFORM content.lock_automatic_range_policy(true);
 v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 -- Bounded cleanup. Capacity exhaustion refuses the whole issuance instead
 -- of evicting an active event or committing a ban without its derivative.
 DELETE FROM restrictions.automatic_ranges WHERE id IN (
  SELECT id FROM restrictions.automatic_ranges WHERE created_at<=v_now-7200
  ORDER BY created_at,id LIMIT 1024);
 IF EXISTS(SELECT 1 FROM restrictions.automatic_ranges e WHERE e.board=p_board AND e.parent=v_parent
    AND e.prefix=v_prefix AND e.browser=v_browser AND e.created_at>v_now-3600) THEN RETURN; END IF;
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.automatic_ranges LIMIT 100000) bounded)>=100000 THEN
  RAISE EXCEPTION 'Automatic range capacity is exhausted.' USING ERRCODE='P0181'; END IF;
 INSERT INTO restrictions.automatic_ranges(board,parent,prefix,browser,created_at,source_ban_id)
 VALUES(p_board,v_parent,v_prefix,v_browser,v_now,p_ban);
END $$;
REVOKE ALL ON FUNCTION restrictions.record_automatic_range(text,bigint,bigint) FROM PUBLIC;

CREATE FUNCTION content.automatic_range_events(p_board text,p_parent bigint,p_prefix bytea,p_browser text)
RETURNS TABLE(id bigint,board text,parent bigint,prefix bytea,browser text,created_at bigint,observed_at bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now bigint; v_invoker text;
BEGIN
 v_invoker:=restrictions.invoker();
 IF v_invoker NOT IN('board_public','board_staff') THEN
  RAISE EXCEPTION 'Automatic range observation is unavailable.' USING ERRCODE='42501'; END IF;
 IF p_board IS NULL OR p_parent IS NULL OR p_parent<0 OR p_prefix IS NULL OR octet_length(p_prefix)<>32
    OR (p_browser IS NOT NULL AND p_browser !~ '^1[0-9a-f]{8}$') THEN
  RAISE EXCEPTION 'Invalid automatic range observation.' USING ERRCODE='P0181'; END IF;
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Automatic range policy is unavailable.' USING ERRCODE='P0181'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=p_board AND (NOT b.staff_only OR v_invoker='board_staff')) THEN
  RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002'; END IF;
 PERFORM content.lock_automatic_range_policy(false);
 v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 RETURN QUERY SELECT e.id,e.board,e.parent,e.prefix,e.browser,e.created_at,v_now
 FROM restrictions.automatic_ranges e WHERE e.board=p_board AND e.parent=p_parent AND e.prefix=p_prefix
 AND (p_browser IS NULL OR e.browser=p_browser) AND e.created_at>v_now-7200 ORDER BY e.id LIMIT 129;
END $$;
REVOKE ALL ON FUNCTION content.automatic_range_events(text,bigint,bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.automatic_range_events(text,bigint,bytea,text) TO board_public,board_staff;

-- Run after a rejected posting transaction has rolled back. The only mutable
-- fields are bounded per-event hit counters; no post/activity data is written.
CREATE FUNCTION content.record_automatic_range_hit(p_id bigint,p_board text,p_parent bigint,p_prefix bytea,p_browser text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now bigint; v_invoker text;
BEGIN
 v_invoker:=restrictions.invoker();
 IF v_invoker NOT IN('board_public','board_staff') THEN
  RAISE EXCEPTION 'Automatic range hit recording is unavailable.' USING ERRCODE='42501'; END IF;
 IF p_id IS NULL OR p_id<1 OR p_board IS NULL OR p_parent IS NULL OR p_parent<0
    OR p_prefix IS NULL OR octet_length(p_prefix)<>32 OR p_browser IS NULL OR p_browser !~ '^1[0-9a-f]{8}$' THEN
  RAISE EXCEPTION 'Invalid automatic range hit.' USING ERRCODE='P0181'; END IF;
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Automatic range policy is unavailable.' USING ERRCODE='P0181'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=p_board AND (NOT b.staff_only OR v_invoker='board_staff')) THEN
  RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002'; END IF;
 PERFORM content.lock_automatic_range_policy(false);
 v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 UPDATE restrictions.automatic_ranges e SET hit_count=CASE WHEN e.hit_count=9223372036854775807 THEN e.hit_count ELSE e.hit_count+1 END,last_hit=v_now
 WHERE e.id=p_id AND e.board=p_board AND e.parent=p_parent AND e.prefix=p_prefix AND e.browser=p_browser
   AND e.created_at>v_now-7200;
END $$;
REVOKE ALL ON FUNCTION content.record_automatic_range_hit(bigint,text,bigint,bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.record_automatic_range_hit(bigint,text,bigint,bytea,text) TO board_public,board_staff;

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
REVOKE CREATE ON SCHEMA restrictions,content FROM board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Automatic events did not preserve restriction authority'; END IF; END $$;
RESET ROLE;
