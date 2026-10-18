DO $$ BEGIN
 IF EXISTS((SELECT * FROM post_secrets.posting_history EXCEPT ALL SELECT * FROM public.owned_event_history_before)
   UNION ALL (SELECT * FROM public.owned_event_history_before EXCEPT ALL SELECT * FROM post_secrets.posting_history))
 OR EXISTS((SELECT * FROM content.posts EXCEPT ALL SELECT * FROM public.owned_event_posts_before)
   UNION ALL (SELECT * FROM public.owned_event_posts_before EXCEPT ALL SELECT * FROM content.posts)) THEN
  RAISE EXCEPTION 'Automatic event upgrade changed saved identity or content'; END IF;
END $$;
BEGIN;
SELECT snapshot::text AS owned_restrictions_before FROM public.owned_event_restrictions_before \gset
SET LOCAL ROLE board_restriction_owner;
SELECT jsonb_build_object('bans',(SELECT jsonb_agg(to_jsonb(b)) FROM restrictions.bans b),
 'audit',(SELECT jsonb_agg(to_jsonb(a)) FROM restrictions.audit a),
 'capacity',(SELECT to_jsonb(c) FROM restrictions.capacity c)) = :'owned_restrictions_before'::jsonb AS unchanged \gset
\if :unchanged
\else
\quit 1
\endif
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true OR EXISTS(SELECT 1 FROM restrictions.automatic_ranges) THEN
  RAISE EXCEPTION 'Automatic event catalog is unavailable or prepopulated'; END IF;
 IF EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor')
   AND (has_table_privilege(r.oid,'restrictions.automatic_ranges','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
     OR has_sequence_privilege(r.oid,'restrictions.automatic_ranges_id_seq','USAGE,SELECT,UPDATE')
     OR has_function_privilege(r.oid,'restrictions.record_automatic_range(text,bigint,bigint)','EXECUTE')
     OR has_function_privilege(r.oid,'content.lock_automatic_range_policy(boolean)','EXECUTE'))) THEN
  RAISE EXCEPTION 'Automatic event private authority broadened'; END IF;
END $$;
ROLLBACK;
-- Seal rejects a runtime sequence grant, including a normally implicit ACL.
BEGIN;
SET LOCAL ROLE board_restriction_owner;
GRANT USAGE ON SEQUENCE restrictions.automatic_ranges_id_seq TO board_public;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Automatic event catalog accepted sequence grant drift'; END IF; END $$;
ROLLBACK;
