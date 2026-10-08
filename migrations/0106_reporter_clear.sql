-- Reporter-wide clearing uses captured IP/session equality from active membership.
-- Retain reports and evidence; do not infer ownership for historical rows.
ALTER TABLE content.reports ADD COLUMN reporter_cleared_at timestamptz;
GRANT SELECT(id,reporter_cleared_at),UPDATE(reporter_cleared_at)
    ON content.reports TO board_report_admission_owner;

ALTER TABLE content.moderation_audit ADD COLUMN reporter_clear_count bigint;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_reporter_clear_count CHECK (
    (action='reporter-clear' AND reporter_clear_count IS NOT NULL
        AND reporter_clear_count BETWEEN 1 AND 10000)
    OR (action<>'reporter-clear' AND reporter_clear_count IS NULL)
);
ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check
    CHECK(action IN ('close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
        'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post','spoiler','unspoiler',
        'undead','unundead','thread-options','force-archive','reporter-clear'));
-- Existing snapshot and mask constraints require NULL for this action.

GRANT CREATE ON SCHEMA content TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
CREATE FUNCTION content.clear_reporter(p_board text,p_report bigint) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    v_invoker text;
    v_boards text[];
    v_actor bytea;
    v_automatic uuid;
    v_ids bigint[];
    v_deleted bigint[];
    v_boards_locked boolean;
    v_updated bigint;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text
        ELSE current_setting('role') END;
    IF v_invoker<>'board_staff' THEN
        RAISE EXCEPTION 'Reporter clear is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Reporter clear requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Start in a fresh transaction: no seed/board/report tuple locks beforehand.
    -- Lock every current board before the admission gate, in canonical order.
    SELECT coalesce(array_agg(locked.slug ORDER BY locked.slug),ARRAY[]::text[])
        INTO v_boards FROM (
            SELECT b.slug FROM content.boards b ORDER BY b.slug LIMIT 513 FOR UPDATE OF b
        ) locked;
    IF cardinality(v_boards)>512 THEN
        RAISE EXCEPTION 'Reporter clear board limit exceeded.' USING ERRCODE='54000';
    END IF;
    PERFORM singleton FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094';
    END IF;
    -- Fresh statements after lock waits see committed admissions/retirements.
    SELECT m.actor_hash,m.automatic_identity INTO v_actor,v_automatic
        FROM post_secrets.report_membership m
        WHERE m.board=p_board AND m.report_id=p_report;
    IF NOT FOUND THEN RETURN NULL; END IF;
    SELECT array_agg(matched.report_id ORDER BY matched.report_id),
        bool_and(matched.board=ANY(v_boards)) INTO v_ids,v_boards_locked
        FROM (
            SELECT m.report_id,m.board FROM post_secrets.report_membership m
            WHERE m.actor_hash=v_actor
                OR (v_automatic IS NOT NULL AND m.automatic_identity=v_automatic)
            ORDER BY m.report_id LIMIT 10001
        ) matched;
    IF cardinality(v_ids)>10000 THEN
        RAISE EXCEPTION 'Reporter clear report limit exceeded.' USING ERRCODE='54000';
    END IF;
    -- A newly committed board may have admitted reports before we got the gate.
    -- Never append its lock here: abort and retry the whole fresh transaction.
    IF v_boards_locked IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Reporter clear board set changed.' USING ERRCODE='55P03';
    END IF;
    WITH deleted AS (
        DELETE FROM post_secrets.report_membership m WHERE m.report_id=ANY(v_ids)
        RETURNING m.report_id
    ) SELECT array_agg(d.report_id ORDER BY d.report_id) INTO v_deleted FROM deleted d;
    -- The existing DELETE trigger preserves partial group counters and removes
    -- only empty groups. Mark exactly the retained reports actually deleted.
    UPDATE content.reports r SET reporter_cleared_at=clock_timestamp()
        WHERE r.id=ANY(v_deleted) AND r.reporter_cleared_at IS NULL;
    GET DIAGNOSTICS v_updated=ROW_COUNT;
    IF cardinality(v_deleted) IS DISTINCT FROM cardinality(v_ids)
        OR v_updated IS DISTINCT FROM cardinality(v_deleted)::bigint OR v_updated<1 THEN
        RAISE EXCEPTION 'Reporter clear membership changed.' USING ERRCODE='23514';
    END IF;
    RETURN v_updated;
END $$;
REVOKE ALL ON FUNCTION content.clear_reporter(text,bigint)
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
GRANT EXECUTE ON FUNCTION content.clear_reporter(text,bigint) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_report_admission_owner;
