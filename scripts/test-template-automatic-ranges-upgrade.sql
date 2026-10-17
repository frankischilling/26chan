SET ROLE board_restriction_owner;
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Template range issuer is not ready'; END IF;
 IF (SELECT templates FROM public.owned_template_ranges_before) IS DISTINCT FROM
    (SELECT jsonb_agg(to_jsonb(t)-'source_rule' ORDER BY id) FROM restrictions.templates t)
 OR EXISTS(SELECT 1 FROM restrictions.templates WHERE source_rule IS NOT NULL)
 OR (SELECT events FROM public.owned_template_ranges_before) IS DISTINCT FROM
    (SELECT jsonb_agg(to_jsonb(e) ORDER BY id) FROM restrictions.automatic_ranges e) THEN
  RAISE EXCEPTION 'Template range upgrade inferred or changed historical evidence'; END IF;
 BEGIN
  UPDATE restrictions.templates SET source_rule=repeat('x',256) WHERE id=1900000101;
  RAISE EXCEPTION 'Unbounded template rule accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
RESET ROLE;
DO $$ DECLARE runtime text; BEGIN
 FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
  IF has_table_privilege(runtime,'restrictions.templates','SELECT,INSERT,UPDATE,DELETE')
    OR has_column_privilege(runtime,'restrictions.templates','source_rule','SELECT,INSERT,UPDATE')
    OR has_function_privilege(runtime,'restrictions.record_automatic_range(text,bigint,bigint)','EXECUTE')
    OR has_function_privilege(runtime,'content.issue_post_template_restriction(bytea,bytea,integer,text,bigint,bigint)','EXECUTE') IS DISTINCT FROM (runtime='board_staff') THEN
   RAISE EXCEPTION 'Template range authority changed for %',runtime; END IF;
 END LOOP;
END $$;
BEGIN;
SET LOCAL ROLE board_restriction_owner;
GRANT SELECT(source_rule) ON restrictions.templates TO board_staff;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Template rule grant drift was accepted'; END IF; END $$;
ROLLBACK;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Template range upgrade changed content readiness'; END IF; END $$;
RESET ROLE;
