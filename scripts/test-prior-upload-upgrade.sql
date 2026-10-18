SET ROLE board_restriction_owner;
DO $$ BEGIN
 IF (SELECT bans FROM public.owned_upload_history_before) IS DISTINCT FROM
    (SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM restrictions.bans b)
 OR (SELECT audit FROM public.owned_upload_history_before) IS DISTINCT FROM
    (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM restrictions.audit a)
 OR (SELECT capacity FROM public.owned_upload_history_before) IS DISTINCT FROM
    (SELECT to_jsonb(c) FROM restrictions.capacity c) THEN
  RAISE EXCEPTION 'Upload history upgrade changed existing evidence'; END IF;
 IF restrictions.prior_upload_history(decode(repeat('62',16),'hex'),false) IS DISTINCT FROM true
 OR restrictions.prior_upload_history(decode(repeat('63',16),'hex'),false) IS DISTINCT FROM false
 OR restrictions.prior_upload_history(decode(repeat('63',16),'hex'),true) IS DISTINCT FROM true
 OR restrictions.prior_upload_history(decode(repeat('64',16),'hex'),false) IS DISTINCT FROM false
 OR restrictions.prior_upload_history(NULL,false) IS NOT NULL THEN
  RAISE EXCEPTION 'Upload history upgrade changed retained event semantics'; END IF;
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Upload history upgrade is not ready'; END IF;
END $$;
RESET ROLE;
DO $$ DECLARE role_name text; BEGIN
 FOREACH role_name IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
  IF has_function_privilege(role_name,'content.prior_ban_upload_digest(text,text,text,bigint,boolean,boolean)','EXECUTE')
   OR has_function_privilege(role_name,'restrictions.prior_upload_history(bytea,boolean)','EXECUTE')
   OR has_function_privilege(role_name,'restrictions.prior_upload_decision(boolean,boolean,integer,integer,integer,integer,integer,integer)','EXECUTE')
   OR has_function_privilege(role_name,'content.assert_prior_upload_history()','EXECUTE') THEN
   RAISE EXCEPTION 'Private upload-history authority leaked to %',role_name; END IF;
  IF has_function_privilege(role_name,'content.prior_upload_banned(text,text,text,bigint,boolean,boolean)','EXECUTE')
    IS DISTINCT FROM (role_name IN('board_public','board_staff')) THEN
   RAISE EXCEPTION 'Upload-history entry point grant differs for %',role_name; END IF;
 END LOOP;
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid='content.prior_upload_banned(text,text,text,bigint,boolean,boolean)'::regprocedure
  AND prorettype='boolean'::regtype AND NOT proretset AND prosecdef
  AND proowner='board_restriction_owner'::regrole AND proconfig=ARRAY['search_path=pg_catalog, pg_temp']::text[]) THEN
  RAISE EXCEPTION 'Upload-history API exposes the wrong contract'; END IF;
END $$;
BEGIN;
SET LOCAL ROLE board_attachment_owner;
GRANT EXECUTE ON FUNCTION content.prior_ban_upload_digest(text,text,text,bigint,boolean,boolean) TO PUBLIC;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Upload digest dependency grant drift was accepted'; END IF; END $$;
ROLLBACK;
BEGIN;
ALTER TABLE content.post_media DISABLE TRIGGER zzz_prior_upload_history;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Late upload guard drift was accepted'; END IF; END $$;
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_attachment_owner;
ALTER FUNCTION content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean,timestamptz) COST 101;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Public attachment writer drift was accepted'; END IF; END $$;
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_attachment_owner;
ALTER FUNCTION content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean) COST 101;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Public attachment wrapper drift was accepted'; END IF; END $$;
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_attachment_owner;
ALTER FUNCTION content.consume_staff_attachment_receipt(bigint,text,bigint,text,bytea,boolean,boolean) COST 101;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Staff attachment writer drift was accepted'; END IF; END $$;
ROLLBACK;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Upload history changed content readiness'; END IF; END $$;
RESET ROLE;
