-- IP branch of imgboard.php:5691-5697,9537-9571. Password and Pass matching
-- require separate trusted authorities and are deliberately not synthesized.
ALTER TABLE content.boards
    ADD COLUMN user_thread_limit integer NOT NULL DEFAULT 5
        CHECK (user_thread_limit BETWEEN 0 AND 100000),
    ADD COLUMN user_thread_period_hours integer NOT NULL DEFAULT 24
        CHECK (user_thread_period_hours BETWEEN 0 AND 876000);
-- Executable global_config.ini and boards/*.config.ini values, not commented
-- examples. These are independent of the total per-board thread_limit.
UPDATE content.boards SET user_thread_limit=3
    WHERE slug IN ('a','bant','i','pol','qa','v','vm','vmg','vrpg','vst');
UPDATE content.boards SET user_thread_limit=50 WHERE slug='test';
UPDATE content.boards SET user_thread_period_hours=168 WHERE slug='i';
UPDATE content.boards SET user_thread_period_hours=6 WHERE slug='pol';
UPDATE content.boards SET user_thread_period_hours=48 WHERE slug='qa';
UPDATE content.boards SET user_thread_period_hours=72 WHERE slug='qst';
UPDATE content.boards SET user_thread_period_hours=120 WHERE slug='news';

-- Reuse 0087/0090's restricted owner and RLS-visible board/post/thread reads.
-- Runtime roles gain no history access and no new counter or identity table.
GRANT SELECT(user_thread_limit,user_thread_period_hours)
    ON content.boards TO board_posting_cooldown_owner;

CREATE FUNCTION content.check_user_thread_quota(
    p_actor bytea,p_board text,p_request_at bigint
) RETURNS TABLE(rejected boolean,user_thread_limit integer,user_thread_period_hours integer)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_limit integer; v_period integer; v_count bigint;
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
    -- All OP callers, including authenticated staff, already hold 0087's actor
    -- stripe -> global OP gate -> board locks. Reenter only the board here.
    -- This serializes quota, insertion, deletion/archive and policy changes.
    SELECT b.user_thread_limit,b.user_thread_period_hours INTO v_limit,v_period
        FROM content.boards b WHERE b.slug=p_board
            AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator'))
        FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Board not found.' USING ERRCODE='P0002'; END IF;

    SELECT count(*) INTO v_count FROM (
        SELECT op.id FROM content.posts op
        JOIN content.threads t ON t.board=op.board AND t.id=op.thread_id
        WHERE op.board=p_board AND op.id=op.thread_id AND NOT op.deleted
            AND NOT t.deleted AND t.archived_at IS NULL
            -- Compare the stored source timestamp, not history registration
            -- time. Numeric epoch arithmetic avoids timestamp-range overflow.
            -- The source has a strict lower edge and no upper edge.
            AND extract(epoch FROM op.created_at)>p_request_at-v_period::bigint*3600
            AND EXISTS(SELECT 1 FROM post_secrets.posting_history h
                WHERE h.board=op.board AND h.post_id=op.id AND h.thread_id=op.thread_id
                    AND h.actor_hash=p_actor)
        -- OP and thread primary keys plus EXISTS count each OP exactly once.
        -- Zero is a real reject-all maximum, never a disable switch.
        LIMIT v_limit
    ) bounded;
    RETURN QUERY SELECT v_count>=v_limit,v_limit,v_period;
END $$;

REVOKE ALL ON FUNCTION content.check_user_thread_quota(bytea,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.check_user_thread_quota(bytea,text,bigint)
    TO board_public,board_staff;
GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner;
ALTER FUNCTION content.check_user_thread_quota(bytea,text,bigint)
    OWNER TO board_posting_cooldown_owner;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner;
-- No backfill: historical and key-rotated IP identities remain unknown. SQL
-- errors propagate (fail closed), deliberately unlike the source's fail-open.
