-- Report network policy runs after quota and explicit-ban checks, before
-- insertion and anonymous activity registration. GET advice remains unchanged.
-- No operator rules, source deployment keys or private lists are installed.

SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
    PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
    IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Restriction catalog drifted before report network policy'; END IF;
END $$;
RESET ROLE;

GRANT CREATE ON SCHEMA post_secrets TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE FUNCTION post_secrets.report_network_activity(
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_request_at bigint
) RETURNS TABLE(created_at bigint,network_at bigint,change_score integer,post_count integer,report_count integer,verified boolean) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    s post_secrets.anonymous_sessions%ROWTYPE;
    v_last bigint;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report session observation requires Read Committed.' USING ERRCODE='22023';
    END IF;
    IF p_token IS NULL OR octet_length(p_token)<>32
        OR p_network IS NULL OR octet_length(p_network)<>32
        OR p_address IS NULL OR octet_length(p_address)<>32
        OR p_environment IS NULL OR octet_length(p_environment)<>32
        OR p_minted IS NULL OR p_request_at IS NULL OR p_request_at<=0
        OR abs(p_request_at::numeric-extract(epoch FROM clock_timestamp())::bigint)>30 THEN
        RAISE EXCEPTION 'Invalid anonymous activity context.' USING ERRCODE='23514';
    END IF;
    -- Private caller prerequisite: own board -> report gate -> session, and
    -- never acquire another board/gate after this call. Reenter the same row
    -- lock held by report admission's resolve_automatic_identity(...,true).
    -- No policy/capacity lock, UUID allocation, or raw-table grant is needed.
    SELECT a.* INTO s FROM post_secrets.anonymous_sessions a
        WHERE a.token_hash=p_token FOR UPDATE;
    IF NOT FOUND THEN
        IF NOT p_minted THEN
            RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000';
        END IF;
        -- A fresh capability has no established activity or verification.
        RETURN QUERY SELECT p_request_at,p_request_at,0,0,0,false;
        RETURN;
    END IF;
    -- Match the identity resolver's revocation/expiry contract, including
    -- actual expiry after a lock wait. Request time still drives source ages.
    IF p_minted OR s.expires_at<=p_request_at
        OR s.expires_at<=extract(epoch FROM clock_timestamp())::bigint THEN
        RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000';
    END IF;

    -- Resume only this local copy, exactly as Snapshot::for_request does.
    -- Saturating subtraction prevents future stored clocks underflowing.
    v_last:=CASE WHEN s.activity_at>0 THEN s.activity_at ELSE s.created_at END;
    IF greatest(0,p_request_at-v_last)>=604800 THEN
        s.created_at:=p_request_at; s.network_at:=p_request_at;
        s.address_at:=p_request_at; s.environment_at:=p_request_at;
        s.activity_at:=0; s.action_at:=p_request_at;
        s.verified_level:=0; s.posts:=0; s.images:=0; s.threads:=0;
        s.reports:=0; s.pending:=0; s.change_score:=0;
    ELSE
        IF s.environment_hash<>p_environment THEN s.environment_at:=p_request_at; END IF;
        IF s.network_hash<>p_network THEN
            s.network_at:=p_request_at; s.address_at:=p_request_at;
        ELSIF s.address_hash<>p_address THEN
            s.address_at:=p_request_at;
        END IF;
    END IF;

    -- Resume and observe only; report registration has not changed this state.
    RETURN QUERY SELECT s.created_at,s.network_at,s.change_score::integer,
        s.posts::integer+CASE WHEN (s.pending & 1)<>0 THEN 1 ELSE 0 END,
        s.reports::integer+CASE WHEN (s.pending & 8)<>0 THEN 1 ELSE 0 END,
        s.verified_level>0;
END $$;

REVOKE ALL ON FUNCTION post_secrets.report_network_activity(bytea,bytea,bytea,bytea,boolean,bigint)
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
GRANT EXECUTE ON FUNCTION post_secrets.report_network_activity(bytea,bytea,bytea,bytea,boolean,bigint)
    TO board_report_admission_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_anonymous_owner;

-- The report admission function has already locked its board. This helper
-- takes only the shared generation row; it never takes a second board/peer.
GRANT CREATE ON SCHEMA content TO board_admission_owner;
SET LOCAL ROLE board_admission_owner;
CREATE FUNCTION content.lock_report_network_policy() RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    PERFORM singleton FROM admission.policy WHERE singleton FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Report network policy is unavailable.' USING ERRCODE='P0179';
    END IF;
END $$;
REVOKE ALL ON FUNCTION content.lock_report_network_policy() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.lock_report_network_policy() TO board_report_admission_owner;
CREATE FUNCTION content.report_network_rules(p_board text,p_now bigint,p_work_safe boolean)
RETURNS TABLE(id bigint,range_start bigint,range_end bigint,asn bigint,active boolean,
    created_at bigint,updated_at bigint,expires_at bigint,boards text,ops_only boolean,
    images_only boolean,reports_only boolean,leniency smallint,signatures text)
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
    SELECT r.id,r.range_start,r.range_end,r.asn,r.active,r.created_at,r.updated_at,r.expires_at,
        r.boards,r.ops_only,r.images_only,r.reports_only,r.leniency,r.signatures
    FROM admission.network_rules r
    WHERE r.active AND p_now>=0 AND (r.expires_at=0 OR r.expires_at>p_now) AND (r.boards IN ('','0') OR p_board=ANY(string_to_array(r.boards,','))
        OR (p_work_safe AND '_ws_'=ANY(string_to_array(r.boards,','))))
    ORDER BY r.leniency,r.position,r.id LIMIT 129
$$;
REVOKE ALL ON FUNCTION content.report_network_rules(text,bigint,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.report_network_rules(text,bigint,boolean) TO board_report_admission_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_admission_owner;

GRANT CREATE ON SCHEMA post_secrets TO board_report_admission_owner;
SET LOCAL ROLE board_report_admission_owner;
CREATE FUNCTION post_secrets.assert_report_network(
    p_board text,p_post bigint,p_actor bytea,p_token bytea,p_network bytea,
    p_address bytea,p_environment bytea,p_minted boolean,p_activity_at bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    raw text; ctx jsonb; r record; activity record; have_activity boolean:=false;
    work_safe boolean; request_at bigint:=0; ipv4 bigint; asn bigint; peer_known boolean; asn_known boolean;
    browser_hint text; request_hint text; selected boolean; unknown boolean;
    since_at bigint; network_age bigint; known boolean;
BEGIN
    -- The report owner already has private-board visibility and worksafe read.
    -- The narrow policy owner must not join boards through its public-only RLS.
    SELECT b.worksafe INTO work_safe FROM content.boards b WHERE b.slug=p_board;
    IF NOT FOUND OR work_safe IS NULL THEN
        RAISE EXCEPTION 'Report board policy is unavailable.' USING ERRCODE='P0179'; END IF;
    raw:=current_setting('board.report_network_context',true);
    -- Request-bound comparison data, never CAPTCHA, identity or Pass authority.
    -- Consume the context once even if there are no configured rules.
    PERFORM set_config('board.report_network_context','',true);
    IF raw IS NOT NULL AND raw<>'' THEN
        IF octet_length(raw)>2048 THEN
            RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179';
        END IF;
        BEGIN
            ctx:=raw::jsonb;
            IF jsonb_typeof(ctx) IS DISTINCT FROM 'object' THEN
                RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
            IF (SELECT count(*) FROM jsonb_object_keys(ctx))<>10
                OR ctx->>'board' IS DISTINCT FROM p_board
                OR ctx->>'post' IS DISTINCT FROM p_post::text
                OR ctx->>'actor' IS DISTINCT FROM encode(p_actor,'hex')
                OR jsonb_typeof(ctx->'request_at') IS DISTINCT FROM 'number'
                OR (ctx->>'request_at') !~ '^[0-9]{1,18}$'
                OR jsonb_typeof(ctx->'peer_known') IS DISTINCT FROM 'boolean'
                OR jsonb_typeof(ctx->'asn_known') IS DISTINCT FROM 'boolean'
                OR NOT (ctx ?& ARRAY['ipv4','asn','browser','request']) THEN
                RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179';
            END IF;
            request_at:=(ctx->>'request_at')::bigint;
            peer_known:=(ctx->>'peer_known')::boolean;
            asn_known:=(ctx->>'asn_known')::boolean;
            IF ctx->'ipv4'<>'null'::jsonb THEN
                IF jsonb_typeof(ctx->'ipv4')<>'number' OR (ctx->>'ipv4') !~ '^[0-9]{1,10}$' THEN
                    RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
                ipv4:=(ctx->>'ipv4')::bigint;
                IF NOT peer_known OR ipv4>4294967295 THEN
                    RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
            END IF;
            IF ctx->'asn'<>'null'::jsonb THEN
                IF jsonb_typeof(ctx->'asn')<>'number' OR (ctx->>'asn') !~ '^[0-9]{1,10}$' THEN
                    RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
                asn:=(ctx->>'asn')::bigint;
                IF NOT asn_known OR asn NOT BETWEEN 1 AND 4294967295 THEN
                    RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
            END IF;
            IF ctx->'browser'<>'null'::jsonb AND jsonb_typeof(ctx->'browser')<>'string'
                OR ctx->'request'<>'null'::jsonb AND jsonb_typeof(ctx->'request')<>'string' THEN
                RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
            browser_hint:=ctx->>'browser'; request_hint:=ctx->>'request';
            IF octet_length(browser_hint)>128 OR browser_hint ~ '[[:cntrl:]]'
                OR octet_length(request_hint)>128 OR request_hint ~ '[[:cntrl:]]' THEN
                RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
        EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN
            RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179';
        END;
    END IF;
    -- Missing context uses time zero to conservatively observe all active rules.
    -- Empty policy and posting-only rules retain the compatibility entry points.
    IF (SELECT count(*) FROM content.report_network_rules(p_board,request_at,work_safe))>128 THEN
        RAISE EXCEPTION 'Report network policy is unavailable.' USING ERRCODE='P0179'; END IF;
    FOR r IN SELECT * FROM content.report_network_rules(p_board,request_at,work_safe) LOOP
        IF r.ops_only OR r.images_only THEN CONTINUE; END IF;
        IF ctx IS NULL THEN RAISE EXCEPTION 'Report network context is unavailable.' USING ERRCODE='P0179'; END IF;
        selected:=false; unknown:=false;
        IF r.range_start IS NOT NULL THEN
            IF NOT peer_known THEN unknown:=true;
            ELSIF ipv4 BETWEEN r.range_start AND r.range_end THEN selected:=true;
            END IF;
        END IF;
        IF r.asn IS NOT NULL THEN
            IF NOT asn_known THEN unknown:=true;
            ELSIF asn=r.asn THEN selected:=true;
            END IF;
        END IF;
        IF NOT selected THEN
            IF unknown THEN RAISE EXCEPTION 'Report network provider is unavailable.' USING ERRCODE='P0179'; END IF;
            CONTINUE;
        END IF;
        IF r.signatures NOT IN ('','0') THEN
            selected:=(browser_hint IS NOT NULL AND browser_hint NOT IN ('','0') AND strpos(r.signatures,browser_hint)>0)
                OR (request_hint IS NOT NULL AND request_hint NOT IN ('','0') AND strpos(r.signatures,request_hint)>0);
            IF NOT selected THEN
                IF browser_hint IS NULL OR request_hint IS NULL THEN
                    RAISE EXCEPTION 'Report network provider is unavailable.' USING ERRCODE='P0179'; END IF;
                CONTINUE;
            END IF;
        END IF;
        IF r.leniency<>0 AND p_token IS NOT NULL THEN
            IF NOT have_activity THEN
                SELECT * INTO STRICT activity FROM post_secrets.report_network_activity(
                    p_token,p_network,p_address,p_environment,p_minted,p_activity_at);
                have_activity:=true;
            END IF;
            since_at:=CASE WHEN r.updated_at<>0 THEN r.updated_at ELSE r.created_at END;
            network_age:=greatest(0,p_activity_at-activity.network_at);
            known:=NOT (activity.change_score>9 AND network_age<1800) AND (
                network_age>=86400
                OR (since_at>0 AND activity.network_at<=since_at AND (activity.post_count>0 OR activity.report_count>5))
                OR (greatest(0,p_activity_at-activity.created_at)>=86400 AND (
                    since_at>0 OR ((activity.post_count>=3 OR activity.report_count>=10)
                        AND (network_age>=1800 OR activity.post_count>=9 OR activity.report_count>=20)))));
            IF (r.leniency=1 AND (activity.verified OR known))
                OR (r.leniency=2 AND known) OR (r.leniency=3 AND activity.verified) THEN CONTINUE; END IF;
        END IF;
        RAISE EXCEPTION 'Reporting from this IP range has been blocked due to abuse.' USING ERRCODE='P179B';
    END LOOP;
END $$;
REVOKE ALL ON FUNCTION post_secrets.assert_report_network(text,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_report_admission_owner;

GRANT CREATE ON SCHEMA content TO board_report_admission_owner;
SET LOCAL ROLE board_report_admission_owner;

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
    RETURN v_report;
END $$;

RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_report_admission_owner;
GRANT CREATE ON SCHEMA restrictions TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE OR REPLACE FUNCTION restrictions.catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='restrictions'
 ), triggers AS (
  SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'post_secrets.posting_history'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='post_secrets.posting_history'::regclass AND t.tgname='source_restriction_guard' AND NOT t.tgisinternal
  UNION ALL
  SELECT t.*,'content.reports'::text AS identity FROM pg_trigger t
   WHERE t.tgrelid='content.reports'::regclass AND t.tgname='source_report_restriction_guard' AND NOT t.tgisinternal
 ), funcs AS (
  SELECT p.* FROM pg_proc p WHERE p.proowner='board_restriction_owner'::regrole
    OR p.oid IN(SELECT t.tgfoid FROM triggers t)
    OR p.oid IN('content.lock_report_network_policy()'::regprocedure,
       'content.report_network_rules(text,bigint,boolean)'::regprocedure,
       'post_secrets.report_network_activity(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
       'post_secrets.assert_report_network(text,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
    r.relrowsecurity,r.relforcerowsecurity,r.reloptions,restrictions.acl(coalesce(r.relacl,acldefault('r',r.relowner))),
    CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
       pg_get_expr(d.adbin,d.adrelid),restrictions.acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated)
       ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    restrictions.acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
    ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(t.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
    ORDER BY t.identity,t.tgname) FROM triggers t),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),pg_get_functiondef(p.oid),
      has_function_privilege('board_restriction_owner',p.oid,'EXECUTE')) ORDER BY p.oid::regprocedure::text)
   FROM pg_proc p WHERE p.oid IN('staff_identity.has_board_access(bigint,text)'::regprocedure,
      'content.lock_posting_actor(bytea,boolean)'::regprocedure,
      'post_secrets.lookup_automatic_identity(bytea,bigint)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea)'::regprocedure,
      'content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
      'content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),restrictions.acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
   FROM pg_namespace n WHERE n.nspname='restrictions'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA restrictions FROM board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN
    IF content.restrictions_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Report network policy did not preserve restriction authority'; END IF;
END $$;
RESET ROLE;
