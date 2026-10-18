SET ROLE board_restriction_owner;
DO $$
DECLARE v_before public.owned_file_restrictions_before%ROWTYPE; v_role text;
BEGIN
 SELECT * INTO STRICT v_before FROM public.owned_file_restrictions_before;
 IF v_before.bans IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(b)-'origin'-'file_rule_id'-'file_digest' ORDER BY id) FROM restrictions.bans b)
 OR v_before.audit IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM restrictions.audit a)
 OR v_before.capacity IS DISTINCT FROM (SELECT to_jsonb(c) FROM restrictions.capacity c)
 OR EXISTS(SELECT 1 FROM restrictions.bans WHERE origin<>'staff' OR file_rule_id IS NOT NULL OR file_digest IS NOT NULL)
 THEN RAISE EXCEPTION 'File provenance changed existing restriction data'; END IF;
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'File provenance catalog is not ready'; END IF;
 FOR v_role IN SELECT rolname FROM pg_roles WHERE rolname LIKE 'board_%'
   AND rolname NOT IN('board_migrator','board_restriction_owner') LOOP
  IF has_table_privilege(v_role,'restrictions.bans','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
    OR has_any_column_privilege(v_role,'restrictions.bans','SELECT,INSERT,UPDATE,REFERENCES')
    OR has_function_privilege(v_role,'restrictions.posting_decision(bytea,bytea,text,bigint)','EXECUTE') THEN
   RAISE EXCEPTION 'File provenance exposed restriction storage'; END IF;
 END LOOP;
 BEGIN
  UPDATE restrictions.bans SET post_id=NULL WHERE post_id=188;
  RAISE EXCEPTION 'Staff provenance accepted a missing post';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  UPDATE restrictions.bans SET account_id=NULL WHERE post_id=188;
  RAISE EXCEPTION 'Staff provenance accepted a missing account';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  UPDATE restrictions.bans SET ends_at=starts_at-interval '1 second' WHERE post_id=188;
  RAISE EXCEPTION 'Staff warning duration changed';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer readiness changed'; END IF; END $$;
RESET ROLE;
REVOKE USAGE ON SCHEMA public FROM board_restriction_owner;
