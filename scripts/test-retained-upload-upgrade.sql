DO $$ DECLARE role_name text; BEGIN
 IF (SELECT rows FROM public.owned_upload_digest_before) IS DISTINCT FROM
    (SELECT jsonb_agg(to_jsonb(m)-'normalized_md5' ORDER BY post_id) FROM content.post_media m)
 OR EXISTS(SELECT 1 FROM content.post_media WHERE normalized_md5 IS NOT NULL) THEN
  RAISE EXCEPTION 'Retained upload upgrade rewrote history or invented a digest'; END IF;
 FOREACH role_name IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
  IF has_column_privilege(role_name,'content.post_media','normalized_md5','SELECT,INSERT,UPDATE')
   OR has_function_privilege(role_name,'content.retain_attachment_digest()','EXECUTE') THEN
   RAISE EXCEPTION 'Retained upload authority leaked to %',role_name; END IF;
 END LOOP;
 IF EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid IN('content.visible_post_media'::regclass,'content.staff_post_media'::regclass)
     AND attname='normalized_md5' AND NOT attisdropped) THEN
  RAISE EXCEPTION 'Retained upload metadata escaped through a media view'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid='content.retain_attachment_digest()'::regprocedure
   AND proowner='board_attachment_owner'::regrole AND prosecdef
   AND proconfig=ARRAY['search_path=pg_catalog, pg_temp']::text[]
   AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(proacl,acldefault('f',proowner))) a
       WHERE a.grantee<>proowner OR a.is_grantable)) THEN
  RAISE EXCEPTION 'Retained upload trigger authority differs'; END IF;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Retained upload upgrade failed content readiness'; END IF;
END $$;
RESET ROLE;
