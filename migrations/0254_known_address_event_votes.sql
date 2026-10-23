-- Source known-address vote exemption, rechecked under the private run lock.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before optional event authority'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;

SELECT singleton FROM event_private.catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_event_owner;
DO $$ BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted before score scopes'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA content,event_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION event_private.apply_like(p_run bigint,p_board text,p_post bigint,p_actor bytea,
 p_session bytea,p_receipt bytea,p_peer bytea,p_context bytea,p_ipv4 bigint,p_known boolean)
RETURNS integer LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_run event_private.runs%ROWTYPE; v_now bigint; v_actor bytea; v_peer_actor bytea;
 v_target bytea; v_target_peer bytea; v_pass bytea; membership record; v_likes integer;
 v_post record; v_last bigint; v_bypass boolean:=false; v_range_blocked boolean; v_rules bigint;
BEGIN
 IF p_run IS NULL OR p_run<=0 OR p_post IS NULL OR p_post<=0 OR p_board IS NULL
 OR octet_length(p_board) NOT BETWEEN 1 AND 32 OR (p_ipv4 IS NOT NULL AND p_ipv4 NOT BETWEEN 1 AND 4294967295)
 OR p_actor IS NULL OR octet_length(p_actor)<>32 OR p_session IS NULL OR octet_length(p_session)<>32
 OR p_known IS NULL
 OR (p_known AND (p_receipt IS NOT NULL OR p_peer IS NOT NULL OR p_context IS NOT NULL))
 OR (NOT p_known AND (p_receipt IS NULL OR octet_length(p_receipt)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
 OR p_context IS NULL OR octet_length(p_context)<>32)) THEN
  RAISE EXCEPTION 'Bad Request.' USING ERRCODE='P0224'; END IF;
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted' USING ERRCODE='42501'; END IF;
 PERFORM content.lock_report_network_policy();
 -- Match the existing policy -> board -> verification -> event lock order. No event
 -- mutation acquires a board or a verification lock after the run lock.
 PERFORM slug FROM content.boards WHERE slug=p_board AND NOT staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'You can''t like this post.' USING ERRCODE='P0224'; END IF;
 IF p_known THEN
  PERFORM token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=p_session
   AND expires_at>floor(extract(epoch FROM clock_timestamp()))::bigint FOR UPDATE;
  IF NOT FOUND OR EXISTS(SELECT 1 FROM verification_private.browser_identities WHERE session_hash=p_session) THEN
   RAISE EXCEPTION 'Like verification is required.' USING ERRCODE='P0166'; END IF;
 ELSIF EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=p_receipt) THEN
  SELECT * INTO STRICT membership FROM verification_private.pass_take_receipt(p_receipt,p_peer,p_session,p_context,'like',p_board,p_post);
  SELECT token_hash INTO STRICT v_pass FROM verification_private.pass_entitlements WHERE id=membership.entitlement_id;
  v_bypass:=true;
 ELSE
  PERFORM verification_private.take_write_receipt_data(p_receipt,p_peer,p_session,'like',p_board,p_post,p_context);
 END IF;
 SELECT * INTO v_run FROM event_private.runs WHERE id=p_run AND kind=3 FOR UPDATE;
 v_now:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 IF NOT FOUND OR NOT v_run.enabled OR v_run.starts_at>v_now OR v_run.ends_at<=v_now THEN
  RAISE EXCEPTION 'Score event is closed.' USING ERRCODE='P0224'; END IF;
 v_peer_actor:=event_private.score_actor(p_run,'peer',p_actor);
 v_actor:=CASE WHEN v_bypass THEN event_private.score_actor(p_run,'pass',v_pass) ELSE v_peer_actor END;
 IF p_known AND NOT EXISTS(SELECT 1 FROM event_private.score_accounts
  WHERE run_id=p_run AND actor_hash=v_peer_actor AND last_vote_at IS NOT NULL) THEN
  RAISE EXCEPTION 'Like verification is required.' USING ERRCODE='P0166'; END IF;
 SELECT h.actor_hash,m.identity_hash INTO v_target_peer,v_target
 FROM post_secrets.posting_history h LEFT JOIN verification_private.pass_posts m ON m.post_id=h.post_id
 WHERE h.post_id=p_post AND h.board=p_board;
 IF NOT FOUND THEN RAISE EXCEPTION 'You can''t like this post.' USING ERRCODE='P0224'; END IF;
 v_target:=CASE WHEN v_target IS NULL THEN event_private.score_actor(p_run,'peer',v_target_peer)
  ELSE event_private.score_actor(p_run,'pass',v_target) END;
 IF v_target_peer=p_actor OR v_target=v_actor THEN RAISE EXCEPTION 'You can''t like your own posts.' USING ERRCODE='P0224'; END IF;
 IF EXISTS(SELECT 1 FROM event_private.score_votes WHERE run_id=p_run AND actor_hash=v_actor AND post_id=p_post) THEN
  RAISE EXCEPTION 'You already like this post.' USING ERRCODE='P0224'; END IF;
 SELECT max(last_vote_at) INTO v_last FROM event_private.score_accounts
 WHERE run_id=p_run AND actor_hash IN(v_actor,v_peer_actor);
 v_range_blocked:=false;
 IF NOT v_bypass AND p_ipv4 IS NOT NULL THEN
  SELECT count(*),coalesce(bool_or(r.range_start<=p_ipv4 AND r.range_end>=p_ipv4
   AND r.boards IN('','0') AND NOT r.ops_only AND NOT r.images_only AND NOT r.reports_only
   AND r.signatures IN('','0')),false) INTO v_rules,v_range_blocked
   FROM content.network_admission_rules(p_board,v_now) r;
  IF v_rules>128 THEN RAISE EXCEPTION 'Event network policy unavailable' USING ERRCODE='42501'; END IF;
 END IF;
 IF v_last>v_now-120 OR v_range_blocked THEN
  RAISE EXCEPTION 'You have to wait a while before liking this post.' USING ERRCODE='P0224'; END IF;
 SELECT p.id,p.thread_id,p.deleted,p.content_erased,t.archived_at,t.deleted AS thread_deleted,t.content_erased AS thread_erased
 INTO v_post FROM content.posts p JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
 WHERE p.id=p_post AND p.board=p_board;
 IF NOT FOUND THEN RAISE EXCEPTION 'This post doesn''t exist anymore.' USING ERRCODE='P0224'; END IF;
 IF v_post.id=v_post.thread_id OR v_post.deleted OR v_post.content_erased OR v_post.archived_at IS NOT NULL OR v_post.thread_deleted OR v_post.thread_erased THEN
  RAISE EXCEPTION 'You can''t like this post.' USING ERRCODE='P0224'; END IF;
 SELECT likes+1 INTO v_likes FROM event_private.score_posts WHERE run_id=p_run AND post_id=p_post;
 v_likes:=coalesce(v_likes,1);
 IF v_likes>=1048576 THEN RAISE EXCEPTION 'You can''t like this post anymore.' USING ERRCODE='P0224'; END IF;
 IF v_run.vote_count>=1000000 THEN RAISE EXCEPTION 'Event vote capacity reached.' USING ERRCODE='P0224'; END IF;
 PERFORM event_private.reserve_score_actor(p_run,v_actor);
 IF p_board NOT IN('b','qa','s4s','bant','vip') THEN
  PERFORM event_private.reserve_score_actor(p_run,v_target);
  UPDATE event_private.score_accounts SET score=score+CASE WHEN actor_hash=v_actor THEN 1 ELSE 10 END
   WHERE run_id=p_run AND actor_hash IN(v_actor,v_target);
 END IF;
 UPDATE event_private.score_accounts SET last_vote_at=v_now WHERE run_id=p_run AND actor_hash=v_actor;
 INSERT INTO event_private.score_votes VALUES(p_run,v_actor,p_post,p_board,v_now);
 INSERT INTO event_private.score_posts VALUES(p_run,p_post,v_likes)
  ON CONFLICT(run_id,post_id) DO UPDATE SET likes=EXCLUDED.likes;
 UPDATE event_private.runs SET vote_count=vote_count+1 WHERE id=p_run;
 RETURN v_likes;
END $$;
REVOKE ALL ON FUNCTION event_private.apply_like(bigint,text,bigint,bytea,bytea,bytea,bytea,bytea,bigint,boolean) FROM PUBLIC;
CREATE OR REPLACE FUNCTION content.event_like(p_run bigint,p_board text,p_post bigint,p_actor bytea,
 p_session bytea,p_receipt bytea,p_peer bytea,p_context bytea,p_ipv4 bigint)
RETURNS integer LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT event_private.apply_like(p_run,p_board,p_post,p_actor,p_session,p_receipt,p_peer,p_context,p_ipv4,false)
$$;
CREATE FUNCTION content.event_like_known(p_run bigint,p_board text,p_post bigint,p_actor bytea,p_session bytea,p_ipv4 bigint)
RETURNS integer LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT event_private.apply_like(p_run,p_board,p_post,p_actor,p_session,NULL,NULL,NULL,p_ipv4,true)
$$;
REVOKE ALL ON FUNCTION content.event_like_known(bigint,text,bigint,bytea,bytea,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.event_like_known(bigint,text,bigint,bytea,bytea,bigint) TO board_public;
-- Advice never authorizes a write: the mutation rechecks this private history
-- after the run lock and requires the existing browser session to be live.
CREATE FUNCTION content.event_like_exemption(p_run bigint,p_actor bytea,p_session bytea) RETURNS boolean
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted' USING ERRCODE='42501'; END IF;
 IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_session IS NULL OR octet_length(p_session)<>32 THEN RETURN false; END IF;
 RETURN EXISTS(SELECT 1 FROM event_private.runs r JOIN event_private.score_accounts a ON a.run_id=r.id
  WHERE r.id=p_run AND r.kind=3 AND r.enabled AND r.starts_at<=extract(epoch FROM statement_timestamp())
   AND r.ends_at>extract(epoch FROM statement_timestamp())
   AND a.actor_hash=event_private.score_actor(p_run,'peer',p_actor) AND a.last_vote_at IS NOT NULL)
  AND NOT EXISTS(SELECT 1 FROM verification_private.browser_identities WHERE session_hash=p_session);
END $$;
REVOKE ALL ON FUNCTION content.event_like_exemption(bigint,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.event_like_exemption(bigint,bytea,bytea) TO board_public;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,event_private FROM board_anonymous_owner;
SET LOCAL ROLE board_event_owner;
SELECT set_config('board.migration_score_event_hash',encode(event_private.catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE event_private.catalog SET definition_hash=decode(current_setting('board.migration_score_event_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_image_classifier_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog SET definition_hash=decode(current_setting('board.migration_image_classifier_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_image_report_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_image_report_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed challenge authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_event_owner;
DO $$ BEGIN IF content.events_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Known-address migration changed event authority'; END IF; END $$;
RESET ROLE;
