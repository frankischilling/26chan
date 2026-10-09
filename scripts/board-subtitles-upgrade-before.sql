-- Owned fixture: operator-edited descriptions, policy, and an additional board.
UPDATE content.boards SET description='Operator description <b>kept</b>',title='Operator title',show_blotter=false,max_comment_chars=1234 WHERE slug='b';
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('ownedsub','Owned subtitle upgrade','Arbitrary operator description',1000,100,100,100,10);
CREATE TABLE public.subtitle_boards_before AS SELECT slug,to_jsonb(b) saved FROM content.boards b;
CREATE TABLE public.subtitle_threads_before AS SELECT id,to_jsonb(t) saved FROM content.threads t;
CREATE TABLE public.subtitle_posts_before AS SELECT id,to_jsonb(p) saved FROM content.posts p;
CREATE TABLE public.subtitle_acl_before AS SELECT oid,relowner,relacl::text,relrowsecurity,relforcerowsecurity FROM pg_class WHERE oid='content.boards'::regclass;
