-- Forward automatic-session branch of the active OP quota. Keep 0092's
-- IP-only overload for staff/internal callers without a public session.
-- The private UUID is server-allocated evidence, not a browser token or a
-- submitted deletion/recovery password. Existing rows are never backfilled.
CREATE FUNCTION content.check_user_thread_quota(
    p_actor bytea,p_board text,p_request_at bigint,
    p_token bytea,p_minted boolean,p_session_request_at bigint
) RETURNS TABLE(rejected boolean,user_thread_limit integer,user_thread_period_hours integer)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_invoker text; v_limit integer; v_period integer; v_count bigint;
    v_automatic_identity uuid; v_source_new boolean;
BEGIN
    IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_request_at IS NULL
        OR p_request_at NOT BETWEEN 0 AND 9223372036854689407 THEN
        RAISE EXCEPTION 'Invalid thread quota context.' USING ERRCODE='23514';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Thread quota is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Thread quota requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Preserve 0092's board visibility and quota serialization. The caller
    -- holds actor -> OP capacity -> board -> content admission -> session;
    -- both the board and session locks below are reentries, not new gates.
    SELECT b.user_thread_limit,b.user_thread_period_hours INTO v_limit,v_period
        FROM content.boards b WHERE b.slug=p_board
            AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator'))
        FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002'; END IF;

    -- Read the pre-registration state without allocating an identity or
    -- advancing activity. Same-creation-second and idle-reset requests are
    -- source-new and therefore omit the source UserPwd branch entirely.
    SELECT a.automatic_identity,a.source_new
        INTO STRICT v_automatic_identity,v_source_new
        FROM post_secrets.resolve_automatic_identity(
            p_token,p_minted,p_session_request_at,true) a;

    SELECT count(*) INTO v_count FROM (
        SELECT op.id FROM content.posts op
        JOIN content.threads t ON t.board=op.board AND t.id=op.thread_id
        WHERE op.board=p_board AND op.id=op.thread_id AND NOT op.deleted
            AND NOT t.deleted AND t.archived_at IS NULL
            -- Strict lower edge, no upper edge, and stored OP source time.
            AND extract(epoch FROM op.created_at)>p_request_at-v_period::bigint*3600
            AND EXISTS(SELECT 1 FROM post_secrets.posting_history h
                WHERE h.board=op.board AND h.post_id=op.id AND h.thread_id=op.thread_id
                    AND (h.actor_hash=p_actor
                        OR (NOT v_source_new AND v_automatic_identity IS NOT NULL
                            AND h.automatic_identity=v_automatic_identity)))
        -- One EXISTS counts an OP once even when both branches match.
        -- Zero remains a real reject-all maximum.
        LIMIT v_limit
    ) bounded;
    RETURN QUERY SELECT v_count>=v_limit,v_limit,v_period;
END $$;

REVOKE ALL ON FUNCTION content.check_user_thread_quota(bytea,text,bigint,bytea,boolean,bigint)
    FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.check_user_thread_quota(bytea,text,bigint,bytea,boolean,bigint)
    TO board_public,board_staff;
GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner;
ALTER FUNCTION content.check_user_thread_quota(bytea,text,bigint,bytea,boolean,bigint)
    OWNER TO board_posting_cooldown_owner;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner;
-- No runtime role gains direct access to private identity or history rows.
-- SQL failures propagate fail-closed, with the unchanged source quota error.
