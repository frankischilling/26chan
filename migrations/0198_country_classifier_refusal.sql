-- Private country-change refusal state, exposed only through a scoped runtime
-- bridge. Solved challenges and selected badges are not bypass authority.
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Classifier history drifted before country refusal bridge'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer authority drifted around country refusal bridge'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction authority drifted around country refusal bridge'; END IF; END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA content,post_secrets TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION content.posting_classifier_history(p_board text,p_token bytea,p_minted boolean)
 RETURNS TABLE(edge_blocked boolean,country_blocked boolean)
 LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
BEGIN
 IF v_invoker<>'board_public' THEN RAISE EXCEPTION 'Classifier history authority is unavailable.' USING ERRCODE='42501'; END IF;
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR KEY SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 RETURN QUERY SELECT * FROM post_secrets.read_classifier_history(p_token,p_minted);
END $$;
CREATE FUNCTION content.record_country_classifier_refusal(
 p_board text,p_token bytea,p_network bytea,p_address bytea,p_environment bytea,p_minted boolean,p_now bigint
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 v_enabled boolean; v_history boolean; v_posts integer; s post_secrets.anonymous_sessions%ROWTYPE;
BEGIN
 IF v_invoker<>'board_public' THEN RAISE EXCEPTION 'Country refusal authority is unavailable.' USING ERRCODE='42501'; END IF;
 IF current_setting('transaction_isolation')<>'read committed'
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Country refusal history is unavailable.' USING ERRCODE='P198U'; END IF;
 SELECT b.captcha_enabled INTO v_enabled FROM content.boards b
 WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 IF NOT v_enabled THEN RAISE EXCEPTION 'Country refusal is not applicable.' USING ERRCODE='28000'; END IF;
 -- Reenter the same session lock before reading history or changing state.
 -- The issuer accepts only server-derived capability fingerprints and clock.
 SELECT country_blocked INTO STRICT v_history FROM post_secrets.read_classifier_history(p_token,p_minted);
 IF p_minted THEN
  -- Fresh state has no earlier environment or retained history to refuse.
  RAISE EXCEPTION 'Country refusal requires an established session.' USING ERRCODE='28000'; END IF;
 SELECT a.* INTO s FROM post_secrets.anonymous_sessions a WHERE a.token_hash=p_token FOR UPDATE;
 IF FOUND AND p_now<greatest(s.created_at,s.network_at,s.address_at,s.environment_at,s.activity_at,s.action_at) THEN
  -- Requests may acquire the lock in a different order from their start times.
  -- Never overwrite a newer committed session with an older request snapshot.
  RAISE EXCEPTION 'Country refusal session advanced.' USING ERRCODE='28000'; END IF;
 PERFORM post_secrets.resume_anonymous_refusal(p_token,p_network,p_address,p_environment,p_minted,p_now);
 SELECT a.* INTO STRICT s FROM post_secrets.anonymous_sessions a WHERE a.token_hash=p_token;
 v_posts:=s.posts::integer+CASE WHEN (s.pending & 1)<>0 THEN 1 ELSE 0 END;
 IF s.verified_level<>0 OR v_posts>=20
 OR NOT (v_history OR (s.created_at<>p_now AND s.environment_at=p_now)) THEN
  -- Raising also rolls back the provisional session refresh.
  RAISE EXCEPTION 'Country refusal authorization changed.' USING ERRCODE='28000'; END IF;
 IF NOT v_history THEN PERFORM post_secrets.record_classifier_history(p_token,'country_changed'); END IF;
 RETURN true;
END $$;
REVOKE ALL ON FUNCTION content.posting_classifier_history(text,bytea,boolean),
 content.record_country_classifier_refusal(text,bytea,bytea,bytea,bytea,boolean,bigint) FROM PUBLIC;
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT DISTINCT p.oid::regprocedure AS identity,a.grantee
  FROM pg_proc p CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
  WHERE p.oid IN('content.posting_classifier_history(text,bytea,boolean)'::regprocedure,
   'content.record_country_classifier_refusal(text,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)
   AND a.grantee NOT IN(0,p.proowner)
 LOOP EXECUTE format('REVOKE ALL ON FUNCTION %s FROM %I',r.identity,pg_get_userbyid(r.grantee)); END LOOP;
END $$;
GRANT EXECUTE ON FUNCTION content.posting_classifier_history(text,bytea,boolean),
 content.record_country_classifier_refusal(text,bytea,bytea,bytea,bytea,boolean,bigint) TO board_public;
CREATE OR REPLACE FUNCTION post_secrets.classifier_history_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
 SET search_path=pg_catalog,pg_temp AS $$
 WITH relations AS (
  SELECT c.* FROM pg_class c WHERE c.oid IN(
   'post_secrets.classifier_history'::regclass,'post_secrets.classifier_history_catalog'::regclass,
   'post_secrets.anonymous_sessions'::regclass,'post_secrets.anonymous_policy'::regclass,'content.boards'::regclass)
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE (n.nspname='post_secrets' AND p.proname IN('classifier_history_catalog_hash',
   'classifier_history_ready','read_classifier_history','record_classifier_history'))
   OR (n.nspname='content' AND p.proname IN('posting_classifier_history','record_country_classifier_refusal'))
   OR p.oid='post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure
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
CREATE OR REPLACE FUNCTION post_secrets.classifier_history_ready() RETURNS boolean LANGUAGE sql STABLE
 SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT definition_hash=post_secrets.classifier_history_catalog_hash()
 FROM post_secrets.classifier_history_catalog WHERE singleton),false)
 AND EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_anonymous_owner'
  AND NOT(rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE member='board_anonymous_owner'::regrole)
 AND EXISTS(SELECT 1 FROM pg_auth_members WHERE roleid='board_anonymous_owner'::regrole
  AND member='board_migrator'::regrole AND NOT admin_option AND NOT inherit_option AND set_option)
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE roleid='board_anonymous_owner'::regrole
  AND (member<>'board_migrator'::regrole OR admin_option OR inherit_option OR NOT set_option))
 AND NOT EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN('board_public','board_staff','board_auth',
  'board_media','board_media_read','board_media_intake','board_monitor','board_verifier','board_mailer','board_moderation_retention')
  AND pg_has_role(r.oid,'board_anonymous_owner'::regrole,'MEMBER'))
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
 RAISE EXCEPTION 'Country refusal bridge is unavailable'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer authority drifted around country refusal bridge'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction authority drifted around country refusal bridge'; END IF; END $$;
RESET ROLE;
