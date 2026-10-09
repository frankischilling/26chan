DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM public.subtitle_boards_before before FULL JOIN content.boards after USING(slug)
 WHERE before.saved IS DISTINCT FROM to_jsonb(after)-'board_subtitle') THEN RAISE EXCEPTION 'Existing board state changed'; END IF;
 IF EXISTS(SELECT 1 FROM public.subtitle_threads_before before FULL JOIN content.threads after USING(id)
 WHERE before.saved IS DISTINCT FROM to_jsonb(after)) THEN RAISE EXCEPTION 'Thread state changed'; END IF;
 IF EXISTS(SELECT 1 FROM public.subtitle_posts_before before FULL JOIN content.posts after USING(id)
 WHERE before.saved IS DISTINCT FROM to_jsonb(after)) THEN RAISE EXCEPTION 'Post state changed'; END IF;
 IF EXISTS(SELECT 1 FROM content.boards WHERE board_subtitle IS DISTINCT FROM CASE WHEN slug IN ('b','trash') THEN 'fiction' WHEN slug='gif' THEN 'worksafe_gif' ELSE 'none' END) THEN RAISE EXCEPTION 'Subtitle import differs'; END IF;
 IF EXISTS(TABLE public.subtitle_acl_before EXCEPT SELECT oid,relowner,relacl::text,relrowsecurity,relforcerowsecurity FROM pg_class WHERE oid='content.boards'::regclass)
 OR EXISTS(SELECT oid,relowner,relacl::text,relrowsecurity,relforcerowsecurity FROM pg_class WHERE oid='content.boards'::regclass EXCEPT TABLE public.subtitle_acl_before) THEN RAISE EXCEPTION 'Board authority changed'; END IF;
 BEGIN UPDATE content.boards SET board_subtitle='<script>alert(1)</script>' WHERE slug='b';
 RAISE EXCEPTION 'Hostile profile accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN UPDATE content.boards SET board_subtitle=NULL WHERE slug='b';
 RAISE EXCEPTION 'Null profile accepted'; EXCEPTION WHEN not_null_violation THEN NULL; END;
END $$;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('ownednew','Owned future board','Future description',1000,100,100,100,10);
DO $$ BEGIN
 IF (SELECT board_subtitle FROM content.boards WHERE slug='ownednew') IS DISTINCT FROM 'none' THEN RAISE EXCEPTION 'Future default differs'; END IF;
 IF EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor']) role WHERE has_column_privilege(role,'content.boards','board_subtitle','UPDATE')) THEN RAISE EXCEPTION 'Runtime subtitle write authority'; END IF;
END $$;
