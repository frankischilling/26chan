-- Bump-only identity evidence. Public markup, poster counts and the existing
-- staff proof interfaces retain their separate, narrower membership rules.
GRANT SELECT(id,board,thread_id,deleted,created_at)
    ON content.posts TO board_posting_cooldown_owner;
-- 0087's owner-only board SELECT policy, together with post_visibility and
-- thread_visibility, already permits these reads on private boards. The owner
-- remains NOBYPASSRLS; every callable read below checks the actual invoker.

CREATE FUNCTION content.posting_op_bump_context(p_actor bytea,p_board text,p_thread bigint)
RETURNS TABLE(own_reply boolean,latest_post_id bigint,latest_created_at timestamptz)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_staff_only boolean;
BEGIN
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid posting actor.' USING ERRCODE='23514';
    END IF;
    IF p_thread IS NULL OR p_thread<=0 THEN
        RAISE EXCEPTION 'Invalid OP bump context.' USING ERRCODE='23514';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
        RAISE EXCEPTION 'OP bump context is unavailable.' USING ERRCODE='42501';
    END IF;
    SELECT b.staff_only INTO v_staff_only FROM content.boards b WHERE b.slug=p_board;
    IF NOT FOUND OR (v_staff_only AND v_invoker NOT IN ('board_staff','board_migrator')) THEN
        RETURN;
    END IF;
    RETURN QUERY
        SELECT EXISTS(SELECT 1 FROM post_secrets.posting_history h
                WHERE h.board=p_board AND h.thread_id=p_thread AND h.post_id=p_thread
                    AND h.actor_hash=p_actor),
            latest.id,latest.created_at
        FROM content.threads t
        JOIN content.posts op ON op.board=t.board AND op.thread_id=t.id AND op.id=t.id
        LEFT JOIN LATERAL (
            SELECT p.id,p.created_at FROM post_secrets.posting_history h
            JOIN content.posts p ON p.board=h.board AND p.thread_id=h.thread_id AND p.id=h.post_id
            WHERE h.board=p_board AND h.thread_id=p_thread AND h.post_id<>p_thread
                AND h.actor_hash=p_actor AND NOT p.deleted
            ORDER BY p.id DESC LIMIT 1
        ) latest ON true
        WHERE t.board=p_board AND t.id=p_thread AND NOT t.deleted
            AND t.archived_at IS NULL AND NOT op.deleted;
END $$;

-- Companion to staff_op_context, not a replacement: a known legacy OP can
-- have no replies yet. Its ownership still matters to the initial window.
-- Legacy rows cannot recover historical private/badged identities.
CREATE FUNCTION content.staff_op_bump_context(p_board text,p_thread bigint,p_peer text)
RETURNS TABLE(own_reply boolean,latest_post_id bigint,latest_created_at timestamptz)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text;
BEGIN
    IF p_thread IS NULL OR p_thread<=0 THEN
        RAISE EXCEPTION 'Invalid OP bump context.' USING ERRCODE='23514';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'OP bump context is unavailable.' USING ERRCODE='42501';
    END IF;
    RETURN QUERY
        SELECT own.matches,latest.id,latest.created_at
        FROM content.threads t JOIN content.boards b ON b.slug=t.board
        JOIN content.posts op ON op.board=t.board AND op.thread_id=t.id AND op.id=t.id
        CROSS JOIN LATERAL (
            SELECT EXISTS(SELECT 1 FROM post_secrets.op_peers o
                WHERE o.thread_id=t.id AND o.peer=p_peer::inet) AS matches
        ) own
        LEFT JOIN LATERAL (
            SELECT p.id,p.created_at FROM post_secrets.op_replies r
            JOIN content.posts p ON p.id=r.post_id
            WHERE own.matches AND r.thread_id=t.id AND p.board=p_board
                AND p.thread_id=t.id AND p.id<>t.id AND NOT p.deleted
            ORDER BY p.id DESC LIMIT 1
        ) latest ON true
        WHERE t.board=p_board AND t.id=p_thread AND NOT t.deleted
            AND t.archived_at IS NULL AND NOT op.deleted AND NOT b.staff_only AND b.slug<>'j';
END $$;

REVOKE ALL ON FUNCTION content.posting_op_bump_context(bytea,text,bigint),
    content.staff_op_bump_context(text,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.posting_op_bump_context(bytea,text,bigint) TO board_public,board_staff;
GRANT EXECUTE ON FUNCTION content.staff_op_bump_context(text,bigint,text) TO board_staff;
GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner,board_staff_post_owner;
ALTER FUNCTION content.posting_op_bump_context(bytea,text,bigint) OWNER TO board_posting_cooldown_owner;
ALTER FUNCTION content.staff_op_bump_context(text,bigint,text) OWNER TO board_staff_post_owner;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner,board_staff_post_owner;
-- No history backfill, runtime history SELECT, or additional legacy-table grants.
