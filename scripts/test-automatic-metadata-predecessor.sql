-- Synthetic pre-migration identity; no browser or address can be recovered.
BEGIN;
INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at)
 VALUES(1000001,'demo',1000001,decode(repeat('11',32),'hex'),1700000000);
CREATE TABLE public.owned_automatic_history_before AS SELECT * FROM post_secrets.posting_history;
CREATE TABLE public.owned_automatic_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_automatic_threads_before AS SELECT * FROM content.threads;
COMMIT;
