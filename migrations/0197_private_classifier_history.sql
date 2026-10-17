-- Bounded lookup state for two late-posting classifiers. These private helpers
-- grant no runtime posting, bypass, event-writing or history-reading authority.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before classifier history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before classifier history'; END IF;
END $$;
RESET ROLE;

CREATE TABLE post_secrets.classifier_history (
 token_hash bytea NOT NULL REFERENCES post_secrets.anonymous_sessions(token_hash) ON DELETE CASCADE,
 kind text NOT NULL CHECK(kind IN('block_bm_bot','country_changed')),
 created_at bigint NOT NULL CHECK(created_at>0),
 PRIMARY KEY(token_hash,kind)
);
-- At most two entries per capped anonymous session; no free-form event payload.
CREATE TABLE post_secrets.classifier_history_catalog (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 definition_hash bytea NOT NULL CHECK(octet_length(definition_hash)=32)
);
REVOKE ALL ON post_secrets.classifier_history,post_secrets.classifier_history_catalog FROM PUBLIC;
-- Do not bless access inherited from operator-specific default privileges.
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT DISTINCT c.oid::regclass AS relation,a.grantee
  FROM pg_class c CROSS JOIN LATERAL aclexplode(coalesce(c.relacl,acldefault('r',c.relowner))) a
  WHERE c.oid IN('post_secrets.classifier_history'::regclass,'post_secrets.classifier_history_catalog'::regclass)
   AND a.grantee NOT IN(0,c.relowner)
 LOOP EXECUTE format('REVOKE ALL ON TABLE %s FROM %I',r.relation,pg_get_userbyid(r.grantee)); END LOOP;
END $$;
GRANT SELECT,INSERT ON post_secrets.classifier_history TO board_anonymous_owner;
GRANT UPDATE(created_at) ON post_secrets.classifier_history TO board_anonymous_owner;
GRANT SELECT ON post_secrets.classifier_history_catalog TO board_anonymous_owner;
GRANT CREATE ON SCHEMA post_secrets TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;

CREATE FUNCTION post_secrets.classifier_history_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
 SET search_path=pg_catalog,pg_temp AS $$
 WITH relations AS (
  SELECT c.* FROM pg_class c WHERE c.oid IN(
   'post_secrets.classifier_history'::regclass,'post_secrets.classifier_history_catalog'::regclass)
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE n.nspname='post_secrets' AND p.proname IN('classifier_history_catalog_hash',
   'classifier_history_ready','read_classifier_history','record_classifier_history')
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(c.relname,pg_get_userbyid(c.relowner),c.relkind,
   c.relpersistence,c.relrowsecurity,c.relforcerowsecurity,c.reloptions,
   coalesce(c.relacl,acldefault('r',c.relowner))::text,
   (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),
    a.attnotnull,a.attidentity,pg_get_expr(d.adbin,d.adrelid),a.attacl::text) ORDER BY a.attnum)
    FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
    WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),
   (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated)
    ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=c.oid),
   (SELECT jsonb_agg(jsonb_build_array(pg_get_indexdef(i.indexrelid),i.indisvalid,i.indisready)
    ORDER BY i.indexrelid::regclass::text) FROM pg_index i WHERE i.indrelid=c.oid),
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
  (SELECT jsonb_build_array(nspname,pg_get_userbyid(nspowner),coalesce(nspacl,acldefault('n',nspowner))::text)
   FROM pg_namespace WHERE nspname='post_secrets')
 )::text,'UTF8'))
$$;
CREATE FUNCTION post_secrets.classifier_history_ready() RETURNS boolean LANGUAGE sql STABLE
 SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT definition_hash=post_secrets.classifier_history_catalog_hash()
 FROM post_secrets.classifier_history_catalog WHERE singleton),false)
$$;
CREATE FUNCTION post_secrets.read_classifier_history(p_token bytea,p_minted boolean)
 RETURNS TABLE(edge_blocked boolean,country_blocked boolean)
 LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_expiry bigint; v_found boolean; v_now bigint;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Classifier history requires read committed isolation.' USING ERRCODE='P197U'; END IF;
 IF p_token IS NULL OR octet_length(p_token)<>32 OR p_minted IS NULL THEN
  RAISE EXCEPTION 'Invalid classifier history context.' USING ERRCODE='23514'; END IF;
 LOCK TABLE post_secrets.classifier_history IN ROW EXCLUSIVE MODE;
 IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Classifier history is unavailable.' USING ERRCODE='P197U'; END IF;
 SELECT expires_at INTO v_expiry FROM post_secrets.anonymous_sessions WHERE token_hash=p_token FOR UPDATE;
 v_found:=FOUND;
 v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF (v_found AND (p_minted OR v_expiry<=v_now)) OR (NOT v_found AND NOT p_minted) THEN
  RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000'; END IF;
 edge_blocked:=false; country_blocked:=false;
 IF v_found THEN
  SELECT EXISTS(SELECT 1 FROM post_secrets.classifier_history WHERE token_hash=p_token
   AND kind='block_bm_bot' AND created_at>v_now-86400),
   EXISTS(SELECT 1 FROM post_secrets.classifier_history WHERE token_hash=p_token
   AND kind='country_changed' AND created_at>v_now-86400)
  INTO edge_blocked,country_blocked;
 END IF;
 RETURN NEXT;
END $$;
CREATE FUNCTION post_secrets.record_classifier_history(p_token bytea,p_kind text)
 RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now bigint; v_active boolean; v_rows integer;
BEGIN
 IF p_kind IS NULL OR p_kind NOT IN('block_bm_bot','country_changed') THEN
  RAISE EXCEPTION 'Invalid classifier event kind.' USING ERRCODE='23514'; END IF;
 -- The future issuer must first resume/create the session with authenticated
 -- fingerprints. This helper cannot create sessions or choose activity state.
 SELECT CASE WHEN p_kind='block_bm_bot' THEN edge_blocked ELSE country_blocked END
 INTO STRICT v_active FROM post_secrets.read_classifier_history(p_token,false);
 IF v_active THEN RETURN false; END IF;
 v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 INSERT INTO post_secrets.classifier_history(token_hash,kind,created_at) VALUES(p_token,p_kind,v_now)
 ON CONFLICT(token_hash,kind) DO UPDATE SET created_at=EXCLUDED.created_at;
 GET DIAGNOSTICS v_rows=ROW_COUNT;
 RETURN v_rows=1;
END $$;
REVOKE ALL ON FUNCTION post_secrets.classifier_history_catalog_hash(),
 post_secrets.classifier_history_ready(),post_secrets.read_classifier_history(bytea,boolean),
 post_secrets.record_classifier_history(bytea,text) FROM PUBLIC;
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT DISTINCT p.oid::regprocedure AS identity,a.grantee
  FROM pg_proc p CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
  WHERE p.oid IN('post_secrets.classifier_history_catalog_hash()'::regprocedure,
   'post_secrets.classifier_history_ready()'::regprocedure,
   'post_secrets.read_classifier_history(bytea,boolean)'::regprocedure,
   'post_secrets.record_classifier_history(bytea,text)'::regprocedure)
   AND a.grantee NOT IN(0,p.proowner)
 LOOP EXECUTE format('REVOKE ALL ON FUNCTION %s FROM %I',r.identity,pg_get_userbyid(r.grantee)); END LOOP;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_classifier_history_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
INSERT INTO post_secrets.classifier_history_catalog
 VALUES(true,decode(current_setting('board.migration_classifier_history_hash'),'hex'));
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Classifier history catalog is unavailable'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Classifier history changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Classifier history changed restriction authority'; END IF; END $$;
RESET ROLE;
