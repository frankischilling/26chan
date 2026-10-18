DO $$
DECLARE role_name text;
BEGIN
 IF EXISTS(SELECT 1 FROM media.input_receipts) THEN
  RAISE EXCEPTION 'Historical original input must remain unknown.';
 END IF;
 IF EXISTS(
  WITH current_rows AS (
   SELECT 'asset'::text kind,to_jsonb(a) value FROM media.assets a
   UNION ALL SELECT 'job',to_jsonb(j) FROM media.jobs j
   UNION ALL SELECT 'approved',to_jsonb(a) FROM media.approved_assets a
   UNION ALL SELECT 'post',to_jsonb(p) FROM content.posts p
  ), differences AS (
   (SELECT * FROM current_rows EXCEPT ALL SELECT * FROM public.input_receipt_predecessor)
   UNION ALL
   (SELECT * FROM public.input_receipt_predecessor EXCEPT ALL SELECT * FROM current_rows)
  ) SELECT 1 FROM differences
 ) THEN RAISE EXCEPTION 'Receipt upgrade changed historical rows.'; END IF;
 FOREACH role_name IN ARRAY ARRAY['board_public','board_staff','board_media_read','board_media_intake','board_auth','board_media'] LOOP
  IF has_table_privilege(role_name,'media.input_receipts','SELECT,INSERT,UPDATE,DELETE') THEN
   RAISE EXCEPTION 'Private input receipts have an unexpected table grant.';
  END IF;
  IF has_function_privilege(role_name,'media.record_input_receipt(text,text,text,text,bigint,bytea,text)','EXECUTE') IS DISTINCT FROM (role_name='board_media')
     OR has_function_privilege(role_name,'media.require_input_receipt(text,text,text,text,bigint,bytea,text)','EXECUTE') IS DISTINCT FROM (role_name='board_media') THEN
   RAISE EXCEPTION 'Original input authority differs.';
  END IF;
 END LOOP;
 IF EXISTS(SELECT 1 FROM pg_proc WHERE oid IN (
   'media.record_input_receipt(text,text,text,text,bigint,bytea,text)'::regprocedure,
   'media.require_input_receipt(text,text,text,text,bigint,bytea,text)'::regprocedure)
   AND (proowner<>'board_migrator'::regrole OR NOT prosecdef OR proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog'])) THEN
  RAISE EXCEPTION 'Input receipt function authority differs.';
 END IF;
END $$;
