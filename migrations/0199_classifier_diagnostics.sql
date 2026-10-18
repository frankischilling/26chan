-- Bounded diagnostics only. These rows never supply classifier cooldown or
-- posting authority, and contain no raw credentials, headers or post bodies.
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Classifier history drifted before diagnostic ledger'; END IF; END $$;
RESET ROLE;
CREATE TABLE post_secrets.classifier_events (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 created_at bigint NOT NULL CHECK(created_at>0),
 kind text NOT NULL CHECK(kind IN('log_mask_changed','block_txt_threat','block_aicg',
  'block_lolg','block_owg','block_bag','block_gbfg','block_adgq','block_zzz',
  'block_funkg','block_pub_prox','block_bag_scat','block_bm_bot','block_bm_bot_pwd',
  'country_changed','block_country_changed')),
 board text NOT NULL CHECK(board ~ '^[a-z0-9]{1,10}$'),
 thread_id bigint NOT NULL CHECK(thread_id>=0),
 country text CHECK(country ~ '^[A-Z]{2}$'),
 threat_score double precision CHECK(threat_score>=0 AND threat_score<'Infinity'::double precision)
);
CREATE INDEX classifier_events_expiry ON post_secrets.classifier_events(created_at,id);
REVOKE ALL ON post_secrets.classifier_events FROM PUBLIC;
REVOKE ALL ON SEQUENCE post_secrets.classifier_events_id_seq FROM PUBLIC;
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT DISTINCT c.oid::regclass AS relation,c.relkind,a.grantee
  FROM pg_class c CROSS JOIN LATERAL aclexplode(coalesce(c.relacl,acldefault(CASE WHEN c.relkind='S' THEN 'S'::"char" ELSE 'r'::"char" END,c.relowner))) a
  WHERE c.oid IN('post_secrets.classifier_events'::regclass,'post_secrets.classifier_events_id_seq'::regclass)
   AND a.grantee NOT IN(0,c.relowner)
 LOOP EXECUTE format('REVOKE ALL ON %s %s FROM %I',CASE WHEN r.relkind='S' THEN 'SEQUENCE' ELSE 'TABLE' END,r.relation,pg_get_userbyid(r.grantee)); END LOOP;
END $$;
GRANT SELECT,INSERT,DELETE ON post_secrets.classifier_events TO board_anonymous_owner;
GRANT USAGE ON SEQUENCE post_secrets.classifier_events_id_seq TO board_anonymous_owner;
GRANT CREATE ON SCHEMA content,post_secrets TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION post_secrets.prune_classifier_events() RETURNS bigint
 LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE removed bigint; cutoff bigint:=floor(extract(epoch FROM clock_timestamp()))::bigint-604800;
BEGIN
 WITH expired AS (SELECT id FROM post_secrets.classifier_events
  WHERE created_at<cutoff
  ORDER BY created_at,id LIMIT 1000), deleted AS (
  DELETE FROM post_secrets.classifier_events e USING expired x WHERE e.id=x.id RETURNING e.id)
 SELECT count(*) INTO removed FROM deleted;
 RETURN removed;
END $$;
CREATE FUNCTION content.record_posting_classifier_events(
 p_board text,p_thread bigint,p_kinds text[],p_country text,p_score double precision
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 v_enabled boolean; v_now bigint; v_tail text[];
BEGIN
 IF v_invoker<>'board_public' THEN RAISE EXCEPTION 'Classifier diagnostic authority is unavailable.' USING ERRCODE='42501'; END IF;
 IF p_thread IS NULL OR p_thread<0 OR p_kinds IS NULL OR array_ndims(p_kinds)<>1
 OR array_lower(p_kinds,1)<>1 OR cardinality(p_kinds) NOT BETWEEN 1 AND 3 OR array_position(p_kinds,NULL) IS NOT NULL
 OR NOT (p_kinds <@ ARRAY['log_mask_changed','block_txt_threat','block_aicg',
  'block_lolg','block_owg','block_bag','block_gbfg','block_adgq','block_zzz',
  'block_funkg','block_pub_prox','block_bag_scat','block_bm_bot','block_bm_bot_pwd',
  'country_changed','block_country_changed'])
 OR (p_country IS NOT NULL AND p_country !~ '^[A-Z]{2}$')
 OR (p_score IS NOT NULL AND NOT(p_score>=0 AND p_score<'Infinity'::double precision)) THEN
  RAISE EXCEPTION 'Invalid classifier diagnostic context.' USING ERRCODE='23514'; END IF;
 v_tail:=CASE WHEN p_kinds[1]='log_mask_changed' THEN p_kinds[2:cardinality(p_kinds)] ELSE p_kinds END;
 IF NOT ((cardinality(v_tail)=0 AND p_kinds=ARRAY['log_mask_changed'])
  OR (cardinality(v_tail)=1 AND v_tail[1] NOT IN('log_mask_changed','country_changed'))
  OR v_tail=ARRAY['country_changed','block_country_changed']) THEN
  RAISE EXCEPTION 'Invalid classifier diagnostic order.' USING ERRCODE='23514'; END IF;
 SELECT b.captcha_enabled INTO v_enabled FROM content.boards b
 WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
 IF NOT FOUND OR NOT v_enabled THEN RAISE EXCEPTION 'Classifier diagnostics are not applicable.' USING ERRCODE='28000'; END IF;
 LOCK TABLE post_secrets.classifier_events IN SHARE ROW EXCLUSIVE MODE;
 IF current_setting('transaction_isolation')<>'read committed'
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Classifier diagnostics are unavailable.' USING ERRCODE='P199U'; END IF;
 PERFORM post_secrets.prune_classifier_events();
 IF (SELECT count(*) FROM post_secrets.classifier_events)+cardinality(p_kinds)>100000 THEN
  -- Capacity is a logging condition, never a new admission decision.
  RETURN false; END IF;
 v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 INSERT INTO post_secrets.classifier_events(created_at,kind,board,thread_id,country,threat_score)
 SELECT v_now,k,p_board,p_thread,CASE WHEN k='country_changed' THEN p_country END,
  CASE WHEN k NOT IN('log_mask_changed','country_changed','block_country_changed','block_bm_bot_pwd') THEN p_score END
 FROM unnest(p_kinds) WITH ORDINALITY AS events(k,ordinal) ORDER BY ordinal;
 RETURN true;
END $$;
CREATE FUNCTION post_secrets.classifier_retention_ready() RETURNS boolean
 LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_moderation_retention'
  AND NOT(rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE member='board_moderation_retention'::regrole)
 AND post_secrets.classifier_history_ready()
$$;
CREATE FUNCTION content.expire_classifier_events() RETURNS bigint
 LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
BEGIN
 IF v_invoker NOT IN('board_moderation_retention','board_migrator')
 OR session_user::text<>v_invoker THEN
  RAISE EXCEPTION 'Classifier retention authority is unavailable.' USING ERRCODE='42501'; END IF;
 LOCK TABLE post_secrets.classifier_events IN SHARE ROW EXCLUSIVE MODE;
 IF current_setting('transaction_isolation')<>'read committed'
 OR post_secrets.classifier_retention_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Classifier diagnostics are unavailable.' USING ERRCODE='P199U'; END IF;
 RETURN post_secrets.prune_classifier_events();
END $$;
REVOKE ALL ON FUNCTION post_secrets.prune_classifier_events(),post_secrets.classifier_retention_ready(),
 content.record_posting_classifier_events(text,bigint,text[],text,double precision),
 content.expire_classifier_events() FROM PUBLIC;
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT DISTINCT p.oid::regprocedure AS identity,a.grantee
  FROM pg_proc p CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
  WHERE p.oid IN('post_secrets.prune_classifier_events()'::regprocedure,
   'post_secrets.classifier_retention_ready()'::regprocedure,
   'content.record_posting_classifier_events(text,bigint,text[],text,double precision)'::regprocedure,
   'content.expire_classifier_events()'::regprocedure) AND a.grantee NOT IN(0,p.proowner)
 LOOP EXECUTE format('REVOKE ALL ON FUNCTION %s FROM %I',r.identity,pg_get_userbyid(r.grantee)); END LOOP;
END $$;
GRANT EXECUTE ON FUNCTION content.record_posting_classifier_events(text,bigint,text[],text,double precision) TO board_public;
GRANT EXECUTE ON FUNCTION content.expire_classifier_events() TO board_moderation_retention,board_migrator;
CREATE OR REPLACE FUNCTION post_secrets.classifier_history_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
 SET search_path=pg_catalog,pg_temp AS $$
 WITH relations AS (
  SELECT c.* FROM pg_class c WHERE c.oid IN(
   'post_secrets.classifier_history'::regclass,'post_secrets.classifier_history_catalog'::regclass,
   'post_secrets.classifier_events'::regclass,'post_secrets.classifier_events_id_seq'::regclass,
   'post_secrets.anonymous_sessions'::regclass,'post_secrets.anonymous_policy'::regclass,'content.boards'::regclass)
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE (n.nspname='post_secrets' AND p.proname IN('classifier_history_catalog_hash',
   'classifier_history_ready','read_classifier_history','record_classifier_history','prune_classifier_events','classifier_retention_ready'))
   OR (n.nspname='content' AND p.proname IN('posting_classifier_history','record_country_classifier_refusal',
    'record_posting_classifier_events','expire_classifier_events'))
   OR p.oid='post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(c.relname,pg_get_userbyid(c.relowner),c.relkind,
   c.relpersistence,c.relrowsecurity,c.relforcerowsecurity,c.reloptions,
   (SELECT jsonb_build_array(seqstart,seqincrement,seqmax,seqmin,seqcache,seqcycle) FROM pg_sequence WHERE seqrelid=c.oid),
   coalesce(c.relacl,acldefault(CASE WHEN c.relkind='S' THEN 'S'::"char" ELSE 'r'::"char" END,c.relowner))::text,
   (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),
    a.attnotnull,a.attidentity,pg_get_expr(d.adbin,d.adrelid),a.attacl::text) ORDER BY a.attnum)
    FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
    WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),
   (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated)
    ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=c.oid),
   (SELECT jsonb_agg(jsonb_build_array(pg_get_indexdef(i.indexrelid),i.indisvalid,i.indisready)
    ORDER BY i.indexrelid::regclass::text) FROM pg_index i WHERE i.indrelid=c.oid),
   (SELECT jsonb_agg(jsonb_build_array(p.polname,p.polcmd,p.polpermissive,
    (SELECT jsonb_agg(CASE WHEN r=0 THEN 'PUBLIC' ELSE pg_get_userbyid(r)::text END
     ORDER BY CASE WHEN r=0 THEN 'PUBLIC' ELSE pg_get_userbyid(r)::text END) FROM unnest(p.polroles) r),
    pg_get_expr(p.polqual,p.polrelid),pg_get_expr(p.polwithcheck,p.polrelid)) ORDER BY p.polname)
    FROM pg_policy p WHERE p.polrelid=c.oid),
   (SELECT jsonb_agg(jsonb_build_array(t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)) ORDER BY t.tgname)
    FROM pg_trigger t WHERE t.tgrelid=c.oid AND NOT t.tgisinternal)) ORDER BY c.relname) FROM relations c),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
   coalesce(p.proacl,acldefault('f',p.proowner))::text,pg_get_functiondef(p.oid))
   ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,
   t.tgenabled,t.tgtype,t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
   ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(
    SELECT oid FROM pg_constraint WHERE conrelid='post_secrets.classifier_history'::regclass AND contype='f')),
  (SELECT jsonb_agg(jsonb_build_array(nspname,pg_get_userbyid(nspowner),
    coalesce(nspacl,acldefault('n',nspowner))::text) ORDER BY nspname)
   FROM pg_namespace WHERE nspname IN('post_secrets','content'))
 )::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_classifier_history_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog
 SET definition_hash=decode(current_setting('board.migration_classifier_history_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Classifier diagnostic ledger is unavailable'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer authority drifted around classifier diagnostics'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction authority drifted around classifier diagnostics'; END IF; END $$;
RESET ROLE;
