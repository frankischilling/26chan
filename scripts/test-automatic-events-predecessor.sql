-- Owned synthetic evidence already stored by the predecessor schema.
BEGIN;
INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at,automatic_browser,automatic_prefix)
 VALUES(1000001,'demo',1000001,decode(repeat('11',32),'hex'),1700000000,'1deadbeef',decode(repeat('22',32),'hex'));
CREATE TABLE public.owned_event_history_before AS SELECT * FROM post_secrets.posting_history;
CREATE TABLE public.owned_event_posts_before AS SELECT * FROM content.posts;
SET LOCAL ROLE board_restriction_owner;
SELECT jsonb_build_object('bans',(SELECT jsonb_agg(to_jsonb(b)) FROM restrictions.bans b),
 'audit',(SELECT jsonb_agg(to_jsonb(a)) FROM restrictions.audit a),
 'capacity',(SELECT to_jsonb(c) FROM restrictions.capacity c))::text AS owned_restrictions_before \gset
RESET ROLE;
CREATE TABLE public.owned_event_restrictions_before AS SELECT :'owned_restrictions_before'::jsonb AS snapshot;
COMMIT;
