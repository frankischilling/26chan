-- Explicit opt-in only: importing a catalog never activates it. No seeds,
-- backfill, effective-priority calculation, CAPTCHA or Pass authority.
GRANT CREATE ON SCHEMA content,post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
-- Immutable catalogs are validated by activation; omitting a pointer FK also
-- avoids taking a catalog-row key-share lock when switching the active mode.
ALTER TABLE post_secrets.report_admission_gate ADD COLUMN active_catalog_revision bigint;
-- Only migration-time REFERENCES authority is needed for the immutable FK.
GRANT REFERENCES(revision,id) ON post_secrets.report_catalog_rows TO board_migrator;
RESET ROLE;

ALTER TABLE content.reports
    ADD COLUMN category_revision bigint,
    ADD COLUMN category_id bigint,
    ADD COLUMN category_kind smallint,
    ADD COLUMN category_base_weight double precision,
    DROP CONSTRAINT reports_reason_check,
    ADD CONSTRAINT reports_category_complete CHECK (
        num_nonnulls(category_revision,category_id,category_kind,category_base_weight) IN (0,4)),
    ADD CONSTRAINT reports_category_kind_check CHECK (
        category_kind=CASE WHEN category_id=31 THEN 2 ELSE 1 END),
    ADD CONSTRAINT reports_category_weight_check CHECK (category_base_weight NOT IN
        ('Infinity'::double precision,'-Infinity'::double precision,'NaN'::double precision)),
    ADD CONSTRAINT reports_reason_check CHECK (
        (category_revision IS NULL AND octet_length(reason) BETWEEN 1 AND 1000)
        OR (category_revision IS NOT NULL AND octet_length(reason) BETWEEN 0 AND 4096)),
    ADD CONSTRAINT reports_category_catalog_fk FOREIGN KEY(category_revision,category_id)
        REFERENCES post_secrets.report_catalog_rows(revision,id);
GRANT INSERT(category_revision,category_id,category_kind,category_base_weight)
    ON content.reports TO board_report_admission_owner;
GRANT SELECT(worksafe) ON content.boards TO board_report_admission_owner;
GRANT SELECT(post_id,bytes,file_deleted) ON content.post_media TO board_report_admission_owner;

SET ROLE board_report_admission_owner;
REVOKE REFERENCES(revision,id) ON post_secrets.report_catalog_rows FROM board_migrator;

CREATE FUNCTION content.set_report_catalog_active(p_revision bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker<>'board_migrator' THEN
        RAISE EXCEPTION 'Report catalog access is unavailable.' USING ERRCODE='42501';
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Report catalog activation requires Read Committed.' USING ERRCODE='22023';
    END IF;
    -- Only this gate: never a board, import gate, or anonymous-session lock.
    PERFORM singleton FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094';
    END IF;
    IF p_revision IS NOT NULL THEN
        IF NOT EXISTS(SELECT 1 FROM post_secrets.report_catalog_versions
            WHERE revision=p_revision AND category_count>0) THEN
            RAISE EXCEPTION 'Report catalog revision must contain imported categories.' USING ERRCODE='22023';
        END IF;
    END IF;
    UPDATE post_secrets.report_admission_gate SET active_catalog_revision=p_revision WHERE singleton;
END $$;

-- Private selector over immutable rows. The caller must validate report_target
-- first. The only executable public paths below do so with the actual invoker.
-- Stored media bytes/deletion state deliberately ignore asset availability.
CREATE FUNCTION post_secrets.eligible_report_categories(p_board text,p_post bigint,p_revision bigint)
RETURNS TABLE(id bigint,title text,kind smallint,base_weight double precision,ordinal integer,scope_order integer)
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
    SELECT r.id,r.title,(CASE WHEN r.id=31 THEN 2 ELSE 1 END)::smallint,r.weight,r.ordinal,
        CASE WHEN r.id=31 THEN 2 WHEN r.board NOT IN ('','0') THEN 0 ELSE 1 END
    FROM post_secrets.report_catalog_rows r
    JOIN content.boards b ON b.slug=p_board
    JOIN content.posts p ON p.board=b.slug AND p.id=p_post
    LEFT JOIN content.post_media m ON m.post_id=p.id
    WHERE r.revision=p_revision AND (r.id=31 OR (
        (r.board='' OR (r.board='_ws_' AND b.worksafe)
            OR (r.board='_nws_' AND NOT b.worksafe)
            OR (r.board NOT IN ('_ws_','_nws_') AND r.board=p_board))
        AND (NOT r.op_only OR p.id=p.thread_id)
        AND (NOT r.reply_only OR p.id<>p.thread_id)
        AND (NOT r.image_only OR (m.bytes<>0 AND NOT m.file_deleted))
        AND (r.exclude_boards IS NULL OR r.exclude_boards IN ('','0')
            OR strpos(','||r.exclude_boards||',',','||p_board||',')=0)))
    ORDER BY 6,r.ordinal
$$;

CREATE FUNCTION content.report_category_form(p_board text,p_post bigint) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_revision bigint; v_categories jsonb;
BEGIN
    v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
    IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
        RAISE EXCEPTION 'Report admission is unavailable.' USING ERRCODE='42501';
    END IF;
    -- Advisory, bounded by the imported catalog limits. No locks, identity
    -- allocation, session activity, cookies or private weight/filter fields.
    PERFORM post_secrets.report_target(p_board,p_post,clock_timestamp());
    SELECT active_catalog_revision INTO v_revision
        FROM post_secrets.report_admission_gate WHERE singleton;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094';
    END IF;
    SELECT coalesce(jsonb_agg(jsonb_build_object('id',r.id,'title',r.title,
        'kind',CASE WHEN r.kind=2 THEN 'illegal' ELSE 'rule' END)
        ORDER BY r.scope_order,r.ordinal),'[]'::jsonb) INTO v_categories
        FROM post_secrets.eligible_report_categories(p_board,p_post,v_revision) r;
    RETURN jsonb_build_object('revision',v_revision,'categories',v_categories);
END $$;

CREATE OR REPLACE FUNCTION content.admit_report(p_board text,p_post bigint,p_reason text,p_actor bytea)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer; v_revision bigint;
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
    SELECT membership_limit,active_catalog_revision INTO v_limit,v_revision
        FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    IF v_revision IS NOT NULL THEN
        RAISE EXCEPTION 'Free-text reporting is not active.' USING ERRCODE='P0001';
    END IF;
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

CREATE OR REPLACE FUNCTION content.admit_report(
    p_board text,p_post bigint,p_reason text,p_actor bytea,
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_request_at bigint
)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer; v_revision bigint; v_automatic_identity uuid;
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
    SELECT membership_limit,active_catalog_revision INTO v_limit,v_revision
        FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Report admission capacity is unavailable.' USING ERRCODE='P0094'; END IF;
    IF v_revision IS NOT NULL THEN
        RAISE EXCEPTION 'Free-text reporting is not active.' USING ERRCODE='P0001';
    END IF;
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

CREATE FUNCTION content.admit_categorical_report(
    p_board text,p_post bigint,p_category bigint,p_expected_revision bigint,p_actor bytea,
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_request_at bigint
)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_thread bigint; v_now timestamptz; v_report bigint; v_limit integer; v_automatic_identity uuid; v_revision bigint; v_category record;
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
    -- Board -> singleton report gate -> anonymous session. Never acquire
    -- another board after this gate. Registration reenters this same board.
    -- Deletion retirement never takes this gate or an anonymous-session lock.
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board
        AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator')) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
    SELECT membership_limit,active_catalog_revision INTO v_limit,v_revision
        FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE;
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
    -- Authoritative mode, revision and eligibility are checked after quotas,
    -- while the gate prevents activation changes until this transaction ends.
    IF v_revision IS NULL THEN
        RAISE EXCEPTION 'Categorical reporting is not active.' USING ERRCODE='P0001';
    END IF;
    IF p_expected_revision IS DISTINCT FROM v_revision THEN
        RAISE EXCEPTION 'Report categories changed. Please reload the report form.' USING ERRCODE='P0001';
    END IF;
    SELECT * INTO v_category FROM post_secrets.eligible_report_categories(p_board,p_post,v_revision)
        WHERE id=p_category;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Invalid category selected.' USING ERRCODE='P0001';
    END IF;
    INSERT INTO content.reports(board,post_id,reason,created_at,
        category_revision,category_id,category_kind,category_base_weight)
        VALUES(p_board,p_post,v_category.title,v_now,
            v_revision,v_category.id,v_category.kind,v_category.base_weight)
        RETURNING id INTO v_report;
    INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity)
        VALUES(v_report,p_actor,p_board,p_post,v_thread,v_now,NULL);
    -- Registration requires current-transaction provenance and a NULL identity,
    -- then allocates/reuses the session UUID and stamps this membership itself.
    -- Any failure rolls the report, membership and session changes back together.
    PERFORM content.register_anonymous_report(
        p_token,p_network,p_address,p_environment,p_minted,p_board,v_report,p_request_at);
    RETURN v_report;
END $$;

REVOKE ALL ON FUNCTION content.set_report_catalog_active(bigint),
    post_secrets.eligible_report_categories(text,bigint,bigint),
    content.report_category_form(text,bigint),
    content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)
    FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
GRANT EXECUTE ON FUNCTION content.set_report_catalog_active(bigint) TO board_migrator;
GRANT EXECUTE ON FUNCTION content.report_category_form(text,bigint) TO board_public,board_staff,board_migrator;
GRANT EXECUTE ON FUNCTION content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)
    TO board_public,board_migrator;
-- CREATE OR REPLACE preserved both old free-text ACLs and actual-invoker guards.
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_report_admission_owner;
