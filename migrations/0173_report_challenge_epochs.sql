-- Track report identity epochs before trusting retained exemption history.
SET ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Restriction catalog drifted before report challenge history'; END IF;
END $$;
RESET ROLE;
SET ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Report snapshot catalog drifted before report challenge history'; END IF;
END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before report challenge history'; END IF;
END $$;
RESET ROLE;
ALTER TABLE verification_private.policy ADD COLUMN report_history_epoch bytea
 CHECK(report_history_epoch IS NULL OR octet_length(report_history_epoch)=32);
GRANT CREATE ON SCHEMA content,restrictions TO board_restriction_owner;
SET ROLE board_restriction_owner;
CREATE INDEX restriction_history_started ON restrictions.bans(starts_at DESC);
CREATE INDEX restriction_actor_history ON restrictions.bans(actor_hash,starts_at DESC);
CREATE FUNCTION content.report_restriction_history_horizon() RETURNS timestamptz
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT max(starts_at)+interval '30 days' FROM restrictions.bans
$$;
REVOKE ALL ON FUNCTION content.report_restriction_history_horizon() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.report_restriction_history_horizon() TO board_anonymous_owner;
GRANT EXECUTE ON FUNCTION content.restrictions_ready() TO board_anonymous_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,restrictions FROM board_restriction_owner;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION verification_private.observe_report_epoch(p_epoch bytea) RETURNS void
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE previous bytea;
BEGIN
 IF p_epoch IS NOT NULL AND octet_length(p_epoch)<>32 THEN
  RAISE EXCEPTION 'Invalid report history epoch.' USING ERRCODE='23514'; END IF;
 SELECT report_history_epoch INTO previous FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
 IF p_epoch IS NULL OR previous IS DISTINCT FROM p_epoch THEN
  -- Preserve omitted-event watermarks as well as all retained report, clear,
  -- and restriction windows. A known epoch need not restart an elapsed
  -- migration warm-up on an otherwise quiet site. Unknown continuity does.
  UPDATE verification_private.policy SET report_history_epoch=coalesce(p_epoch,report_history_epoch),
   report_history_complete_after=greatest(report_history_complete_after,
    (SELECT max(expires_at) FROM verification_private.report_history),
    content.report_restriction_history_horizon(),
    CASE WHEN p_epoch IS NULL THEN clock_timestamp()+interval '30 days' END) WHERE singleton;
 END IF;
END $$;
REVOKE ALL ON FUNCTION verification_private.observe_report_epoch(bytea) FROM PUBLIC;
CREATE FUNCTION content.observe_report_challenge_epoch() RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE raw text;
BEGIN
 raw:=current_setting('board.report_history_epoch',true);
 IF raw IS NOT NULL AND octet_length(raw)=64 AND raw ~ '^[0-9a-f]{64}$' THEN
  PERFORM verification_private.observe_report_epoch(decode(raw,'hex'));
 ELSE
  -- Missing transport metadata cannot establish continuity, but still must
  -- not prevent an otherwise valid CAPTCHA-backed report or operator import.
  PERFORM verification_private.observe_report_epoch(NULL);
 END IF;
END $$;
REVOKE ALL ON FUNCTION content.observe_report_challenge_epoch() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.observe_report_challenge_epoch() TO board_report_admission_owner;
CREATE OR REPLACE FUNCTION content.record_report_challenge_history(p_actors bytea[],p_times timestamptz[],p_clear boolean)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_limit integer; v_count integer; v_new integer; v_complete boolean; v_now timestamptz;
BEGIN
 IF p_actors IS NULL OR p_times IS NULL OR p_clear IS NULL OR cardinality(p_actors)>10000
    OR cardinality(p_actors)<>cardinality(p_times)
    OR coalesce(array_ndims(p_actors),1)<>1 OR coalesce(array_ndims(p_times),1)<>1
    OR EXISTS(SELECT 1 FROM unnest(p_actors,p_times) n(actor,event_at)
      WHERE actor IS NULL OR octet_length(actor)<>32 OR event_at IS NULL OR NOT isfinite(event_at)) THEN
  RAISE EXCEPTION 'Invalid report challenge history.' USING ERRCODE='23514'; END IF;
 IF cardinality(p_actors)=0 THEN RETURN true; END IF;
 IF NOT p_clear THEN
  IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)
      IN('board_public','board_staff') THEN
   -- Runtime report insertion has already acquired the restriction gate.
   PERFORM content.observe_report_challenge_epoch();
  ELSE
   -- Bulk maintenance has no restriction gate. It can invalidate continuity,
   -- but must not install an epoch using a racing restriction snapshot.
   PERFORM content.invalidate_report_challenge_history((SELECT max(t) FROM unnest(p_times) events(t)));
  END IF;
 END IF;
 -- Report admission or group clear holds board(s) -> report gate first.
 -- This policy lock serializes history recording and any incomplete watermark.
 SELECT report_history_limit INTO v_limit FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
 v_now:=clock_timestamp();
 DELETE FROM verification_private.report_history h WHERE h.actor_hash IN(
  SELECT old.actor_hash FROM verification_private.report_history old WHERE old.expires_at<=v_now
  ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED);
 SELECT count(*) INTO v_count FROM (SELECT 1 FROM verification_private.report_history LIMIT v_limit) bounded;
 SELECT count(DISTINCT n.actor) INTO v_new FROM unnest(p_actors,p_times) n(actor,event_at)
 WHERE n.event_at>v_now-CASE WHEN p_clear THEN interval '72 hours' ELSE interval '1 hour' END
  AND NOT EXISTS(SELECT 1 FROM verification_private.report_history h WHERE h.actor_hash=n.actor);
 v_complete:=v_new<=greatest(0,v_limit-v_count);
 IF NOT v_complete THEN
  -- Never fail a valid CAPTCHA-backed report or moderator clear solely because
  -- this optional-exemption history is full. Suppress exemption until omitted
  -- events age out. Existing actor rows can still be updated at full capacity.
  UPDATE verification_private.policy SET report_history_complete_after=greatest(report_history_complete_after,v_now+interval '72 hours',(SELECT max(t) FROM unnest(p_times) events(t))+interval '72 hours') WHERE singleton;
 END IF;
 WITH incoming AS (
  SELECT n.actor,array_agg(n.event_at ORDER BY n.event_at DESC) times,max(n.event_at) latest
  FROM unnest(p_actors,p_times) n(actor,event_at)
  WHERE n.event_at>v_now-CASE WHEN p_clear THEN interval '72 hours' ELSE interval '1 hour' END GROUP BY n.actor
 ), merged AS (
  SELECT i.actor,
   CASE WHEN p_clear THEN coalesce(h.last_reports,'{}'::timestamptz[])
    ELSE ARRAY(SELECT event_at FROM unnest(coalesce(h.last_reports,'{}'::timestamptz[])||i.times) events(event_at)
      ORDER BY event_at DESC LIMIT 3) END reports,
   CASE WHEN p_clear THEN greatest(h.last_cleared_at,i.latest) ELSE h.last_cleared_at END cleared
  FROM incoming i LEFT JOIN verification_private.report_history h ON h.actor_hash=i.actor
  WHERE v_complete OR h.actor_hash IS NOT NULL
 ) INSERT INTO verification_private.report_history(actor_hash,last_reports,last_cleared_at,expires_at)
 SELECT actor,reports,cleared,greatest((SELECT max(t) FROM unnest(reports) events(t))+interval '1 hour',cleared+interval '72 hours') FROM merged
 ON CONFLICT(actor_hash) DO UPDATE SET last_reports=EXCLUDED.last_reports,last_cleared_at=EXCLUDED.last_cleared_at,expires_at=EXCLUDED.expires_at;
 RETURN v_complete;
END $$;
REVOKE ALL ON FUNCTION content.record_report_challenge_history(bytea[],timestamptz[],boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.record_report_challenge_history(bytea[],timestamptz[],boolean) TO board_report_admission_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
CREATE OR REPLACE FUNCTION post_secrets.capture_report_challenge_history() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE actors bytea[]; times timestamptz[]; latest timestamptz; finite boolean;
BEGIN
 IF TG_TABLE_SCHEMA<>'post_secrets' OR TG_TABLE_NAME<>'report_membership' OR TG_OP<>'INSERT'
    OR TG_LEVEL<>'STATEMENT' OR TG_WHEN<>'AFTER' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid report history trigger.' USING ERRCODE='23514'; END IF;
 SELECT array_agg(n.actor_hash ORDER BY n.report_id),array_agg(n.reported_at ORDER BY n.report_id) INTO actors,times
 FROM (SELECT actor_hash,reported_at,report_id FROM report_challenge_inserted ORDER BY report_id LIMIT 10001) n;
 IF actors IS NULL THEN RETURN NULL; END IF;
 IF cardinality(actors)>10000 THEN
  -- Preserve the existing operator bulk-insert contract. Recording is bounded;
  -- an oversized batch instead disables optional exemption until its events
  -- are outside every report/clear window. The reports still commit normally.
  SELECT max(reported_at),bool_and(isfinite(reported_at)) INTO latest,finite FROM report_challenge_inserted;
  IF finite IS DISTINCT FROM true THEN RAISE EXCEPTION 'Invalid report history time.' USING ERRCODE='23514'; END IF;
  PERFORM content.invalidate_report_challenge_history(latest);
  RETURN NULL;
 END IF;
 PERFORM content.record_report_challenge_history(actors,times,false);
 -- The alphabetically earlier group trigger has already stamped inheritance.
 -- A new report against a cleared group logs a new clear event now, rather
 -- than reusing the group's earlier clear timestamp.
 SELECT array_agg(n.actor_hash ORDER BY n.report_id),array_agg(n.reported_at ORDER BY n.report_id) INTO actors,times
 FROM report_challenge_inserted n JOIN content.reports r ON r.id=n.report_id
 WHERE r.group_clear_inherited IS TRUE;
 IF actors IS NOT NULL THEN PERFORM content.record_report_challenge_history(actors,times,true); END IF;
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION post_secrets.capture_report_challenge_history() FROM PUBLIC;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_report_admission_owner;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION content.report_challenge_history_ready() RETURNS boolean
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT content.restrictions_ready() AND EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='post_secrets.report_membership'::regclass
  AND t.tgname='zz_report_challenge_history' AND t.tgfoid='post_secrets.capture_report_challenge_history()'::regprocedure
  AND t.tgenabled='O' AND t.tgtype=4 AND NOT t.tgisinternal AND t.tgnargs=0
  AND t.tgnewtable='report_challenge_inserted' AND t.tgoldtable IS NULL AND t.tgqual IS NULL)
 -- Inherited-clear capture depends on this exact earlier statement trigger.
 AND EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='post_secrets.report_membership'::regclass
  AND t.tgname='report_membership_group_insert' AND t.tgname<'zz_report_challenge_history'
  AND t.tgfoid='post_secrets.increment_report_group()'::regprocedure
  AND t.tgenabled='O' AND t.tgtype=4 AND NOT t.tgisinternal AND t.tgnargs=0
  AND t.tgnewtable='report_group_inserted' AND t.tgoldtable IS NULL AND t.tgqual IS NULL)
 AND EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='post_secrets.increment_report_group()'::regprocedure
  AND p.proowner='board_report_admission_owner'::regrole AND p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner))
 AND EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='post_secrets.capture_report_challenge_history()'::regprocedure
  AND p.proowner='board_report_admission_owner'::regrole AND p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner))
 AND EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='content.record_report_challenge_history(bytea[],timestamp with time zone[],boolean)'::regprocedure
  AND p.proowner='board_anonymous_owner'::regrole AND p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND has_function_privilege('board_report_admission_owner',p.oid,'EXECUTE')
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE a.privilege_type='EXECUTE' AND a.grantee NOT IN(p.proowner,'board_report_admission_owner'::regrole)))
 AND EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='content.invalidate_report_challenge_history(timestamptz)'::regprocedure
  AND p.proowner='board_anonymous_owner'::regrole AND p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND has_function_privilege('board_report_admission_owner',p.oid,'EXECUTE')
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE a.privilege_type='EXECUTE' AND a.grantee NOT IN(p.proowner,'board_report_admission_owner'::regrole)))
 AND EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='verification_private.observe_report_epoch(bytea)'::regprocedure
  AND p.proowner='board_anonymous_owner'::regrole AND NOT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner))
 AND EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='content.observe_report_challenge_epoch()'::regprocedure
  AND p.proowner='board_anonymous_owner'::regrole AND p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND has_function_privilege('board_report_admission_owner',p.oid,'EXECUTE')
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE a.privilege_type='EXECUTE' AND a.grantee NOT IN(p.proowner,'board_report_admission_owner'::regrole)))
 AND EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='content.report_restriction_history_horizon()'::regprocedure
  AND p.proowner='board_restriction_owner'::regrole AND p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND has_function_privilege('board_anonymous_owner',p.oid,'EXECUTE')
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE a.privilege_type='EXECUTE' AND a.grantee NOT IN(p.proowner,'board_anonymous_owner'::regrole)))
 AND NOT EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_verifier','board_mailer']) runtime(name)
  WHERE has_table_privilege(runtime.name,'verification_private.report_history','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'))
$$;
REVOKE ALL ON FUNCTION content.report_challenge_history_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.report_challenge_history_ready() TO board_public,board_verifier;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
SET ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report challenges did not preserve restriction authority'; END IF; END $$;
RESET ROLE;
SET ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report challenges did not preserve snapshot authority'; END IF; END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report challenges did not preserve transfer authority'; END IF; END $$;
RESET ROLE;
