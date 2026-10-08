-- Forward automatic-session report admission. The private UUID is resolved
-- from a validated opaque session token, never supplied by a caller. Reports
-- do not have the thread quota's source-new exemption. No historical backfill.
-- The migrator has SET-only owner memberships, so replacements and grants are
-- made as their actual owners, with temporary schema CREATE privileges.
SET ROLE board_anonymous_owner;
GRANT EXECUTE ON FUNCTION content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)
    TO board_report_admission_owner;
RESET ROLE;

GRANT CREATE ON SCHEMA content,post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;

CREATE FUNCTION post_secrets.report_target(p_board text,p_post bigint,p_now timestamptz) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_enabled boolean; v_thread bigint; v_sticky boolean; v_capcode text;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Report admission is unavailable.' USING ERRCODE='42501';
    END IF;
    SELECT b.can_report_posts INTO v_enabled FROM content.boards b
        WHERE b.slug=p_board AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator'));
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    IF NOT v_enabled THEN
        RAISE EXCEPTION 'You cannot report posts on this board.' USING ERRCODE='P0001';
    END IF;
    SELECT p.thread_id,t.sticky,p.capcode INTO v_thread,v_sticky,v_capcode
        FROM content.posts p JOIN content.threads t ON t.board=p.board AND t.id=p.thread_id
        JOIN content.boards b ON b.slug=p.board
        WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted AND NOT t.deleted
            AND (t.archived_at IS NULL OR
                (b.archive_retention_seconds>0 AND t.archive_expires_at>p_now));
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    IF p_post=v_thread AND v_sticky THEN
        RAISE EXCEPTION 'Error: You cannot report a sticky.' USING ERRCODE='P0001';
    END IF;
    IF v_capcode IS NOT NULL THEN
        RAISE EXCEPTION 'Error: You cannot report this post.' USING ERRCODE='P0001';
    END IF;
    RETURN v_thread;
END $$;

CREATE FUNCTION post_secrets.check_report_limits(p_board text,p_post bigint,p_actor bytea,p_automatic_identity uuid,p_now timestamptz)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid report actor.' USING ERRCODE='22023';
    END IF;
    -- Source precedence and strict lower edges, with no upper timestamp edge.
    IF EXISTS(SELECT 1 FROM post_secrets.report_membership m
        WHERE (m.actor_hash=p_actor OR (p_automatic_identity IS NOT NULL
            AND m.automatic_identity=p_automatic_identity)) AND m.board=p_board AND m.post_id=p_post) THEN
        RAISE EXCEPTION 'You have already reported this post.' USING ERRCODE='P0001';
    END IF;
    IF EXISTS(SELECT 1 FROM post_secrets.report_membership m
        WHERE (m.actor_hash=p_actor OR (p_automatic_identity IS NOT NULL
            AND m.automatic_identity=p_automatic_identity)) AND m.reported_at>p_now-interval '15 seconds') THEN
        RAISE EXCEPTION 'You have to wait a while before reporting another post.' USING ERRCODE='P0001';
    END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership m
        WHERE (m.actor_hash=p_actor OR (p_automatic_identity IS NOT NULL
            AND m.automatic_identity=p_automatic_identity)) AND m.reported_at>p_now-interval '1 hour' LIMIT 30) matches)>=30 THEN
        RAISE EXCEPTION 'You have to wait a while before reporting another post.' USING ERRCODE='P0001';
    END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership m
        WHERE (m.actor_hash=p_actor OR (p_automatic_identity IS NOT NULL
            AND m.automatic_identity=p_automatic_identity)) AND m.reported_at>p_now-interval '24 hours' LIMIT 80) matches)>=80 THEN
        RAISE EXCEPTION 'You have to wait a while before reporting another post.' USING ERRCODE='P0001';
    END IF;
END $$;

-- Retain private/IP-only call contracts while sharing the current target and
-- source-limit implementation. The old public advisory wrapper stays read-only.
CREATE OR REPLACE FUNCTION post_secrets.report_target(p_board text,p_post bigint) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    RETURN post_secrets.report_target(p_board,p_post,clock_timestamp());
END $$;

CREATE OR REPLACE FUNCTION post_secrets.check_report_limits(
    p_board text,p_post bigint,p_actor bytea,p_now timestamptz
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,NULL::uuid,p_now);
END $$;

CREATE FUNCTION content.check_report_admission(
    p_board text,p_post bigint,p_actor bytea,p_token bytea,p_request_at bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_limit integer; v_automatic_identity uuid; v_now timestamptz;
BEGIN
    -- Advisory GET never takes a board/gate/session lock, allocates identity,
    -- advances activity, or writes a cookie. NULL token is allowed only here.
    SELECT a.automatic_identity INTO STRICT v_automatic_identity
        FROM post_secrets.lookup_automatic_identity(p_token,p_request_at) a;
    v_now:=clock_timestamp();
    PERFORM post_secrets.report_target(p_board,p_post,v_now);
    -- Deliberately ignore source_new: reports always use captured equality.
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,v_automatic_identity,v_now);
    SELECT membership_limit INTO v_limit FROM post_secrets.report_admission_gate WHERE singleton;
    IF v_limit IS NULL THEN
        RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094';
    END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership LIMIT v_limit) members)>=v_limit THEN
        RAISE EXCEPTION 'Report admission capacity is exhausted.' USING ERRCODE='P0094';
    END IF;
END $$;

CREATE OR REPLACE FUNCTION content.admit_report(p_board text,p_post bigint,p_reason text,p_actor bytea)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer;
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
    -- Board -> singleton gate. Never acquire another board after this gate.
    -- Deletion retirement never takes this gate or an anonymous-session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board
        AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator')) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    SELECT membership_limit INTO v_limit FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    -- An RC statement after every wait sees committed admissions and retirements.
    v_now:=clock_timestamp();
    v_thread:=post_secrets.report_target(p_board,p_post,v_now);
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,v_now);
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

CREATE FUNCTION content.admit_report(
    p_board text,p_post bigint,p_reason text,p_actor bytea,
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_request_at bigint
)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer; v_automatic_identity uuid;
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
    SELECT membership_limit INTO v_limit FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
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
    INSERT INTO content.reports(board,post_id,reason,created_at)
        VALUES(p_board,p_post,p_reason,v_now) RETURNING id INTO v_report;
    INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity)
        VALUES(v_report,p_actor,p_board,p_post,v_thread,v_now,NULL);
    -- Registration requires current-transaction provenance and a NULL identity,
    -- then allocates/reuses the session UUID and stamps this membership itself.
    -- Any failure rolls the report, membership and session changes back together.
    PERFORM content.register_anonymous_report(
        p_token,p_network,p_address,p_environment,p_minted,p_board,v_report,p_request_at);
    RETURN v_report;
END $$;

-- A direct or nested four-argument public call cannot bypass session admission:
-- both EXECUTE and its actual-invoker guard exclude board_public. Staff retain
-- the IP-only mutation; the new mutation never delegates to that wrapper.
REVOKE ALL ON FUNCTION content.admit_report(text,bigint,text,bytea) FROM PUBLIC,board_public,board_auth;
GRANT EXECUTE ON FUNCTION content.admit_report(text,bigint,text,bytea) TO board_staff;
REVOKE ALL ON FUNCTION post_secrets.report_target(text,bigint,timestamptz),
    post_secrets.check_report_limits(text,bigint,bytea,uuid,timestamptz),
    content.check_report_admission(text,bigint,bytea,bytea,bigint),
    content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)
    FROM PUBLIC,board_public,board_staff,board_auth;
GRANT EXECUTE ON FUNCTION content.check_report_admission(text,bigint,bytea,bytea,bigint)
    TO board_public,board_staff;
GRANT EXECUTE ON FUNCTION content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)
    TO board_public;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_report_admission_owner;
