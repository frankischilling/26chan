-- Bounded ordinary board/post clearing, separate from reporter-wide retirement.
-- Source: ReportQueue.php:2563-2734 and modes/report.php:613-642. Only captured,
-- complete effective weights qualify; no catalog seed, historical backfill,
-- cross-board unlock, orphan purge, or automatic abuse warning is introduced.
ALTER TABLE content.reports
    ADD COLUMN group_cleared_at timestamptz,
    ADD COLUMN group_cleared_by bigint,
    ADD COLUMN group_clear_inherited boolean,
    ADD CONSTRAINT reports_group_clear_complete CHECK (
        num_nonnulls(group_cleared_at,group_cleared_by,group_clear_inherited) IN (0,3));
GRANT SELECT(board,post_id,group_cleared_at,group_cleared_by,group_clear_inherited),
    UPDATE(group_cleared_at,group_cleared_by,group_clear_inherited)
    ON content.reports TO board_report_admission_owner;
-- Staff history reads one authorized board's latest 100 retained group clears.
-- Reporter retirement hides rows from this view without erasing their evidence.
CREATE INDEX reports_group_clear_history ON content.reports(board,id DESC)
    WHERE group_cleared_at IS NOT NULL AND reporter_cleared_at IS NULL;

ALTER TABLE content.moderation_audit ADD COLUMN group_clear_count bigint;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_group_clear_count CHECK (
    (action='report-group-clear' AND group_clear_count IS NOT NULL
        AND group_clear_count BETWEEN 1 AND 10000)
    OR (action<>'report-group-clear' AND group_clear_count IS NULL)
);
ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_action_check
    CHECK(action IN ('close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
        'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post','spoiler','unspoiler',
        'undead','unundead','thread-options','force-archive','reporter-clear','report-group-clear'));
-- Existing reporter-count, snapshot and mask constraints require NULL for the
-- unrelated fields. The staff handler writes one count-checked audit in the
-- SAME transaction, under its live/recent authority guard, before committing.

GRANT CREATE ON SCHEMA content,post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
ALTER TABLE post_secrets.report_group
    ADD COLUMN cleared_at timestamptz,
    ADD COLUMN cleared_by bigint,
    ADD CONSTRAINT report_group_clear_complete CHECK (
        (cleared_at IS NULL) = (cleared_by IS NULL));
-- No staff-identity FK: an immutable actor identifier must not introduce an
-- authentication-database lock into content admission or erase clear history.

CREATE OR REPLACE FUNCTION post_secrets.increment_report_group() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_expected bigint; v_updated bigint;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report group maintenance requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Supported bulk writers pre-lock ALL boards in canonical order before
    -- membership/FK tuple locks. NOWAIT only protects this later reentry.
    -- Admission already holds board -> gate -> session; take no new gate or
    -- session lock here. The transition relation contains successful INSERTs.
    PERFORM b.slug FROM content.boards b
        WHERE EXISTS(SELECT 1 FROM report_group_inserted n WHERE n.board=b.slug)
        ORDER BY b.slug FOR UPDATE OF b NOWAIT;
    IF EXISTS (
        SELECT 1 FROM report_group_inserted n
        LEFT JOIN content.reports r ON r.id=n.report_id
        WHERE r.id IS NULL OR r.board IS DISTINCT FROM n.board
            OR r.post_id IS DISTINCT FROM n.post_id
            OR r.reporter_cleared_at IS NOT NULL
            OR r.group_cleared_at IS NOT NULL OR r.group_cleared_by IS NOT NULL
            OR r.group_clear_inherited IS NOT NULL
    ) THEN
        RAISE EXCEPTION 'Report group membership evidence is inconsistent.' USING ERRCODE='23514';
    END IF;
    -- Preserve 0100's persistent counters and historical uncertainty. A new
    -- lifetime defaults to uncleared; the conflict branch deliberately leaves
    -- its existing clear metadata untouched. No retained report seeds it.
    INSERT INTO post_secrets.report_group AS g(board,post_id,illegal_count,incomplete)
        SELECT n.board,n.post_id,count(*) FILTER (WHERE r.category_kind=2),
            bool_or(r.category_kind IS NULL) OR (
                NOT EXISTS(SELECT 1 FROM post_secrets.report_group prior
                    WHERE prior.board=n.board AND prior.post_id=n.post_id)
                AND EXISTS(SELECT 1 FROM post_secrets.report_membership old_member
                    WHERE old_member.board=n.board AND old_member.post_id=n.post_id
                        AND NOT EXISTS(SELECT 1 FROM report_group_inserted added
                            WHERE added.report_id=old_member.report_id)))
        FROM report_group_inserted n
        LEFT JOIN content.reports r ON r.id=n.report_id
        GROUP BY n.board,n.post_id
        ON CONFLICT(board,post_id) DO UPDATE SET
            illegal_count=g.illegal_count+EXCLUDED.illegal_count,
            incomplete=g.incomplete OR EXCLUDED.incomplete;

    -- All three admission overloads insert membership after the report and
    -- before returning. Later session registration failure rolls this back as
    -- well. Inherited reports may have unknown weights: they inherit a proven
    -- prior clear, rather than authorizing a fresh clear or reopening a group.
    SELECT count(*) INTO v_expected FROM report_group_inserted n
        JOIN post_secrets.report_group g ON g.board=n.board AND g.post_id=n.post_id
        WHERE g.cleared_at IS NOT NULL;
    UPDATE content.reports r SET group_cleared_at=g.cleared_at,
        group_cleared_by=g.cleared_by,group_clear_inherited=true
        FROM report_group_inserted n
        JOIN post_secrets.report_group g ON g.board=n.board AND g.post_id=n.post_id
        WHERE r.id=n.report_id AND r.board=n.board AND r.post_id=n.post_id
            AND g.cleared_at IS NOT NULL AND r.reporter_cleared_at IS NULL
            AND r.group_cleared_at IS NULL AND r.group_cleared_by IS NULL
            AND r.group_clear_inherited IS NULL;
    GET DIAGNOSTICS v_updated=ROW_COUNT;
    IF v_updated IS DISTINCT FROM v_expected THEN
        RAISE EXCEPTION 'Inherited report clear count changed.' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END $$;
-- CREATE OR REPLACE retains the exact enabled statement-trigger binding and
-- private ACL. retire_empty_report_group() is unchanged: partial retirement
-- preserves clear state; last-member retirement deletes the whole lifetime.

CREATE FUNCTION content.clear_report_group(p_board text,p_post bigint,p_account bigint) RETURNS bigint
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
    -- actor. Lock the single target board before any group/report tuples; no
    -- global capacity changes mean no admission gate or session lock is needed.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE;
    IF NOT FOUND THEN RETURN NULL; END IF;
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
    RETURN v_count;
END $$;
REVOKE ALL ON FUNCTION content.clear_report_group(text,bigint,bigint)
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
GRANT EXECUTE ON FUNCTION content.clear_report_group(text,bigint,bigint) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_report_admission_owner;
