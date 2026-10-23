DO $$ BEGIN
 IF (SELECT jsonb_agg(to_jsonb(v) ORDER BY id) FROM public.owned_perks_before v)
 IS DISTINCT FROM (SELECT jsonb_agg(jsonb_build_object('id',p.id,'event_perk',p.event_perk) ORDER BY id) FROM content.posts p) THEN
 RAISE EXCEPTION 'Restore changed retained event perks'; END IF;
 IF EXISTS(SELECT 1 FROM public.owned_events_before b WHERE
 b.boards IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(v) ORDER BY slug) FROM content.boards v)
 OR b.posts IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(v)-'event_perk' ORDER BY id) FROM content.posts v)
 OR b.threads IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(v) ORDER BY id) FROM content.threads v)) THEN
 RAISE EXCEPTION 'Event upgrade changed board history'; END IF;
 IF (SELECT jsonb_agg(to_jsonb(a) ORDER BY run_id,actor_hash) FROM event_private.accounts a)
 IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(a) ORDER BY run_id,actor_hash) FROM public.owned_events_accounts a) THEN
 RAISE EXCEPTION 'Event restore changed accounts'; END IF;
 IF EXISTS(SELECT 1 FROM public.owned_scores_before b WHERE
 b.accounts IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(a) ORDER BY run_id,actor_hash) FROM event_private.score_accounts a)
 OR b.votes IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(v) ORDER BY run_id,actor_hash,post_id) FROM event_private.score_votes v)
 OR b.posts IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(p) ORDER BY run_id,post_id) FROM event_private.score_posts p)) THEN
 RAISE EXCEPTION 'Event restore changed score ledger'; END IF;
 IF has_column_privilege('board_anonymous_owner','post_secrets.posting_history','actor_hash','SELECT')
 OR has_function_privilege('board_anonymous_owner','content.lock_report_network_policy()','EXECUTE')
 OR has_function_privilege('board_anonymous_owner','content.network_admission_rules(text,bigint)','EXECUTE') THEN
 RAISE EXCEPTION 'Score restore broadened sealed authority'; END IF;
 IF has_schema_privilege('board_public','event_private','USAGE')
 OR has_table_privilege('board_public','event_private.accounts','SELECT,INSERT,UPDATE,DELETE')
 OR has_function_privilege('board_public','event_private.catalog_hash()','EXECUTE') THEN
 RAISE EXCEPTION 'Runtime gained private event authority'; END IF;
END $$;
SET ROLE board_public;
SELECT content.events_ready(),content.report_profile_ready(),content.public_pass_ready(),content.public_vip_ready(),content.public_challenge_ready();
DO $$ BEGIN
 IF content.events_ready() IS DISTINCT FROM true OR content.report_profile_ready() IS DISTINCT FROM true
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_vip_ready() IS DISTINCT FROM true
 OR content.public_challenge_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restored authority unavailable'; END IF;
 IF content.event_score_open() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Restored score event became active'; END IF;
 IF EXISTS(SELECT 1 FROM content.event_active_run(1::smallint)) OR EXISTS(SELECT 1 FROM content.event_active_run(2::smallint)) THEN
 RAISE EXCEPTION 'Restored closed event became active'; END IF;
END $$;
RESET ROLE;
BEGIN;
GRANT SELECT ON content.posts TO board_event_owner;
SET LOCAL ROLE board_public;
DO $$ BEGIN IF content.events_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Ignored owner privilege drift'; END IF; END $$;
ROLLBACK;
BEGIN;
GRANT SELECT ON event_private.accounts TO board_public;
SET LOCAL ROLE board_public;
DO $$ BEGIN IF content.events_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Ignored private account grant drift'; END IF; END $$;
ROLLBACK;
BEGIN;
UPDATE event_private.emotes SET value='untrusted' WHERE ordinal=0;
SET LOCAL ROLE board_public;
DO $$ BEGIN IF content.events_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Ignored roster drift'; END IF; END $$;
ROLLBACK;

BEGIN;
GRANT SELECT ON event_private.score_accounts TO board_public;
SET LOCAL ROLE board_public;
DO $$ BEGIN IF content.events_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Ignored private score grant drift'; END IF; END $$;
ROLLBACK;

BEGIN;
UPDATE content.posts SET since4pass=10017 WHERE id=(SELECT min(id) FROM content.posts);
DO $$ BEGIN
 BEGIN
  UPDATE content.posts SET since4pass=10018 WHERE id=(SELECT min(id) FROM content.posts);
  RAISE EXCEPTION 'Unknown stock identity accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
ROLLBACK;
