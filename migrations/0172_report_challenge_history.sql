-- Retain the source report/clear windows independently of report retirement.
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
DO $$ BEGIN IF (SELECT encode(sha256(convert_to(replace(prosrc,chr(13),''),'UTF8')),'hex') FROM pg_proc WHERE oid='content.clear_report_group(text,bigint,bigint)'::regprocedure) IS DISTINCT FROM '74ee6813a62ecceb6f829f7846a83df63bd8c33f161b8e86401038620faa41b6' THEN RAISE EXCEPTION 'Report group clear changed before history capture'; END IF; END $$;
ALTER TABLE verification_private.policy
 ADD COLUMN report_history_limit integer NOT NULL DEFAULT 100000 CHECK(report_history_limit BETWEEN 1 AND 1000000),
 ADD COLUMN report_history_complete_after timestamptz NOT NULL DEFAULT (clock_timestamp()+interval '72 hours');
CREATE TABLE verification_private.report_history (
 actor_hash bytea PRIMARY KEY CHECK(octet_length(actor_hash)=32),
 last_reports timestamptz[] NOT NULL DEFAULT '{}' CHECK(cardinality(last_reports)<=3 AND array_position(last_reports,NULL) IS NULL),
 last_cleared_at timestamptz,
 expires_at timestamptz NOT NULL CHECK(isfinite(expires_at))
);
CREATE INDEX verification_report_history_expiry ON verification_private.report_history(expires_at);
REVOKE ALL ON verification_private.report_history FROM PUBLIC;
GRANT SELECT,INSERT,UPDATE,DELETE ON verification_private.report_history TO board_anonymous_owner;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION content.invalidate_report_challenge_history(p_latest timestamptz) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF p_latest IS NULL OR NOT isfinite(p_latest) THEN RAISE EXCEPTION 'Invalid report history time.' USING ERRCODE='23514'; END IF;
 UPDATE verification_private.policy SET report_history_complete_after=greatest(report_history_complete_after,clock_timestamp()+interval '72 hours',p_latest+interval '72 hours') WHERE singleton;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
END $$;
REVOKE ALL ON FUNCTION content.invalidate_report_challenge_history(timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.invalidate_report_challenge_history(timestamptz) TO board_report_admission_owner;
CREATE FUNCTION content.record_report_challenge_history(p_actors bytea[],p_times timestamptz[],p_clear boolean)
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
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA content,post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
CREATE FUNCTION post_secrets.capture_report_challenge_history() RETURNS trigger
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
CREATE TRIGGER zz_report_challenge_history AFTER INSERT ON post_secrets.report_membership
 REFERENCING NEW TABLE AS report_challenge_inserted
 FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.capture_report_challenge_history();
CREATE OR REPLACE FUNCTION content.clear_report_group(p_board text,p_post bigint,p_account bigint) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_invoker text;
    v_ids bigint[];
    v_cleared_at timestamptz;
    v_cleared_by bigint;
    v_now timestamptz;
    v_count bigint;
    v_updated bigint;
    v_known bigint;
    v_weight double precision;
    v_consistent boolean;
    v_actors bytea[]; v_times timestamptz[];
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    IF v_invoker<>'board_staff' THEN
        RAISE EXCEPTION 'Report group clear is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report group clear requires Read Committed.' USING ERRCODE='22023';
    END IF;
    IF p_board IS NULL OR p_board='' OR p_post IS NULL OR p_post<=0
        OR p_account IS NULL OR p_account<=0 THEN
        RAISE EXCEPTION 'Invalid report group clear target.' USING ERRCODE='22023';
    END IF;
    -- Caller starts a fresh content transaction and verifies board-scoped
    -- janitor-or-higher authority. Only the authenticated server supplies the
    -- actor. Lock the target board, then the shared admission gate, before
    -- group/report tuples. History recording needs no session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE;
    IF NOT FOUND THEN RETURN NULL; END IF;
    PERFORM singleton FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    -- Fresh RC statements after the wait include committed admission, reporter
    -- purge, and archive/deletion retirement. Never acquire another board here.
    SELECT g.cleared_at,g.cleared_by INTO v_cleared_at,v_cleared_by
        FROM post_secrets.report_group g WHERE g.board=p_board AND g.post_id=p_post
        FOR UPDATE;
    IF NOT FOUND THEN RETURN NULL; END IF;
    SELECT array_agg(m.report_id ORDER BY m.report_id) INTO v_ids FROM (
        SELECT member.report_id FROM post_secrets.report_membership member
        WHERE member.board=p_board AND member.post_id=p_post
        ORDER BY member.report_id LIMIT 10001
    ) m;
    IF v_ids IS NULL THEN RETURN NULL; END IF;
    v_count:=cardinality(v_ids);
    IF v_count>10000 THEN
        RAISE EXCEPTION 'Report group clear limit exceeded.' USING ERRCODE='54000';
    END IF;
    SELECT count(*),bool_and(r.board=p_board AND r.post_id=p_post
            AND r.reporter_cleared_at IS NULL
            AND r.group_cleared_at IS NOT DISTINCT FROM v_cleared_at
            AND r.group_cleared_by IS NOT DISTINCT FROM v_cleared_by
            AND ((v_cleared_at IS NULL AND r.group_clear_inherited IS NULL)
                OR (v_cleared_at IS NOT NULL AND r.group_clear_inherited IS NOT NULL)))
        INTO v_updated,v_consistent FROM content.reports r WHERE r.id=ANY(v_ids);
    IF v_updated IS DISTINCT FROM v_count OR v_consistent IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Report group clear evidence is inconsistent.' USING ERRCODE='23514';
    END IF;
    -- Idempotent retry: unknown-weight admissions may have inherited this
    -- already-proven clear. Never audit or clear them for a second time.
    IF v_cleared_at IS NOT NULL THEN RETURN 0; END IF;
    SELECT count(e.effective_weight),sum(e.effective_weight)
        INTO v_known,v_weight FROM post_secrets.report_weight_evidence e
        WHERE e.report_id=ANY(v_ids);
    -- Do not substitute catalog bases, missing evidence, retired history or
    -- today's session observations. Current 0102 proofs establish only 0.5;
    -- the explicit zero/nonfinite rejection also protects future evaluators.
    IF v_known IS DISTINCT FROM v_count OR v_weight IS NULL OR v_weight=0
        OR v_weight IN ('Infinity'::double precision,'-Infinity'::double precision,'NaN'::double precision) THEN
        RAISE EXCEPTION 'Report group weight is not established for clearing.' USING ERRCODE='P0108';
    END IF;
    v_now:=clock_timestamp();
    SELECT array_agg(m.actor_hash ORDER BY m.actor_hash),array_agg(v_now ORDER BY m.actor_hash) INTO v_actors,v_times
    FROM (SELECT DISTINCT actor_hash FROM post_secrets.report_membership WHERE report_id=ANY(v_ids)) m;
    PERFORM content.record_report_challenge_history(v_actors,v_times,true);
    UPDATE content.reports r SET group_cleared_at=v_now,
        group_cleared_by=p_account,group_clear_inherited=false
        WHERE r.id=ANY(v_ids) AND r.board=p_board AND r.post_id=p_post
            AND r.reporter_cleared_at IS NULL AND r.group_cleared_at IS NULL
            AND r.group_cleared_by IS NULL AND r.group_clear_inherited IS NULL;
    GET DIAGNOSTICS v_updated=ROW_COUNT;
    IF v_updated IS DISTINCT FROM v_count THEN
        RAISE EXCEPTION 'Report group clear count changed.' USING ERRCODE='23514';
    END IF;
    UPDATE post_secrets.report_group SET cleared_at=v_now,cleared_by=p_account
        WHERE board=p_board AND post_id=p_post AND cleared_at IS NULL AND cleared_by IS NULL;
    GET DIAGNOSTICS v_updated=ROW_COUNT;
    IF v_updated<>1 THEN
        RAISE EXCEPTION 'Report group clear lifetime changed.' USING ERRCODE='23514';
    END IF;
    RETURN v_count;
END $$;
REVOKE ALL ON FUNCTION content.clear_report_group(text,bigint,bigint)
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
GRANT EXECUTE ON FUNCTION content.clear_report_group(text,bigint,bigint) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_report_admission_owner;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION content.report_challenge_history_ready() RETURNS boolean
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='post_secrets.report_membership'::regclass
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
 AND NOT EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_verifier','board_mailer']) runtime(name)
  WHERE has_table_privilege(runtime.name,'verification_private.report_history','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'))
$$;
REVOKE ALL ON FUNCTION content.report_challenge_history_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.report_challenge_history_ready() TO board_public,board_verifier;
CREATE OR REPLACE FUNCTION content.public_challenge_ready() RETURNS boolean
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT content.report_challenge_history_ready() AND coalesce((SELECT count(*)=6 AND bool_and(
  p.proowner='board_anonymous_owner'::regrole
  AND p.prosecdef=(p.proname IN('check_public_post_challenge','check_post_challenge_completion'))
  AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner))
  FROM pg_proc p WHERE p.oid IN(
   to_regprocedure('content.check_public_post_challenge()'),
   to_regprocedure('content.check_post_challenge_completion()'),
   to_regprocedure('verification_private.finish_post_challenge(bigint,bytea,bytea,bytea,bytea,bigint)'),
   to_regprocedure('verification_private.take_write_receipt(bytea,bytea,bytea,text,text,bigint,bytea)'),
   to_regprocedure('verification_private.credit_profile(bytea,bytea,bytea,bytea,bigint)'),
   to_regprocedure('verification_private.take_write_receipt_data(bytea,bytea,bytea,text,text,bigint,bytea)'))),false)
 AND EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid=to_regprocedure('content.consume_report_challenge(text,bigint,bytea,bytea,bytea,bytea,bigint)')
  AND p.proowner='board_anonymous_owner'::regrole AND p.prosecdef
  AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND has_function_privilege('board_report_admission_owner',p.oid,'EXECUTE')
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE a.privilege_type='EXECUTE' AND a.grantee NOT IN(p.proowner,'board_report_admission_owner'::regrole)))
 AND EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='content.posts'::regclass
  AND t.tgname='a00_public_post_challenge' AND t.tgfoid='content.check_public_post_challenge()'::regprocedure
  AND t.tgenabled='O' AND t.tgtype=7 AND NOT t.tgdeferrable AND NOT t.tgisinternal
  AND t.tgnargs=0 AND t.tgqual IS NULL)
 AND EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='content.posts'::regclass
  AND t.tgname='post_challenge_completion' AND t.tgfoid='content.check_post_challenge_completion()'::regprocedure
  AND t.tgenabled='O' AND t.tgtype=5 AND t.tgdeferrable AND t.tginitdeferred AND NOT t.tgisinternal
  AND t.tgnargs=0 AND t.tgqual IS NULL)
 AND NOT EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_verifier','board_mailer']) runtime(role_name)
  WHERE pg_has_role(runtime.role_name,'board_anonymous_owner','MEMBER')
   OR has_schema_privilege(runtime.role_name,'verification_private','USAGE')
   OR has_table_privilege(runtime.role_name,'verification_private.post_handoffs','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'))
$$;
REVOKE ALL ON FUNCTION content.public_challenge_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.public_challenge_ready() TO board_public,board_verifier;
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
