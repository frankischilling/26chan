-- Forward-only source report-group lifetimes: modes/report.php:648-653 adds
-- persisted counters; ReportQueue.php:1891-1971 does not subtract a partial
-- reporter purge, and :970-987 removes only an actually empty aggregate.
-- imgboard.php:1821-1865 clears ALL report kinds for a post at archive when
-- its persisted illegal count is below three. Retained reports/audit are not
-- queue membership and must never reconstruct a retired group's lifetime.
-- Install empty: neither historical categories nor historical counts can be
-- inferred safely, including for already categorized pre-migration members.
GRANT SELECT(category_kind) ON content.reports TO board_report_admission_owner;
GRANT CREATE ON SCHEMA post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;

CREATE TABLE post_secrets.report_group (
    board text NOT NULL,
    post_id bigint NOT NULL,
    illegal_count bigint NOT NULL CHECK(illegal_count>=0),
    incomplete boolean NOT NULL,
    PRIMARY KEY(board,post_id)
);
-- No redundant FK: the memberships already validate board/post identities;
-- adding another parent key-share lock here would enlarge the lock graph.
REVOKE ALL ON post_secrets.report_group
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;

-- Supported mutation boundary: ordinary admissions INSERT membership, identity
-- registration UPDATEs only automatic_identity, and retirement DELETEs rows.
-- Operator bulk INSERT/DELETE requires Read Committed, enabled triggers, and
-- pre-locking ALL affected content.boards rows FOR UPDATE in canonical slug
-- order BEFORE the DML, then revalidating the target set under those locks.
-- If revalidation finds another board, restart with the complete sorted set;
-- do not acquire an additional board after membership/parent tuple locks.
-- AFTER STATEMENT NOWAIT protects only once its trigger is reached: it cannot
-- prevent waits or deadlocks from earlier membership FK checks or tuple locks.
-- Reassigning membership identity/target, rewriting historical
-- report categories, TRUNCATE, disabling triggers, and direct counter edits are
-- owner maintenance, not supported runtime mutations. No UPDATE trigger is
-- installed: registering an identity must not count a report for a second time.
CREATE FUNCTION post_secrets.increment_report_group() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report group maintenance requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Own ALL affected boards before any counter write, in the same sorted
    -- order as other multi-board work. Membership INSERT may already own FK
    -- locks, so never wait backwards for a board here. This cannot protect
    -- pre-trigger FK waits: bulk operators must pre-lock boards as above.
    -- Normal admission reenters its
    -- board -> report gate -> anonymous session order without taking new locks
    -- on the gate/session here. A conflict aborts the statement for retry.
    PERFORM b.slug FROM content.boards b
        WHERE EXISTS(SELECT 1 FROM report_group_inserted n WHERE n.board=b.slug)
        ORDER BY b.slug FOR UPDATE OF b NOWAIT;

    -- The transition relation contains only successful new rows, including
    -- multi-row statements. This deliberately hardens the source INSERT IGNORE
    -- anomaly: an ignored duplicate does not increment a persisted counter.
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
            -- PostgreSQL bigint addition raises numeric_value_out_of_range;
            -- overflow rolls back admission rather than wrapping below three.
            illegal_count=g.illegal_count+EXCLUDED.illegal_count,
            incomplete=g.incomplete OR EXCLUDED.incomplete;
    RETURN NULL;
END $$;

CREATE FUNCTION post_secrets.retire_empty_report_group() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report group maintenance requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- DELETE already owns membership tuples. Once this trigger is reached,
    -- fail promptly on a competing board holder instead of waiting backwards.
    -- Earlier tuple waits still require the operator pre-lock discipline above.
    PERFORM b.slug FROM content.boards b
        WHERE EXISTS(SELECT 1 FROM report_group_deleted d WHERE d.board=b.slug)
        ORDER BY b.slug FOR UPDATE OF b NOWAIT;
    DELETE FROM post_secrets.report_group g
        WHERE EXISTS(SELECT 1 FROM report_group_deleted d
            WHERE d.board=g.board AND d.post_id=g.post_id)
        AND NOT EXISTS(SELECT 1 FROM post_secrets.report_membership m
            WHERE m.board=g.board AND m.post_id=g.post_id);
    -- A partial purge changes neither illegal_count nor the sticky incomplete
    -- flag. The final deletion ends the lifetime; a later INSERT starts clean.
    RETURN NULL;
END $$;

CREATE TRIGGER report_membership_group_insert AFTER INSERT ON post_secrets.report_membership
    REFERENCING NEW TABLE AS report_group_inserted
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.increment_report_group();
CREATE TRIGGER report_membership_group_delete AFTER DELETE ON post_secrets.report_membership
    REFERENCING OLD TABLE AS report_group_deleted
    FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.retire_empty_report_group();

CREATE FUNCTION post_secrets.retire_archived_report_membership() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Archive report retirement requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Normal archivers own board -> thread. Raw SQL already owns a thread
    -- tuple, so NOWAIT prevents reversing that order against report admission.
    PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Report board is unavailable.' USING ERRCODE='23514';
    END IF;
    DELETE FROM post_secrets.report_membership m USING post_secrets.report_group g
        WHERE m.board=NEW.board AND m.thread_id=NEW.id
            AND g.board=m.board AND g.post_id=m.post_id
            AND NOT g.incomplete AND g.illegal_count<3;
    -- The nested statement DELETE trigger retires only empty groups. Missing
    -- or incomplete counters and counts >=3 remain conservative; no retained
    -- content.reports or audit row is deleted, resolved, or reclassified.
    RETURN NEW;
END $$;
RESET ROLE;

CREATE TRIGGER retire_archived_report_membership AFTER UPDATE OF archived_at ON content.threads
    FOR EACH ROW WHEN (OLD.archived_at IS NULL AND NEW.archived_at IS NOT NULL)
    EXECUTE FUNCTION post_secrets.retire_archived_report_membership();
-- Repeated archive updates do nothing. Reports admitted after the transition
-- stay members until a later supported retirement; installation does not walk
-- already archived threads. An actual unarchive/rearchive is a new transition.
SET ROLE board_report_admission_owner;
REVOKE ALL ON FUNCTION post_secrets.increment_report_group(),
    post_secrets.retire_empty_report_group(),post_secrets.retire_archived_report_membership()
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_report_admission_owner;
