DO $$ DECLARE before record; role_name text; allowed boolean; BEGIN
 SELECT * INTO STRICT before FROM public.owned_staff_refusal_before;
 IF before.accounts IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM staff_identity.accounts a)
 OR before.credentials IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(c) ORDER BY id) FROM staff_identity.credentials c)
 OR before.sessions IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(s) ORDER BY token_hash) FROM staff_identity.sessions s)
 OR before.intents IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(i)) FROM post_secrets.staff_post_intents i)
 THEN RAISE EXCEPTION 'Staff refusal migration changed predecessor identity state'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner
  WHERE p.oid='content.revalidate_staff_refusal(bytea,bytea,integer,text)'::regprocedure
   AND r.rolname='board_staff_post_owner' AND p.prosecdef AND NOT p.proretset
   AND p.prorettype='void'::regtype AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'])
 THEN RAISE EXCEPTION 'Unexpected staff refusal function authority'; END IF;
 FOR role_name IN SELECT rolname FROM pg_roles WHERE rolname LIKE 'board_%' AND rolname<>'board_migrator' LOOP
  allowed:=role_name IN('board_staff','board_staff_post_owner');
  IF has_function_privilege(role_name,'content.revalidate_staff_refusal(bytea,bytea,integer,text)','EXECUTE') IS DISTINCT FROM allowed
  THEN RAISE EXCEPTION 'Unexpected staff refusal access for %',role_name; END IF;
 END LOOP;
 IF has_any_column_privilege('board_staff','staff_identity.credentials','SELECT,INSERT,UPDATE,REFERENCES')
 OR has_table_privilege('board_staff','staff_identity.credentials','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
 THEN RAISE EXCEPTION 'Staff runtime received credential table authority'; END IF;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Staff refusal migration changed transfer readiness'; END IF; END $$;
RESET ROLE;
