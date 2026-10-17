SET ROLE board_restriction_owner;
DO $$ BEGIN
 IF (SELECT bans FROM public.owned_file_restrictions_before) IS DISTINCT FROM
    (SELECT jsonb_agg(to_jsonb(b)-ARRAY['template_id','upload_observation','normalized_upload_md5'] ORDER BY id) FROM restrictions.bans b)
 OR (SELECT audit FROM public.owned_file_restrictions_before) IS DISTINCT FROM
    (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM restrictions.audit a)
 OR (SELECT capacity FROM public.owned_file_restrictions_before) IS DISTINCT FROM
    (SELECT to_jsonb(c) FROM restrictions.capacity c)
 OR EXISTS(SELECT 1 FROM restrictions.bans WHERE template_id IS NOT NULL
    OR upload_observation IS NOT NULL OR normalized_upload_md5 IS NOT NULL)
 OR EXISTS(SELECT 1 FROM restrictions.templates) THEN
  RAISE EXCEPTION 'Ban template upgrade rewrote or invented history'; END IF;
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Ban template upgrade is not ready'; END IF;
 BEGIN
  INSERT INTO restrictions.templates VALUES(1,'Owned invalid','moderator',true,true,false,60,'Public','Private',false);
  RAISE EXCEPTION 'Source file-required template accepted false policy';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  INSERT INTO restrictions.templates VALUES(900001,'Owned invalid','moderator',true,true,true,NULL,'Public','Private',false);
  RAISE EXCEPTION 'Warning template accepted unknown duration';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
RESET ROLE;
DO $$ DECLARE role_name text; BEGIN
 FOREACH role_name IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
  IF has_table_privilege(role_name,'restrictions.templates','SELECT,INSERT,UPDATE,DELETE')
   OR has_column_privilege(role_name,'restrictions.bans','normalized_upload_md5','SELECT,INSERT,UPDATE') THEN
   RAISE EXCEPTION 'Private template history leaked to %',role_name; END IF;
  IF has_function_privilege(role_name,'content.issue_post_template_restriction(bytea,bytea,integer,text,bigint,bigint)','EXECUTE')
     IS DISTINCT FROM (role_name='board_staff') THEN
   RAISE EXCEPTION 'Template issuer grant differs for %',role_name; END IF;
  IF has_function_privilege(role_name,'staff_identity.restriction_templates(bytea,bytea,integer,text)','EXECUTE')
     IS DISTINCT FROM (role_name='board_auth') THEN
   RAISE EXCEPTION 'Template listing grant differs for %',role_name; END IF;
 END LOOP;
 IF has_schema_privilege('board_auth','content','USAGE') OR has_schema_privilege('board_staff','staff_identity','USAGE') THEN
  RAISE EXCEPTION 'Template path expanded authentication/content schema access'; END IF;
END $$;
BEGIN;
GRANT CREATE ON SCHEMA staff_identity TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Template namespace authority drift was accepted'; END IF; END $$;
ROLLBACK;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Template upgrade changed content readiness'; END IF; END $$;
RESET ROLE;
