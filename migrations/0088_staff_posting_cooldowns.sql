-- imgboard.php:6004-6016: authenticated janitor-or-higher posting uses
-- the newest surviving post ID, including both OPs and replies, on this board.
-- This is independent of capcode cosmetics and ordinary board timer values.
CREATE INDEX posting_history_staff ON post_secrets.posting_history(board,actor_hash,post_id DESC);

CREATE FUNCTION content.check_staff_posting_cooldown(
    p_actor bytea,p_board text,p_request_at bigint
) RETURNS TABLE(kind text,remaining_seconds bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_last bigint; v_staff_only boolean; v_invoker text;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    -- The application calls only after its independent staff proof issuer has
    -- checked current session, role and board scope. This API grants no insert
    -- authority and cannot replace the proof-consuming insertion trigger.
    IF v_invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Staff posting authorization unavailable' USING ERRCODE='28000';
    END IF;
    IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_request_at IS NULL
        OR p_request_at NOT BETWEEN 0 AND 9223372036854689407 THEN
        RAISE EXCEPTION 'Invalid posting context.' USING ERRCODE='23514';
    END IF;
    -- The caller already owns the same actor gate used by ordinary posting.
    SELECT b.staff_only INTO v_staff_only FROM content.boards b WHERE b.slug=p_board FOR UPDATE;
    IF NOT FOUND OR (v_staff_only AND v_invoker NOT IN ('board_staff','board_migrator')) THEN
        RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002';
    END IF;
    SELECT h.request_at INTO v_last FROM post_secrets.posting_history h
        WHERE h.actor_hash=p_actor AND h.board=p_board ORDER BY h.post_id DESC LIMIT 1;
    IF v_last>p_request_at-5 THEN
        -- Source uses S_RENZOKU even when the attempted post is a new thread.
        kind:='reply'; remaining_seconds:=v_last+5-p_request_at;
        RETURN NEXT;
    END IF;
END $$;
REVOKE ALL ON FUNCTION content.check_staff_posting_cooldown(bytea,text,bigint)
    FROM PUBLIC,board_public;
GRANT EXECUTE ON FUNCTION content.check_staff_posting_cooldown(bytea,text,bigint) TO board_staff;
GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner;
ALTER FUNCTION content.check_staff_posting_cooldown(bytea,text,bigint) OWNER TO board_posting_cooldown_owner;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner;

-- Issuance commits on the independent auth pool before admission. A rejected
-- badged/private post must not occupy its account's bounded intent capacity.
-- Ordinary proofs retain their existing, separately scoped discard function.
CREATE FUNCTION staff_identity.discard_badged_post_authority(ticket bytea,session_token bytea)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF ticket IS NULL OR octet_length(ticket)<>32
        OR session_token IS NULL OR octet_length(session_token)<>32 THEN
        RAISE EXCEPTION 'Invalid staff posting context.' USING ERRCODE='23514';
    END IF;
    DELETE FROM post_secrets.staff_post_intents
        WHERE token_hash=ticket AND session_hash=session_token AND NOT ordinary;
END $$;
REVOKE ALL ON FUNCTION staff_identity.discard_badged_post_authority(bytea,bytea)
    FROM PUBLIC,board_public,board_staff;
GRANT EXECUTE ON FUNCTION staff_identity.discard_badged_post_authority(bytea,bytea) TO board_auth;
GRANT CREATE ON SCHEMA staff_identity TO board_staff_post_owner;
ALTER FUNCTION staff_identity.discard_badged_post_authority(bytea,bytea) OWNER TO board_staff_post_owner;
REVOKE CREATE ON SCHEMA staff_identity FROM board_staff_post_owner;
