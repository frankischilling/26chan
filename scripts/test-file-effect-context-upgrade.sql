DO $$ DECLARE before record; signature text; role_name text; BEGIN
 SELECT * INTO STRICT before FROM public.owned_file_effect_context_before;
 IF before.rules IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.file_rules r)
 OR before.exemptions IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(e) ORDER BY network) FROM admission.automatic_ban_exemptions e)
 OR before.warnings IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(w) ORDER BY actor_hash) FROM admission.dmca_warnings w)
 OR before.policy IS DISTINCT FROM (SELECT to_jsonb(p) FROM admission.policy p)
 OR before.capacity IS DISTINCT FROM (SELECT to_jsonb(c) FROM admission.file_effect_capacity c)
 THEN RAISE EXCEPTION 'File effect context migration changed predecessor state'; END IF;
 FOREACH signature IN ARRAY ARRAY['admission.resolve_file_effect_rule(text,bigint,bytea,bytea,boolean)',
   'admission.file_effect_exempt(inet)','admission.file_effect_dmca(bytea)'] LOOP
  IF NOT EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid=signature::regprocedure
   AND p.proowner='board_admission_owner'::regrole AND p.prosecdef
   AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']) THEN
   RAISE EXCEPTION 'Invalid private file effect context owner/path'; END IF;
  FOR role_name IN SELECT rolname FROM pg_roles WHERE rolname LIKE 'board_%' AND rolname<>'board_migrator' LOOP
   IF has_function_privilege(role_name,signature,'EXECUTE') IS DISTINCT FROM (role_name IN('board_admission_owner','board_restriction_owner')) THEN
    RAISE EXCEPTION 'Unexpected private file effect context access for %',role_name; END IF;
  END LOOP;
 END LOOP;
 FOREACH signature IN ARRAY ARRAY['admission.file_rules','admission.automatic_ban_exemptions','admission.dmca_warnings','admission.file_effect_capacity'] LOOP
  IF has_any_column_privilege('board_restriction_owner',signature,'SELECT,INSERT,UPDATE,REFERENCES')
   OR has_table_privilege('board_restriction_owner',signature,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') THEN
   RAISE EXCEPTION 'Private context bypassed admission table boundary'; END IF;
 END LOOP;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'File context upgrade changed transfer readiness'; END IF; END $$;
RESET ROLE;
SET ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'File context upgrade changed restriction readiness'; END IF; END $$;
RESET ROLE;
