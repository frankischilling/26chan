-- Run from the repository root so psql can include the migration below.
BEGIN;
CREATE FUNCTION pg_temp.assert_title_upgrade(expected_title text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
 IF (SELECT title FROM content.boards WHERE slug='s4s') IS DISTINCT FROM expected_title
 THEN RAISE EXCEPTION 's4s title differs from the expected upgrade state'; END IF;
 IF EXISTS(SELECT 1 FROM public.title_boards_before before FULL JOIN content.boards after USING(slug)
 WHERE CASE WHEN before.slug='s4s' THEN jsonb_set(before.saved,'{title}',to_jsonb(expected_title))
            ELSE before.saved END IS DISTINCT FROM to_jsonb(after))
 THEN RAISE EXCEPTION 'Unrelated board state changed'; END IF;
 IF EXISTS(SELECT 1 FROM public.title_threads_before before FULL JOIN content.threads after USING(id)
 WHERE before.saved IS DISTINCT FROM to_jsonb(after)) THEN RAISE EXCEPTION 'Thread state changed'; END IF;
 IF EXISTS(SELECT 1 FROM public.title_posts_before before FULL JOIN content.posts after USING(id)
 WHERE before.saved IS DISTINCT FROM to_jsonb(after)) THEN RAISE EXCEPTION 'Post state changed'; END IF;
 IF EXISTS(TABLE public.title_acl_before EXCEPT
 SELECT oid,relowner,relacl::text,relrowsecurity,relforcerowsecurity FROM pg_class
 WHERE oid IN ('content.boards'::regclass,'content.threads'::regclass,'content.posts'::regclass))
 OR EXISTS(SELECT oid,relowner,relacl::text,relrowsecurity,relforcerowsecurity FROM pg_class
 WHERE oid IN ('content.boards'::regclass,'content.threads'::regclass,'content.posts'::regclass)
 EXCEPT TABLE public.title_acl_before) THEN RAISE EXCEPTION 'Content ownership or table authority changed'; END IF;
 IF EXISTS(TABLE public.title_column_acl_before EXCEPT
 SELECT attrelid,attnum,attacl::text FROM pg_attribute
 WHERE attrelid IN ('content.boards'::regclass,'content.threads'::regclass,'content.posts'::regclass)
 AND attnum>0 AND NOT attisdropped)
 OR EXISTS(SELECT attrelid,attnum,attacl::text FROM pg_attribute
 WHERE attrelid IN ('content.boards'::regclass,'content.threads'::regclass,'content.posts'::regclass)
 AND attnum>0 AND NOT attisdropped EXCEPT TABLE public.title_column_acl_before)
 THEN RAISE EXCEPTION 'Content column authority changed'; END IF;
 IF EXISTS(TABLE public.title_policies_before EXCEPT SELECT oid,to_jsonb(p) saved FROM pg_policy p
 WHERE polrelid IN ('content.boards'::regclass,'content.threads'::regclass,'content.posts'::regclass))
 OR EXISTS(SELECT oid,to_jsonb(p) saved FROM pg_policy p
 WHERE polrelid IN ('content.boards'::regclass,'content.threads'::regclass,'content.posts'::regclass)
 EXCEPT TABLE public.title_policies_before) THEN RAISE EXCEPTION 'Content row policies changed'; END IF;
 IF EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor']) role
 WHERE has_column_privilege(role,'content.boards','title','UPDATE'))
 THEN RAISE EXCEPTION 'Runtime title write authority'; END IF;
END $$;
SELECT pg_temp.assert_title_upgrade('Sh*t 4chan Says');
-- Reapplying the exact migration leaves an already-corrected title unchanged.
\i migrations/0120_board_short_titles.sql
SELECT pg_temp.assert_title_upgrade('Sh*t 4chan Says');
UPDATE content.boards SET title='Operator''s <title> & Pokémon' WHERE slug='s4s';
\i migrations/0120_board_short_titles.sql
SELECT pg_temp.assert_title_upgrade('Operator''s <title> & Pokémon');
ROLLBACK;
