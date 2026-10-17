DO $$ DECLARE before record; role_name text; BEGIN
 SELECT * INTO STRICT before FROM public.owned_capability_file_before;
 IF before.rules IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.file_rules r)
 OR before.exemptions IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(e) ORDER BY network) FROM admission.automatic_ban_exemptions e)
 OR before.warnings IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(w) ORDER BY actor_hash) FROM admission.dmca_warnings w)
 OR before.policy IS DISTINCT FROM (SELECT to_jsonb(p) FROM admission.policy p)
 OR before.capacity IS DISTINCT FROM (SELECT to_jsonb(c) FROM admission.file_effect_capacity c)
 THEN RAISE EXCEPTION 'Capability file migration changed predecessor admission state'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='content.finalize_file_filter(bytea,inet,bytea,text,bigint,text,text,bigint,bigint,bytea,jsonb,bytea,bytea,integer,jsonb)'::regprocedure
  AND p.proowner='board_restriction_owner'::regrole AND p.prosecdef
  AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']) THEN
  RAISE EXCEPTION 'Invalid file finalizer owner/path'; END IF;
 FOR role_name IN SELECT rolname FROM pg_roles WHERE rolname LIKE 'board_%' AND rolname<>'board_migrator' LOOP
  IF has_function_privilege(role_name,'content.finalize_file_filter(bytea,inet,bytea,text,bigint,text,text,bigint,bigint,bytea,jsonb,bytea,bytea,integer,jsonb)','EXECUTE')
    IS DISTINCT FROM (role_name IN('board_restriction_owner','board_public','board_staff')) THEN
   RAISE EXCEPTION 'Unexpected finalizer access for %',role_name; END IF;
  IF has_function_privilege(role_name,'content.file_effect_upload_metadata(text,text,text,bigint,boolean)','EXECUTE')
    IS DISTINCT FROM (role_name IN('board_restriction_owner','board_attachment_owner')) THEN
   RAISE EXCEPTION 'Unexpected file metadata access for %',role_name; END IF;
 END LOOP;
END $$;
SET ROLE board_restriction_owner;
DO $$ DECLARE before record; role_name text; BEGIN
 SELECT * INTO STRICT before FROM public.owned_capability_restrictions_before;
 IF before.bans IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM restrictions.bans b)
 OR before.audit IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM restrictions.audit a)
 OR before.capacity IS DISTINCT FROM (SELECT to_jsonb(c) FROM restrictions.capacity c)
 THEN RAISE EXCEPTION 'Capability file migration changed predecessor restrictions'; END IF;
 IF (SELECT count(*) FROM restrictions.file_clock)<>1
 OR NOT EXISTS(SELECT 1 FROM restrictions.file_clock WHERE singleton AND timezone IS NULL) THEN
  RAISE EXCEPTION 'Migration invented a file-ban deployment timezone'; END IF;
 FOR role_name IN SELECT rolname FROM pg_roles WHERE rolname LIKE 'board_%' AND rolname NOT IN('board_migrator','board_restriction_owner') LOOP
  IF has_any_column_privilege(role_name,'restrictions.file_clock','SELECT,INSERT,UPDATE,REFERENCES')
   OR has_table_privilege(role_name,'restrictions.file_clock','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') THEN
   RAISE EXCEPTION 'Runtime acquired file timezone authority'; END IF;
 END LOOP;
 IF content.restrictions_ready() IS DISTINCT FROM true OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Capability file upgrade changed readiness'; END IF;
END $$;
RESET ROLE;
-- Every mutation stays inside its own subtransaction and must invalidate the
-- runtime seal. The deliberate exception restores the original dependency.
DO $$ DECLARE mutation text; ready boolean; BEGIN
 FOREACH mutation IN ARRAY ARRAY[
  'SET LOCAL ROLE board_admission_owner; REVOKE EXECUTE ON FUNCTION admission.file_effect_dmca(bytea) FROM board_restriction_owner; RESET ROLE',
  'SET LOCAL ROLE board_admission_owner; GRANT EXECUTE ON FUNCTION admission.resolve_file_effect_rule(text,bigint,bytea,bytea,boolean) TO board_public; RESET ROLE',
  'SET LOCAL ROLE board_admission_owner; ALTER FUNCTION admission.file_effect_exempt(inet) SECURITY INVOKER; RESET ROLE',
  'SET LOCAL ROLE board_attachment_owner; ALTER FUNCTION content.file_effect_upload_metadata(text,text,text,bigint,boolean) SET search_path=pg_catalog,public,pg_temp; RESET ROLE',
  'SET LOCAL ROLE board_staff_post_owner; REVOKE EXECUTE ON FUNCTION content.revalidate_staff_refusal(bytea,bytea,integer,text) FROM board_restriction_owner; RESET ROLE',
  'GRANT SELECT ON admission.file_rules TO board_public'
 ] LOOP
  BEGIN
   EXECUTE mutation;
   EXECUTE 'SET LOCAL ROLE board_restriction_owner';
   SELECT content.restrictions_ready() INTO ready;
   EXECUTE 'RESET ROLE';
   IF ready IS DISTINCT FROM false THEN RAISE EXCEPTION 'Restriction seal accepted dependency drift: %',mutation; END IF;
   RAISE SQLSTATE 'ZX192' USING MESSAGE='Restore owned dependency fixture';
  EXCEPTION WHEN SQLSTATE 'ZX192' THEN NULL;
  END;
 END LOOP;
END $$;
SET ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Dependency fixture was not restored'; END IF; END $$;
RESET ROLE;
