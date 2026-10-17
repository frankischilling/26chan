SET ROLE board_restriction_owner;
DO $$
DECLARE v_before public.owned_private_writer_before%ROWTYPE; v_role text; f oid;
BEGIN
 SELECT * INTO STRICT v_before FROM public.owned_private_writer_before;
 IF v_before.bans IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM restrictions.bans b)
 OR v_before.audit IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM restrictions.audit a)
 OR v_before.capacity IS DISTINCT FROM (SELECT to_jsonb(c) FROM restrictions.capacity c)
 THEN RAISE EXCEPTION 'Private file writer changed existing restriction data'; END IF;
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Private file writer catalog is not ready'; END IF;
 FOREACH f IN ARRAY ARRAY['restrictions.file_expiry(timestamptz,integer,text)'::regprocedure::oid,
  'restrictions.record_file_ban(bytea,bytea,text,bigint,bytea,integer,boolean,text,text,text)'::regprocedure::oid] LOOP
  IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=f AND proowner='board_restriction_owner'::regrole
     AND NOT prosecdef AND NOT proretset AND cardinality(proconfig)=2
     AND EXISTS(SELECT 1 FROM unnest(proconfig) c WHERE replace(c,' ','')='search_path=pg_catalog,pg_temp')
     AND EXISTS(SELECT 1 FROM unnest(proconfig) c WHERE lower(c)='timezone=utc')) THEN
   RAISE EXCEPTION 'Private file writer properties differ'; END IF;
  FOR v_role IN SELECT rolname FROM pg_roles WHERE rolname LIKE 'board_%'
    AND rolname NOT IN('board_migrator','board_restriction_owner') LOOP
   IF has_function_privilege(v_role,f,'EXECUTE') THEN RAISE EXCEPTION 'Private file writer execution broadened'; END IF;
  END LOOP;
 END LOOP;
END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Private file writer changed transfer readiness'; END IF; END $$;
RESET ROLE;
REVOKE USAGE ON SCHEMA public FROM board_restriction_owner;
