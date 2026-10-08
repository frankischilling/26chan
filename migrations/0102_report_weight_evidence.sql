-- Private, immutable admission-time evidence only. Unknown source authority,
-- threat and history stay NULL; neither runtime roles nor request metadata
-- establish them. This is not weight enforcement or a new admission policy.
-- Historical reports and the legacy IP-only free-text path remain Unknown
-- without evidence rows. No backfill, update API, session snapshot, or headers.
GRANT CREATE ON SCHEMA content,post_secrets TO board_report_admission_owner;
-- REFERENCES is needed only while creating the evidence FK, not at runtime.
GRANT REFERENCES(id) ON content.reports TO board_report_admission_owner;
SET ROLE board_report_admission_owner;

CREATE TABLE post_secrets.report_weight_evidence (
    report_id bigint PRIMARY KEY REFERENCES content.reports(id) ON DELETE CASCADE,
    evaluator_version smallint NOT NULL CHECK(evaluator_version=1),
    known_or_verified boolean,
    authenticated_janitor_or_higher boolean CHECK(authenticated_janitor_or_higher IS NULL),
    threat_at_least_point_four boolean CHECK(threat_at_least_point_four IS NULL),
    history_filtered boolean CHECK(history_filtered IS NULL),
    effective_weight double precision CHECK(effective_weight NOT IN
        ('Infinity'::double precision,'-Infinity'::double precision,'NaN'::double precision)),
    numeric_proof text,
    source_reason text CHECK(source_reason IS NULL),
    evaluated_at timestamptz NOT NULL,
    CONSTRAINT report_weight_evidence_numeric_pair_check CHECK (
        (effective_weight IS NULL AND numeric_proof IS NULL)
        OR (effective_weight IS NOT NULL AND numeric_proof IS NOT NULL
            AND effective_weight=0.5 AND numeric_proof='BaseEqualsFallback'))
);
-- Evidence survives membership/group retirement because reports are retained.
-- Physical owner deletion of a report retains its prior behavior via CASCADE.
-- Only the private owner can maintain this table; no runtime read/write/API.
REVOKE ALL ON post_secrets.report_weight_evidence
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
RESET ROLE;
REVOKE REFERENCES(id) ON content.reports FROM board_report_admission_owner;
SET ROLE board_report_admission_owner;

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
    -- Board -> singleton report gate -> anonymous session. Never acquire
    -- another board after this gate. Registration reenters this same board.
    -- Deletion retirement never takes this gate or an anonymous-session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board
        AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator')) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
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
    -- An RC statement after every wait sees committed admissions and retirements.
    v_now:=clock_timestamp();
    v_thread:=post_secrets.report_target(p_board,p_post,v_now);
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,v_automatic_identity,v_now);
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
    -- Board -> singleton report gate -> anonymous session. Never acquire
    -- another board after this gate. Registration reenters this same board.
    -- Deletion retirement never takes this gate or an anonymous-session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board
        AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator')) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
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
    -- An RC statement after every wait sees committed admissions and retirements.
    v_now:=clock_timestamp();
    v_thread:=post_secrets.report_target(p_board,p_post,v_now);
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,v_automatic_identity,v_now);
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
    RETURN v_report;
END $$;

-- CREATE OR REPLACE retains both functions' owners, ACLs, invoker guards and
-- signatures. The existing private 0101 helper grant is sufficient; no raw
-- anonymous-session or extra runtime privileges are added. Any failed helper,
-- report, evidence, membership/group, or registration step rolls back together.
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_report_admission_owner;
