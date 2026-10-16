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
 IF NOT EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='content.stamp_comment_format()'::regprocedure
   AND NOT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
   AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0))
 THEN RAISE EXCEPTION 'Stamp authority changed'; END IF;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer readiness did not survive format upgrade'; END IF;
END $$;
RESET ROLE;

-- Call the real stamp trigger on a temporary shape. This does not create or
-- authorize a content post, staff receipt or attachment.
CREATE TEMP TABLE owned_stamp(id bigint,thread_id bigint,board text,comment_format smallint);
CREATE TRIGGER stamp BEFORE INSERT ON owned_stamp FOR EACH ROW EXECUTE FUNCTION content.stamp_comment_format();
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES('greentext','Owned greentext upgrade','Synthetic policy matrix',2000,100,100,100,10);
DO $$ DECLARE mask integer; op boolean; reply_op boolean; actual smallint; expected smallint; BEGIN
 FOR mask IN 0..7 LOOP
  UPDATE content.boards SET comment_spoiler_cleanup=(mask&1)<>0,
   comment_code_spacing=(mask&2)<>0,comment_sjis_spacing=(mask&4)<>0,op_markup=true
   WHERE slug='greentext';
  FOREACH op IN ARRAY ARRAY[false,true] LOOP
   FOREACH reply_op IN ARRAY ARRAY[false,true] LOOP
    PERFORM set_config('board.source_op_reply',reply_op::text,true);
    INSERT INTO owned_stamp VALUES(2,CASE WHEN op THEN 2 ELSE 1 END,'greentext',0)
     RETURNING comment_format INTO actual;
    expected:=232+mask+CASE WHEN op OR reply_op THEN 16 ELSE 0 END;
    IF actual<>expected THEN RAISE EXCEPTION 'New formatter policy differs: % <> %',actual,expected; END IF;
   END LOOP;
  END LOOP;
 END LOOP;
END $$;
