-- This file runs only in the owned upgrade database created by the shell test.
DO $$ BEGIN
 IF EXISTS(SELECT * FROM content.posts EXCEPT SELECT * FROM public.owned_posts_before)
 OR EXISTS(SELECT * FROM public.owned_posts_before EXCEPT SELECT * FROM content.posts)
 OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_threads_before)
 OR EXISTS(SELECT * FROM public.owned_threads_before EXCEPT SELECT * FROM content.threads)
 OR EXISTS(SELECT * FROM content.boards EXCEPT SELECT * FROM public.owned_boards_before)
 OR EXISTS(SELECT * FROM public.owned_boards_before EXCEPT SELECT * FROM content.boards)
 OR EXISTS(SELECT * FROM content.moderation_audit EXCEPT SELECT * FROM public.owned_audits_before)
 OR EXISTS(SELECT * FROM public.owned_audits_before EXCEPT SELECT * FROM content.moderation_audit)
 THEN RAISE EXCEPTION 'Historical content, profiles, policy, clocks or audit changed'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='content.require_attachment_for_empty_post()'::regprocedure
   AND p.proowner='board_attachment_owner'::regrole AND p.prosecdef
   AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
   AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0))
 THEN RAISE EXCEPTION 'Deferred attachment authority changed'; END IF;
 IF has_function_privilege('board_migrator','content.require_attachment_for_empty_post()','EXECUTE')
 OR has_function_privilege('board_public','content.require_attachment_for_empty_post()','EXECUTE')
 OR has_schema_privilege('board_attachment_owner','content','CREATE')
 THEN RAISE EXCEPTION 'Temporary attachment privileges remain'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='content.posts'::regclass
   AND t.tgname='posts_empty_comment_attachment' AND t.tgenabled='O' AND t.tgdeferrable
   AND t.tginitdeferred AND t.tgtype=21
   AND (SELECT array_agg(a.attname::text ORDER BY a.attname)
       FROM unnest(t.tgattr::smallint[]) n JOIN pg_attribute a ON a.attrelid=t.tgrelid AND a.attnum=n)
       =ARRAY['comment','dice_result','fortune_color','fortune_text','subject'])
 THEN RAISE EXCEPTION 'Final-row generated content coverage changed'; END IF;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer readiness did not survive generated-content upgrade'; END IF;
END $$;
RESET ROLE;

