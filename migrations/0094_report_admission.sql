-- modes/report.php:110-168, IP branch only. Opaque session tokens are not
-- source deletion passwords, and no Pass authority is synthesized here.
-- Existing reports are deliberately not backfilled with invented identities.
CREATE TABLE post_secrets.report_admission_gate (
    singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
    membership_limit integer NOT NULL DEFAULT 100000 CHECK(membership_limit BETWEEN 1 AND 1000000)
);
INSERT INTO post_secrets.report_admission_gate(singleton) VALUES(true);
CREATE TABLE post_secrets.report_membership (
    report_id bigint PRIMARY KEY REFERENCES content.reports(id) ON DELETE CASCADE,
    actor_hash bytea NOT NULL CHECK(octet_length(actor_hash)=32),
    board text NOT NULL,
    post_id bigint NOT NULL,
    thread_id bigint NOT NULL,
    reported_at timestamptz NOT NULL,
    FOREIGN KEY(board,post_id) REFERENCES content.posts(board,id),
    FOREIGN KEY(board,thread_id) REFERENCES content.threads(board,id)
);
CREATE INDEX report_membership_actor_target ON post_secrets.report_membership(actor_hash,board,post_id);
CREATE INDEX report_membership_actor_time ON post_secrets.report_membership(actor_hash,reported_at);
CREATE INDEX report_membership_target ON post_secrets.report_membership(board,post_id);
CREATE INDEX report_membership_thread ON post_secrets.report_membership(board,thread_id);
-- No session FK, TTL, resolved/dismissed filter, historical backfill, or
-- eviction: even an old surviving report continues to reject duplicates.
REVOKE ALL ON post_secrets.report_admission_gate,post_secrets.report_membership
    FROM PUBLIC,board_public,board_staff,board_auth;
GRANT USAGE ON SCHEMA content,post_secrets TO board_report_admission_owner;
GRANT SELECT,UPDATE(singleton) ON post_secrets.report_admission_gate TO board_report_admission_owner;
GRANT SELECT,INSERT,DELETE ON post_secrets.report_membership TO board_report_admission_owner;
GRANT SELECT(slug,staff_only,can_report_posts,archive_retention_seconds),UPDATE(slug)
    ON content.boards TO board_report_admission_owner;
GRANT SELECT(id,board,thread_id,deleted,capcode) ON content.posts TO board_report_admission_owner;
GRANT SELECT(id,board,deleted,sticky,archived_at,archive_expires_at)
    ON content.threads TO board_report_admission_owner;
GRANT INSERT(board,post_id,reason,created_at),SELECT(id) ON content.reports TO board_report_admission_owner;
GRANT USAGE ON SEQUENCE content.reports_id_seq TO board_report_admission_owner;
CREATE POLICY report_admission_board_read ON content.boards FOR SELECT TO board_report_admission_owner USING(true);
CREATE POLICY report_admission_board_lock ON content.boards FOR UPDATE TO board_report_admission_owner USING(true) WITH CHECK(true);
-- Functions enforce the actual invoker's visibility; existing post/thread/
-- report RLS continues to apply. The owner is NOLOGIN and NOBYPASSRLS.
REVOKE INSERT(board,post_id,reason) ON content.reports FROM board_public;
REVOKE ALL ON SEQUENCE content.reports_id_seq FROM board_public;

CREATE FUNCTION post_secrets.report_target(p_board text,p_post bigint) RETURNS bigint
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
                (b.archive_retention_seconds>0 AND t.archive_expires_at>transaction_timestamp()));
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    IF p_post=v_thread AND v_sticky THEN
        RAISE EXCEPTION 'Error: You cannot report a sticky.' USING ERRCODE='P0001';
    END IF;
    IF v_capcode IS NOT NULL THEN
        RAISE EXCEPTION 'Error: You cannot report this post.' USING ERRCODE='P0001';
    END IF;
    RETURN v_thread;
END $$;

CREATE FUNCTION post_secrets.check_report_limits(p_board text,p_post bigint,p_actor bytea,p_now timestamptz)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF p_actor IS NULL OR octet_length(p_actor)<>32 THEN
        RAISE EXCEPTION 'Invalid report actor.' USING ERRCODE='22023';
    END IF;
    -- Source precedence and strict lower edges, with no upper timestamp edge.
    IF EXISTS(SELECT 1 FROM post_secrets.report_membership m
        WHERE m.actor_hash=p_actor AND m.board=p_board AND m.post_id=p_post) THEN
        RAISE EXCEPTION 'You have already reported this post.' USING ERRCODE='P0001';
    END IF;
    IF EXISTS(SELECT 1 FROM post_secrets.report_membership m
        WHERE m.actor_hash=p_actor AND m.reported_at>p_now-interval '15 seconds') THEN
        RAISE EXCEPTION 'You have to wait a while before reporting another post.' USING ERRCODE='P0001';
    END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership m
        WHERE m.actor_hash=p_actor AND m.reported_at>p_now-interval '1 hour' LIMIT 30) matches)>=30 THEN
        RAISE EXCEPTION 'You have to wait a while before reporting another post.' USING ERRCODE='P0001';
    END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership m
        WHERE m.actor_hash=p_actor AND m.reported_at>p_now-interval '24 hours' LIMIT 80) matches)>=80 THEN
        RAISE EXCEPTION 'You have to wait a while before reporting another post.' USING ERRCODE='P0001';
    END IF;
END $$;

CREATE FUNCTION content.check_report_admission(p_board text,p_post bigint,p_actor bytea)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_limit integer;
BEGIN
    -- Advisory GET: read-only, no gate/board/session locks and no reservation.
    PERFORM post_secrets.report_target(p_board,p_post);
    PERFORM post_secrets.check_report_limits(p_board,p_post,p_actor,clock_timestamp());
    SELECT membership_limit INTO v_limit FROM post_secrets.report_admission_gate WHERE singleton;
    IF v_limit IS NULL THEN
        RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094';
    END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.report_membership LIMIT v_limit) members)>=v_limit THEN
        RAISE EXCEPTION 'Report admission capacity is exhausted.' USING ERRCODE='P0094';
    END IF;
END $$;

CREATE FUNCTION content.admit_report(p_board text,p_post bigint,p_reason text,p_actor bytea)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report admission requires Read Committed.' USING ERRCODE='22023';
    END IF;
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
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
    v_thread:=post_secrets.report_target(p_board,p_post);
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

CREATE FUNCTION post_secrets.retire_deleted_report_membership() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report retirement requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Raw UPDATE owns the post/thread tuple before its trigger. NOWAIT avoids
    -- tuple -> board waiting against normal board -> tuple report/deletion work.
    PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE NOWAIT;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report board is unavailable.' USING ERRCODE='23514'; END IF;
    IF TG_TABLE_NAME='threads' THEN
        DELETE FROM post_secrets.report_membership WHERE board=NEW.board AND thread_id=NEW.id;
    ELSE
        DELETE FROM post_secrets.report_membership WHERE board=NEW.board AND post_id=NEW.id;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER retire_deleted_post_report_membership AFTER UPDATE OF deleted ON content.posts
    FOR EACH ROW WHEN (NOT OLD.deleted AND NEW.deleted)
    EXECUTE FUNCTION post_secrets.retire_deleted_report_membership();
CREATE TRIGGER retire_deleted_thread_report_membership AFTER UPDATE OF deleted ON content.threads
    FOR EACH ROW WHEN (NOT OLD.deleted AND NEW.deleted)
    EXECUTE FUNCTION post_secrets.retire_deleted_report_membership();
-- Archive-only transitions retain membership: the source illegal>=3 report
-- distinction is not implemented. Thread expiry/rollover deletion does retire.

CREATE FUNCTION post_secrets.retire_staff_file_report_membership(p_board text,p_post bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Staff report retirement is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report retirement requires Read Committed.' USING ERRCODE='22023';
    END IF;
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board FOR UPDATE NOWAIT;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report board is unavailable.' USING ERRCODE='23514'; END IF;
    DELETE FROM post_secrets.report_membership WHERE board=p_board AND post_id=p_post;
END $$;

CREATE FUNCTION content.staff_delete_post_attachment(p_board text,p_post bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Staff attachment removal is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report retirement requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Existing function checks visibility and requires a fresh file transition.
    -- A second removal fails before retirement; public file-only stays intact.
    PERFORM content.delete_post_attachment(p_board,p_post);
    PERFORM post_secrets.retire_staff_file_report_membership(p_board,p_post);
END $$;

REVOKE ALL ON FUNCTION post_secrets.report_target(text,bigint),
    post_secrets.check_report_limits(text,bigint,bytea,timestamptz),
    post_secrets.retire_deleted_report_membership(),
    post_secrets.retire_staff_file_report_membership(text,bigint),
    content.check_report_admission(text,bigint,bytea),content.admit_report(text,bigint,text,bytea),
    content.staff_delete_post_attachment(text,bigint) FROM PUBLIC,board_public,board_staff,board_auth;
-- The migrator does not inherit either owner role: finish EXECUTE grants
-- before transferring ownership rather than relying on implicit membership.
GRANT EXECUTE ON FUNCTION post_secrets.retire_staff_file_report_membership(text,bigint) TO board_attachment_owner;
GRANT EXECUTE ON FUNCTION content.check_report_admission(text,bigint,bytea),
    content.admit_report(text,bigint,text,bytea) TO board_public,board_staff;
GRANT EXECUTE ON FUNCTION content.staff_delete_post_attachment(text,bigint) TO board_staff;
GRANT CREATE ON SCHEMA content,post_secrets TO board_report_admission_owner;
ALTER TABLE post_secrets.report_admission_gate OWNER TO board_report_admission_owner;
ALTER TABLE post_secrets.report_membership OWNER TO board_report_admission_owner;
ALTER FUNCTION post_secrets.report_target(text,bigint) OWNER TO board_report_admission_owner;
ALTER FUNCTION post_secrets.check_report_limits(text,bigint,bytea,timestamptz) OWNER TO board_report_admission_owner;
ALTER FUNCTION post_secrets.retire_deleted_report_membership() OWNER TO board_report_admission_owner;
ALTER FUNCTION post_secrets.retire_staff_file_report_membership(text,bigint) OWNER TO board_report_admission_owner;
ALTER FUNCTION content.check_report_admission(text,bigint,bytea) OWNER TO board_report_admission_owner;
ALTER FUNCTION content.admit_report(text,bigint,text,bytea) OWNER TO board_report_admission_owner;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_report_admission_owner;
GRANT USAGE ON SCHEMA post_secrets TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
ALTER FUNCTION content.staff_delete_post_attachment(text,bigint) OWNER TO board_attachment_owner;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
