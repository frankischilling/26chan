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
 IF NOT EXISTS(SELECT 1 FROM content.posts WHERE substring(wordfilter_payload FROM 1 FOR 4)=decode('57463031','hex'))
 OR NOT EXISTS(SELECT 1 FROM content.posts WHERE substring(wordfilter_payload FROM 1 FOR 4)=decode('57463032','hex'))
 THEN RAISE EXCEPTION 'Historical typed payload fixtures are missing'; END IF;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer readiness did not survive generated formatting upgrade'; END IF;
END $$;
RESET ROLE;

-- Invoke only the actual format stamp on a temporary fixture shape. This
-- cannot authorize a post, staff limit, saved payload or attachment.
CREATE TEMP TABLE owned_generated_stamp(id bigint,thread_id bigint,board text,comment_format smallint,wordfilter_payload bytea);
CREATE TRIGGER stamp BEFORE INSERT ON owned_generated_stamp FOR EACH ROW EXECUTE FUNCTION content.stamp_comment_format();
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES('generated','Owned generated formatting','Synthetic policy matrix',2000,100,100,100,10);
DO $$ DECLARE mask integer; op boolean; reply_op boolean; actual smallint; expected smallint; header text; BEGIN
 FOR mask IN 0..7 LOOP
  UPDATE content.boards SET comment_spoiler_cleanup=(mask&1)<>0,
   comment_code_spacing=(mask&2)<>0,comment_sjis_spacing=(mask&4)<>0,op_markup=true
   WHERE slug='generated';
  FOREACH op IN ARRAY ARRAY[false,true] LOOP
   FOREACH reply_op IN ARRAY ARRAY[false,true] LOOP
    FOREACH header IN ARRAY ARRAY['WF01','WF02','WF03','WF04'] LOOP
     PERFORM set_config('board.source_op_reply',reply_op::text,true);
     INSERT INTO owned_generated_stamp VALUES(2,CASE WHEN op THEN 2 ELSE 1 END,'generated',0,convert_to(header,'UTF8'))
      RETURNING comment_format INTO actual;
     expected:=232+mask+CASE WHEN op OR reply_op THEN 16 ELSE 0 END+CASE WHEN header IN ('WF03','WF04') THEN 256 ELSE 0 END;
     IF actual<>expected THEN RAISE EXCEPTION 'Generated formatter policy differs: % <> %',actual,expected; END IF;
    END LOOP;
   END LOOP;
  END LOOP;
 END LOOP;
END $$;
