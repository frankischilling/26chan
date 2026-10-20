-- Retain per-report clear history independently of membership retirement.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before report clear history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before report clear history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before report clear history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before report clear history'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before report clear history'; END IF; END $$;
GRANT CREATE ON SCHEMA verification_private,content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE TABLE verification_private.pass_report_history_policy (
 singleton boolean PRIMARY KEY CHECK(singleton),
 event_limit integer NOT NULL DEFAULT 100000 CHECK(event_limit BETWEEN 1 AND 1000000),
 complete_after timestamptz NOT NULL DEFAULT (clock_timestamp()+interval '5 days') CHECK(isfinite(complete_after))
);
INSERT INTO verification_private.pass_report_history_policy(singleton) VALUES(true);
-- No report FK: erasing a reported post must not erase its clear history.
CREATE TABLE verification_private.pass_report_clear_events (
 report_id bigint PRIMARY KEY CHECK(report_id>0),
 actor_hash bytea NOT NULL CHECK(octet_length(actor_hash)=32),
 automatic_identity uuid,
 identity_hash bytea CHECK(identity_hash IS NULL OR octet_length(identity_hash)=32),
 created_at timestamptz NOT NULL CHECK(isfinite(created_at))
);
CREATE INDEX pass_report_clear_actor ON verification_private.pass_report_clear_events(actor_hash,created_at);
CREATE INDEX pass_report_clear_automatic ON verification_private.pass_report_clear_events(automatic_identity,created_at);
CREATE INDEX pass_report_clear_member ON verification_private.pass_report_clear_events(identity_hash,created_at);
CREATE INDEX pass_report_clear_expiry ON verification_private.pass_report_clear_events(created_at);
REVOKE ALL ON verification_private.pass_report_history_policy,verification_private.pass_report_clear_events FROM PUBLIC;
CREATE FUNCTION content.pass_record_report_clears(p_reports bigint[],p_actors bytea[],p_automatic uuid[],p_at timestamptz)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE maximum integer; existing bigint; incoming bigint; stamp timestamptz:=clock_timestamp();
BEGIN
 PERFORM verification_private.pass_gate();
 IF p_reports IS NULL OR p_actors IS NULL OR p_automatic IS NULL OR p_at IS NULL OR NOT isfinite(p_at)
 OR cardinality(p_reports)>10000 OR cardinality(p_reports)<>cardinality(p_actors) OR cardinality(p_reports)<>cardinality(p_automatic)
 OR coalesce(array_ndims(p_reports),1)<>1 OR coalesce(array_ndims(p_actors),1)<>1 OR coalesce(array_ndims(p_automatic),1)<>1
 OR EXISTS(SELECT 1 FROM unnest(p_reports,p_actors) n(id,actor) WHERE id IS NULL OR id<=0 OR actor IS NULL OR octet_length(actor)<>32)
 OR (SELECT count(DISTINCT id) FROM unnest(p_reports) n(id))<>cardinality(p_reports) THEN
 RAISE EXCEPTION 'Invalid report clear history' USING ERRCODE='23514'; END IF;
 IF cardinality(p_reports)=0 THEN RETURN true; END IF;
 SELECT event_limit INTO STRICT maximum FROM verification_private.pass_report_history_policy WHERE singleton FOR UPDATE;
 DELETE FROM verification_private.pass_report_clear_events WHERE report_id IN(
  SELECT report_id FROM verification_private.pass_report_clear_events WHERE created_at<stamp-interval '5 days' ORDER BY created_at LIMIT 256);
 -- Idempotent retries preserve the first clear time and cannot change identity.
 IF EXISTS(SELECT 1 FROM unnest(p_reports,p_actors,p_automatic) n(id,actor,automatic)
  JOIN verification_private.pass_report_clear_events e ON e.report_id=n.id
  LEFT JOIN verification_private.pass_reports m ON m.report_id=n.id
  WHERE e.actor_hash<>n.actor OR e.automatic_identity IS DISTINCT FROM n.automatic OR e.identity_hash IS DISTINCT FROM m.identity_hash) THEN
 RAISE EXCEPTION 'Report clear history identity changed' USING ERRCODE='23514'; END IF;
 SELECT count(*) INTO existing FROM verification_private.pass_report_clear_events;
 SELECT count(*) INTO incoming FROM unnest(p_reports) n(id)
 WHERE NOT EXISTS(SELECT 1 FROM verification_private.pass_report_clear_events e WHERE e.report_id=n.id);
 IF incoming>greatest(0,maximum-existing) THEN
  -- Preserve valid reports and clears; omitted events prevent negative proofs.
  UPDATE verification_private.pass_report_history_policy SET complete_after=greatest(complete_after,stamp+interval '5 days',p_at+interval '5 days') WHERE singleton;
  RETURN false;
 END IF;
 INSERT INTO verification_private.pass_report_clear_events(report_id,actor_hash,automatic_identity,identity_hash,created_at)
 SELECT n.id,n.actor,n.automatic,m.identity_hash,p_at FROM unnest(p_reports,p_actors,p_automatic) n(id,actor,automatic)
 LEFT JOIN verification_private.pass_reports m ON m.report_id=n.id ON CONFLICT(report_id) DO NOTHING;
 RETURN true;
END $$;
REVOKE ALL ON FUNCTION content.pass_record_report_clears(bigint[],bytea[],uuid[],timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_record_report_clears(bigint[],bytea[],uuid[],timestamptz) TO board_report_admission_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private,content FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA post_secrets,content TO board_report_admission_owner;
SET LOCAL ROLE board_report_admission_owner;
CREATE FUNCTION post_secrets.record_current_report_clear(p_report bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 -- Called after anonymous registration, so retained password equality is final.
 PERFORM content.pass_record_report_clears(ARRAY[m.report_id],ARRAY[m.actor_hash],ARRAY[m.automatic_identity],clock_timestamp())
 FROM post_secrets.report_membership m JOIN content.reports r ON r.id=m.report_id
 WHERE m.report_id=p_report AND (r.group_clear_inherited IS TRUE OR r.auto_cleared_at IS NOT NULL);
END $$;
REVOKE ALL ON FUNCTION post_secrets.record_current_report_clear(bigint) FROM PUBLIC;
CREATE OR REPLACE FUNCTION content.admit_report(p_board text,p_post bigint,p_reason text,p_actor bytea)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer; v_revision bigint;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report admission requires Read Committed.' USING ERRCODE='22023';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Report admission is unavailable.' USING ERRCODE='42501';
    END IF;
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid report actor.' USING ERRCODE='22023';
    END IF;
    IF p_reason IS NULL OR octet_length(p_reason) NOT BETWEEN 1 AND 1000 OR btrim(p_reason)='' THEN
        RAISE EXCEPTION 'Report reason must contain 1 to 1000 bytes.' USING ERRCODE='22023';
    END IF;
    -- Board -> network generation -> singleton gate. Never acquire another board after this gate.
    -- Deletion retirement never takes this gate or an anonymous-session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board
        AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator')) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    PERFORM content.lock_report_network_policy();
    SELECT membership_limit,active_catalog_revision INTO v_limit,v_revision
        FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    IF v_revision IS NOT NULL THEN
        RAISE EXCEPTION 'Free-text reporting is not active.' USING ERRCODE='P0001';
    END IF;
    -- An RC statement after every wait sees committed admissions and retirements.
    v_now:=clock_timestamp();
    v_thread:=post_secrets.report_target(p_board,p_post,v_now);
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,v_now);
    PERFORM content.assert_reporting_restrictions(p_board,p_post,p_actor);
    PERFORM post_secrets.assert_report_network(p_board,p_post,p_actor,NULL,NULL,NULL,NULL,NULL,NULL);
    -- Operator safety cap, not source quota: only this private membership is
    -- bounded. No eviction/TTL releases old duplicate protection. Lowering the
    -- cap below current occupancy blocks new reports until retirement or an
    -- operator raises the limit; retained content.reports/audit are unaffected.
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership LIMIT v_limit) members)>=v_limit THEN
        RAISE EXCEPTION 'Report admission capacity is exhausted.' USING ERRCODE='P0094';
    END IF;
    INSERT INTO content.reports(board,post_id,reason,created_at)
        VALUES(p_board,p_post,p_reason,v_now) RETURNING id INTO v_report;
    INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
        VALUES(v_report,p_actor,p_board,p_post,v_thread,v_now);
    PERFORM post_secrets.record_current_report_clear(v_report);
    RETURN v_report;
END $$;

CREATE OR REPLACE FUNCTION content.admit_report(
    p_board text,p_post bigint,p_reason text,p_actor bytea,
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_request_at bigint
)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer; v_revision bigint; v_automatic_identity uuid;
    v_known boolean; v_evaluated_at timestamptz;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report admission requires Read Committed.' USING ERRCODE='22023';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_migrator') THEN
        RAISE EXCEPTION 'Report admission is unavailable.' USING ERRCODE='42501';
    END IF;
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid report actor.' USING ERRCODE='22023';
    END IF;
    IF p_reason IS NULL OR octet_length(p_reason) NOT BETWEEN 1 AND 1000 OR btrim(p_reason)='' THEN
        RAISE EXCEPTION 'Report reason must contain 1 to 1000 bytes.' USING ERRCODE='22023';
    END IF;
    -- Board -> network generation -> singleton report gate -> anonymous session. Never acquire
    -- another board after this gate. Registration reenters this same board.
    -- Deletion retirement never takes this gate or an anonymous-session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board
        AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator')) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    PERFORM content.lock_report_network_policy();
    SELECT membership_limit,active_catalog_revision INTO v_limit,v_revision
        FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    IF v_revision IS NOT NULL THEN
        RAISE EXCEPTION 'Free-text reporting is not active.' USING ERRCODE='P0001';
    END IF;
    -- Preserve target-policy precedence over session authorization errors.
    -- Recheck below because the session wait can cross an archive expiry.
    PERFORM post_secrets.report_target(p_board,p_post,clock_timestamp());
    -- Resolve/lock without allocation or activity before capturing admission
    -- time. A wait here can cross a cooldown or an archive expiry boundary.
    SELECT a.automatic_identity INTO STRICT v_automatic_identity
        FROM post_secrets.resolve_automatic_identity(p_token,p_minted,p_request_at,true) a;
    -- CAPTCHA precedes the source's report quota/restriction checks. Keeping
    -- consumption here also restores the proof if a later admission step fails.
    PERFORM content.consume_report_challenge(p_board,p_post,p_actor,p_token,p_network,p_address,p_environment,p_request_at);
    -- An RC statement after every wait sees committed admissions and retirements.
    v_now:=clock_timestamp();
    v_thread:=post_secrets.report_target(p_board,p_post,v_now);
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,v_automatic_identity,v_now);
    PERFORM content.assert_reporting_restrictions(p_board,p_post,p_actor);
    PERFORM post_secrets.assert_report_network(p_board,p_post,p_actor,p_token,p_network,p_address,p_environment,p_minted,p_request_at);
    -- Operator safety cap, not source quota: only this private membership is
    -- bounded. No eviction/TTL releases old duplicate protection. Lowering the
    -- cap below current occupancy blocks new reports until retirement or an
    -- operator raises the limit; retained content.reports/audit are unaffected.
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership LIMIT v_limit) members)>=v_limit THEN
        RAISE EXCEPTION 'Report admission capacity is exhausted.' USING ERRCODE='P0094';
    END IF;
    -- All admission checks have passed while board -> gate -> session locks
    -- remain held. Observe resumed PRE-report state before registration can
    -- set the pending report bit, change activity, or allocate a fresh session.
    -- Registration already revalidates request freshness through 0095's
    -- resolve_automatic_identity and 0065's advance_anonymous_session. Preserve
    -- that final boundary unchanged; this observation does not extend it.
    v_known:=post_secrets.report_known_or_verified(
        p_token,p_network,p_address,p_environment,p_minted,p_request_at);
    v_evaluated_at:=clock_timestamp();
    INSERT INTO content.reports(board,post_id,reason,created_at)
        VALUES(p_board,p_post,p_reason,v_now) RETURNING id INTO v_report;
    INSERT INTO post_secrets.report_weight_evidence(
        report_id,evaluator_version,known_or_verified,effective_weight,numeric_proof,evaluated_at)
        VALUES(v_report,1,v_known,NULL,NULL,v_evaluated_at);
    INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity)
        VALUES(v_report,p_actor,p_board,p_post,v_thread,v_now,NULL);
    -- Registration requires current-transaction provenance and a NULL identity,
    -- then allocates/reuses the session UUID and stamps this membership itself.
    -- Any failure rolls the report, membership and session changes back together.
    PERFORM content.register_anonymous_report(
        p_token,p_network,p_address,p_environment,p_minted,p_board,v_report,p_request_at);
    PERFORM post_secrets.record_current_report_clear(v_report);
    RETURN v_report;
END $$;

CREATE OR REPLACE FUNCTION content.admit_categorical_report(
    p_board text,p_post bigint,p_category bigint,p_expected_revision bigint,p_actor bytea,
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_request_at bigint
)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer; v_automatic_identity uuid; v_revision bigint; v_category record;
    v_known boolean; v_evaluated_at timestamptz;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report admission requires Read Committed.' USING ERRCODE='22023';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_migrator') THEN
        RAISE EXCEPTION 'Report admission is unavailable.' USING ERRCODE='42501';
    END IF;
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid report actor.' USING ERRCODE='22023';
    END IF;
    -- Board -> network generation -> singleton report gate -> anonymous session. Never acquire
    -- another board after this gate. Registration reenters this same board.
    -- Deletion retirement never takes this gate or an anonymous-session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board
        AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator')) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    PERFORM content.lock_report_network_policy();
    SELECT membership_limit,active_catalog_revision INTO v_limit,v_revision
        FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    -- Preserve target-policy precedence over session authorization errors.
    -- Recheck below because the session wait can cross an archive expiry.
    PERFORM post_secrets.report_target(p_board,p_post,clock_timestamp());
    -- Resolve/lock without allocation or activity before capturing admission
    -- time. A wait here can cross a cooldown or an archive expiry boundary.
    SELECT a.automatic_identity INTO STRICT v_automatic_identity
        FROM post_secrets.resolve_automatic_identity(p_token,p_minted,p_request_at,true) a;
    -- CAPTCHA precedes the source's report quota/restriction checks. Keeping
    -- consumption here also restores the proof if a later admission step fails.
    PERFORM content.consume_report_challenge(p_board,p_post,p_actor,p_token,p_network,p_address,p_environment,p_request_at);
    -- An RC statement after every wait sees committed admissions and retirements.
    v_now:=clock_timestamp();
    v_thread:=post_secrets.report_target(p_board,p_post,v_now);
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,v_automatic_identity,v_now);
    PERFORM content.assert_reporting_restrictions(p_board,p_post,p_actor);
    PERFORM post_secrets.assert_report_network(p_board,p_post,p_actor,p_token,p_network,p_address,p_environment,p_minted,p_request_at);
    -- Operator safety cap, not source quota: only this private membership is
    -- bounded. No eviction/TTL releases old duplicate protection. Lowering the
    -- cap below current occupancy blocks new reports until retirement or an
    -- operator raises the limit; retained content.reports/audit are unaffected.
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership LIMIT v_limit) members)>=v_limit THEN
        RAISE EXCEPTION 'Report admission capacity is exhausted.' USING ERRCODE='P0094';
    END IF;
    -- Authoritative mode, revision and eligibility are checked after quotas,
    -- while the gate prevents activation changes until this transaction ends.
    IF v_revision IS NULL THEN
        RAISE EXCEPTION 'Categorical reporting is not active.' USING ERRCODE='P0001';
    END IF;
    IF p_expected_revision IS DISTINCT FROM v_revision THEN
        RAISE EXCEPTION 'Report categories changed. Please reload the report form.' USING ERRCODE='P0001';
    END IF;
    SELECT * INTO v_category FROM post_secrets.eligible_report_categories(p_board,p_post,v_revision)
        WHERE id=p_category;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Invalid category selected.' USING ERRCODE='P0001';
    END IF;
    -- All admission checks have passed while board -> gate -> session locks
    -- remain held. Observe resumed PRE-report state before registration can
    -- set the pending report bit, change activity, or allocate a fresh session.
    -- Registration already revalidates request freshness through 0095's
    -- resolve_automatic_identity and 0065's advance_anonymous_session. Preserve
    -- that final boundary unchanged; this observation does not extend it.
    v_known:=post_secrets.report_known_or_verified(
        p_token,p_network,p_address,p_environment,p_minted,p_request_at);
    v_evaluated_at:=clock_timestamp();
    INSERT INTO content.reports(board,post_id,reason,created_at,
        category_revision,category_id,category_kind,category_base_weight)
        VALUES(p_board,p_post,v_category.title,v_now,
            v_revision,v_category.id,v_category.kind,v_category.base_weight)
        RETURNING id INTO v_report;
    -- Source staff authority remains Unknown even for a privileged database
    -- caller. Only base==fallback proves a number across every possible path;
    -- the source reason remains Unknown even when this numeric value is known.
    INSERT INTO post_secrets.report_weight_evidence(
        report_id,evaluator_version,known_or_verified,effective_weight,numeric_proof,evaluated_at)
        VALUES(v_report,1,v_known,
            CASE WHEN v_category.base_weight=0.5 THEN 0.5::double precision ELSE NULL END,
            CASE WHEN v_category.base_weight=0.5 THEN 'BaseEqualsFallback' ELSE NULL END,
            v_evaluated_at);
    INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity)
        VALUES(v_report,p_actor,p_board,p_post,v_thread,v_now,NULL);
    -- Registration requires current-transaction provenance and a NULL identity,
    -- then allocates/reuses the session UUID and stamps this membership itself.
    -- Any failure rolls the report, membership and session changes back together.
    PERFORM content.register_anonymous_report(
        p_token,p_network,p_address,p_environment,p_minted,p_board,v_report,p_request_at);
    PERFORM post_secrets.record_current_report_clear(v_report);
    RETURN v_report;
END $$;

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
    -- Automatic clearing already removed this target from the active queue.
    -- It grants no manual-clear history credit and needs no weight proof.
    IF EXISTS(SELECT 1 FROM content.reports WHERE board=p_board AND post_id=p_post
      AND reporter_cleared_at IS NULL AND auto_cleared_at IS NOT NULL) THEN RETURN 0; END IF;
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
    PERFORM content.pass_record_report_clears(array_agg(m.report_id ORDER BY m.report_id),
      array_agg(m.actor_hash ORDER BY m.report_id),array_agg(m.automatic_identity ORDER BY m.report_id),v_now)
    FROM post_secrets.report_membership m WHERE m.report_id=ANY(v_ids);
    RETURN v_count;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets,content FROM board_report_admission_owner;
GRANT CREATE ON SCHEMA verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION verification_private.pass_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='verification_private' AND c.relname LIKE 'pass\_%' ESCAPE '\'
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE (n.nspname IN('verification_private','content') AND p.proname LIKE 'pass\_%' ESCAPE '\')
   OR (n.nspname='content' AND p.proname='public_pass_ready')
   OR p.oid='content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean)'::regprocedure
   OR p.oid='post_secrets.record_current_report_clear(bigint)'::regprocedure
   OR p.oid='post_secrets.check_report_limits(text,bigint,bytea,uuid,timestamp with time zone)'::regprocedure
   OR p.oid='content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean)'::regprocedure
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
    r.relrowsecurity,r.relforcerowsecurity,r.reloptions,
    verification_private.pass_acl(coalesce(r.relacl,acldefault(CASE WHEN r.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,r.relowner))),
    CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
      pg_get_expr(d.adbin,d.adrelid),verification_private.pass_acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated) ORDER BY k.conname)
     FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    verification_private.pass_acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
    ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(r.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)) ORDER BY r.identity,t.tgname)
    FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
   ORDER BY t.tgrelid::regclass::text,t.tgname) FROM pg_trigger t WHERE t.tgfoid IN('verification_private.pass_erase_post_identity()'::regprocedure,'verification_private.pass_capture_report_identity()'::regprocedure)),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
   ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype,t.tgconstrrelid::regclass::text)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(
    SELECT k.oid FROM pg_constraint k JOIN relations r ON r.oid=k.conrelid WHERE k.contype='f')),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
   ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype,t.tgconstrrelid::regclass::text)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(
    SELECT k.oid FROM pg_constraint k JOIN relations r ON r.oid=k.conrelid WHERE k.contype='f')),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),verification_private.pass_acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
    FROM pg_namespace n WHERE n.nspname='verification_private'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private FROM board_anonymous_owner;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_report_profile_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_report_profile_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed posting authority'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed report profile'; END IF; END $$;
