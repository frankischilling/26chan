-- Owned populated fixture, applied after migration 0119 and the demo seed.
DO $$ BEGIN
 IF (SELECT title FROM content.boards WHERE slug='s4s') IS DISTINCT FROM '[s4s] - Sh*t 4chan Says'
 THEN RAISE EXCEPTION 'Expected the historical s4s import before migration 0120'; END IF;
END $$;
UPDATE content.boards SET description='Operator description <b>kept</b>',show_blotter=false,max_comment_chars=1234 WHERE slug='s4s';
UPDATE content.boards SET title='Operator title & Pokémon',description='Unchanged other board' WHERE slug='b';
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('ownedtitle','[s4s] - Sh*t 4chan Says','Same old title on a different slug',1000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,bumped_at,modified_at,http_modified_at,reply_count)
VALUES(8801201,'s4s','2026-09-08 12:00:00+00','2026-09-08 12:05:00+00','2026-09-08 12:05:00+00','2026-09-08 12:06:00+00',1);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8801201,'s4s',8801201,'Anonymous','Historical title','Retained OP <text> & content','2026-09-08 12:00:00+00'),
      (8801202,'s4s',8801201,'Anonymous','','Retained reply','2026-09-08 12:05:00+00');
CREATE TABLE public.title_boards_before AS SELECT slug,to_jsonb(b) saved FROM content.boards b;
CREATE TABLE public.title_threads_before AS SELECT id,to_jsonb(t) saved FROM content.threads t;
CREATE TABLE public.title_posts_before AS SELECT id,to_jsonb(p) saved FROM content.posts p;
CREATE TABLE public.title_acl_before AS
 SELECT oid,relowner,relacl::text,relrowsecurity,relforcerowsecurity FROM pg_class
 WHERE oid IN ('content.boards'::regclass,'content.threads'::regclass,'content.posts'::regclass);
CREATE TABLE public.title_column_acl_before AS
 SELECT attrelid,attnum,attacl::text FROM pg_attribute
 WHERE attrelid IN ('content.boards'::regclass,'content.threads'::regclass,'content.posts'::regclass)
 AND attnum>0 AND NOT attisdropped;
CREATE TABLE public.title_policies_before AS
 SELECT oid,to_jsonb(p) saved FROM pg_policy p
 WHERE polrelid IN ('content.boards'::regclass,'content.threads'::regclass,'content.posts'::regclass);
