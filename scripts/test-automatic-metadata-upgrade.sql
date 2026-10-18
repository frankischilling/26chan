DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE automatic_browser IS NOT NULL OR automatic_prefix IS NOT NULL) THEN
  RAISE EXCEPTION 'Historical metadata was invented'; END IF;
 IF EXISTS((SELECT to_jsonb(h)-'automatic_browser'-'automatic_prefix' FROM post_secrets.posting_history h
            EXCEPT ALL SELECT to_jsonb(h) FROM public.owned_automatic_history_before h)
           UNION ALL
           (SELECT to_jsonb(h) FROM public.owned_automatic_history_before h
            EXCEPT ALL SELECT to_jsonb(h)-'automatic_browser'-'automatic_prefix' FROM post_secrets.posting_history h)) THEN
  RAISE EXCEPTION 'Historical posting identity changed'; END IF;
 IF EXISTS((SELECT * FROM content.posts EXCEPT ALL SELECT * FROM public.owned_automatic_posts_before)
           UNION ALL (SELECT * FROM public.owned_automatic_posts_before EXCEPT ALL SELECT * FROM content.posts))
 OR EXISTS((SELECT * FROM content.threads EXCEPT ALL SELECT * FROM public.owned_automatic_threads_before)
           UNION ALL (SELECT * FROM public.owned_automatic_threads_before EXCEPT ALL SELECT * FROM content.threads)) THEN
  RAISE EXCEPTION 'Historical content changed'; END IF;
 IF EXISTS(SELECT 1 FROM pg_roles r CROSS JOIN (VALUES('automatic_browser'),('automatic_prefix')) a(name)
   WHERE r.rolname IN('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor','board_anonymous_owner')
     AND has_column_privilege(r.oid,'post_secrets.posting_history'::regclass,a.name,'SELECT,INSERT,UPDATE,REFERENCES')) THEN
  RAISE EXCEPTION 'Private metadata privileges broadened'; END IF;
END $$;
BEGIN;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer boundary unavailable'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction boundary unavailable'; END IF; END $$;
ROLLBACK;
-- Constraint failures must be atomic and keep the old evidence unchanged.
DO $$ BEGIN
 BEGIN
  UPDATE post_secrets.posting_history SET automatic_browser='1deadbeef' WHERE post_id=1000001;
  RAISE EXCEPTION 'Partial metadata accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  UPDATE post_secrets.posting_history SET automatic_browser='0deadbeef',automatic_prefix=decode(repeat('22',32),'hex') WHERE post_id=1000001;
  RAISE EXCEPTION 'Desktop metadata accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  UPDATE post_secrets.posting_history SET automatic_browser='1deadbeef',automatic_prefix=decode('22','hex') WHERE post_id=1000001;
  RAISE EXCEPTION 'Short prefix accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
