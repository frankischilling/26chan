-- Private pre-report observation only: modes/report.php:579 asks
-- isUserKnownOrVerified(60) before updateReportActivity. This matches the
-- pinned lib/userpwd.php:487-548 and anonymous-reference.json, using the
-- rewrite's server-owned Snapshot::for_request / State::resume semantics.
-- No effective weight, policy switch, admission wiring, or activity mutation.
GRANT CREATE ON SCHEMA post_secrets TO board_anonymous_owner;
SET ROLE board_anonymous_owner;

CREATE FUNCTION post_secrets.report_known_or_verified(
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_request_at bigint
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    s post_secrets.anonymous_sessions%ROWTYPE;
    v_last bigint; v_network_age bigint; v_password_age bigint;
    v_posts integer; v_reports integer;
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
        -- State::new(request_at).is_known_or_verified(request_at,60,0).
        -- Merely observing a newly minted capability must not allocate it.
        RETURN false;
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

    -- State::is_known_or_verified(request_at,60,0): verification wins before
    -- the churn guard, but only after resume may have cleared stale activity.
    IF s.verified_level>0 THEN RETURN true; END IF;
    v_network_age:=greatest(0,p_request_at-s.network_at);
    IF s.change_score>9 AND v_network_age<1800 THEN RETURN false; END IF;
    IF v_network_age>=3600 THEN RETURN true; END IF;
    v_password_age:=greatest(0,p_request_at-s.created_at);
    IF v_password_age<3600 THEN RETURN false; END IF;
    -- Existing pending activity counts; this request has NOT yet ORed in the
    -- report bit, adjusted churn, incremented counters, or renewed expiry.
    v_posts:=s.posts+CASE WHEN (s.pending & 1)<>0 THEN 1 ELSE 0 END;
    v_reports:=s.reports+CASE WHEN (s.pending & 8)<>0 THEN 1 ELSE 0 END;
    RETURN (v_posts>=3 OR v_reports>=10)
        AND (v_network_age>=1800 OR v_posts>=9 OR v_reports>=20);
END $$;

REVOKE ALL ON FUNCTION post_secrets.report_known_or_verified(bytea,bytea,bytea,bytea,boolean,bigint)
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
GRANT EXECUTE ON FUNCTION post_secrets.report_known_or_verified(bytea,bytea,bytea,bytea,boolean,bigint)
    TO board_report_admission_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_anonymous_owner;
