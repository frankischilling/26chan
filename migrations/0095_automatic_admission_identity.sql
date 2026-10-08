-- Forward-only automatic-session equality, independent of recovery passwords,
-- deletion proofs and IP fingerprints. No historical identity is inferred.
ALTER TABLE post_secrets.anonymous_sessions
    ADD COLUMN automatic_identity uuid UNIQUE;
ALTER TABLE post_secrets.posting_history
    ADD COLUMN automatic_identity uuid,
    ADD COLUMN registration_xid xid8;
-- Separate statements are intentional: ADD WITH DEFAULT would mark old rows
-- as newly registered in the migration transaction. Old provenance stays NULL.
ALTER TABLE post_secrets.posting_history
    ALTER COLUMN registration_xid SET DEFAULT pg_catalog.pg_current_xact_id();
CREATE INDEX posting_history_automatic_op
    ON post_secrets.posting_history(board,automatic_identity)
    WHERE post_id=thread_id AND automatic_identity IS NOT NULL;
GRANT SELECT(post_id,board,thread_id,automatic_identity,registration_xid),UPDATE(automatic_identity)
    ON post_secrets.posting_history TO board_anonymous_owner;

-- The migrator has SET-only membership in the report owner, not INHERIT.
-- Index creation also needs CREATE in the table schema while using that role.
GRANT CREATE ON SCHEMA post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
ALTER TABLE post_secrets.report_membership
    ADD COLUMN automatic_identity uuid,
    ADD COLUMN registration_xid xid8;
ALTER TABLE post_secrets.report_membership
    ALTER COLUMN registration_xid SET DEFAULT pg_catalog.pg_current_xact_id();
CREATE INDEX report_membership_automatic_target
    ON post_secrets.report_membership(automatic_identity,board,post_id)
    WHERE automatic_identity IS NOT NULL;
CREATE INDEX report_membership_automatic_time
    ON post_secrets.report_membership(automatic_identity,reported_at)
    WHERE automatic_identity IS NOT NULL;
GRANT SELECT(report_id,board,post_id,automatic_identity,registration_xid),UPDATE(automatic_identity)
    ON post_secrets.report_membership TO board_anonymous_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_report_admission_owner;

-- History has no session FK: session expiry never destroys captured equality.
-- Helpers never allocate identities, advance activity, or expose UUIDs through
-- the public anonymous-session snapshot. Only successful registration writes.
GRANT CREATE ON SCHEMA content,post_secrets TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION post_secrets.resolve_automatic_identity(
    p_token bytea,p_minted boolean,p_request_at bigint,p_lock boolean
) RETURNS TABLE(automatic_identity uuid,source_new boolean)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE s post_secrets.anonymous_sessions%ROWTYPE; v_last bigint;
BEGIN
    IF p_token IS NULL OR octet_length(p_token)<>32 OR p_minted IS NULL
        OR p_lock IS NULL OR p_request_at IS NULL OR p_request_at<=0
        OR abs(p_request_at::numeric-extract(epoch FROM clock_timestamp())::bigint)>30 THEN
        RAISE EXCEPTION 'Invalid anonymous activity context.' USING ERRCODE='23514';
    END IF;
    IF p_lock THEN
        SELECT a.* INTO s FROM post_secrets.anonymous_sessions a WHERE a.token_hash=p_token FOR UPDATE;
    ELSE
        SELECT a.* INTO s FROM post_secrets.anonymous_sessions a WHERE a.token_hash=p_token;
    END IF;
    IF NOT FOUND THEN
        IF NOT p_minted THEN
            RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000';
        END IF;
        RETURN QUERY SELECT NULL::uuid,true;
        RETURN;
    END IF;
    -- Test current expiry after a possible lock wait, not only the captured
    -- request time. An expired stored row also conflicts with minted=true.
    IF p_minted OR s.expires_at<=p_request_at
        OR s.expires_at<=extract(epoch FROM clock_timestamp())::bigint THEN
        RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000';
    END IF;
    v_last:=CASE WHEN s.activity_at>0 THEN s.activity_at ELSE s.created_at END;
    -- userpwd.php isNew compares creation with this request's captured second.
    -- The seven-day resume reset retains its credential. Future clocks cannot
    -- underflow or accidentally turn into a seven-day idle interval.
    RETURN QUERY SELECT s.automatic_identity,
        s.created_at=p_request_at OR greatest(0,p_request_at-v_last)>=604800;
END $$;

CREATE FUNCTION post_secrets.lookup_automatic_identity(p_token bytea,p_request_at bigint)
RETURNS TABLE(automatic_identity uuid,source_new boolean)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE s post_secrets.anonymous_sessions%ROWTYPE; v_last bigint;
BEGIN
    IF (p_token IS NOT NULL AND octet_length(p_token)<>32)
        OR p_request_at IS NULL OR p_request_at<=0
        OR abs(p_request_at::numeric-extract(epoch FROM clock_timestamp())::bigint)>30 THEN
        RAISE EXCEPTION 'Invalid anonymous activity context.' USING ERRCODE='23514';
    END IF;
    -- Advisory report GET: absent/unknown/expired capability has no equality
    -- branch. This lookup takes no row lock and writes nothing.
    SELECT a.* INTO s FROM post_secrets.anonymous_sessions a WHERE a.token_hash=p_token
        AND a.expires_at>p_request_at
        AND a.expires_at>extract(epoch FROM clock_timestamp())::bigint;
    IF NOT FOUND THEN
        RETURN QUERY SELECT NULL::uuid,true;
        RETURN;
    END IF;
    v_last:=CASE WHEN s.activity_at>0 THEN s.activity_at ELSE s.created_at END;
    RETURN QUERY SELECT s.automatic_identity,
        s.created_at=p_request_at OR greatest(0,p_request_at-v_last)>=604800;
END $$;
REVOKE ALL ON FUNCTION post_secrets.resolve_automatic_identity(bytea,boolean,bigint,boolean),
    post_secrets.lookup_automatic_identity(bytea,bigint) FROM PUBLIC,board_public,board_staff,board_auth;
GRANT EXECUTE ON FUNCTION post_secrets.resolve_automatic_identity(bytea,boolean,bigint,boolean)
    TO board_posting_cooldown_owner,board_report_admission_owner;
GRANT EXECUTE ON FUNCTION post_secrets.lookup_automatic_identity(bytea,bigint)
    TO board_report_admission_owner;

CREATE OR REPLACE FUNCTION content.register_anonymous_post(
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_board text,p_post bigint,p_now bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_kind smallint; v_proof bytea; v_identity uuid;
BEGIN
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
    SELECT (1 | CASE WHEN p.id=p.thread_id THEN 4 ELSE 0 END
        | CASE WHEN EXISTS(SELECT 1 FROM content.visible_post_media m WHERE m.post_id=p.id AND NOT m.file_deleted) THEN 2 ELSE 0 END)::smallint,
        sha256(convert_to(d.password_hash,'UTF8')) INTO v_kind,v_proof
    FROM content.posts p JOIN post_secrets.deletion d ON d.post_id=p.id
    JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board
    WHERE p.id=p_post AND p.board=p_board AND NOT p.deleted AND NOT t.deleted;
    IF NOT FOUND OR EXISTS(SELECT 1 FROM post_secrets.anonymous_posts a WHERE a.post_id=p_post) THEN
        RAISE EXCEPTION 'Anonymous post registration is unavailable.' USING ERRCODE='23514';
    END IF;
    -- Registration may claim only the row inserted by this transaction's
    -- private posting-history trigger. Old unregistered posts cannot acquire
    -- either admission equality or anonymous deletion authority by adoption.
    IF NOT EXISTS(SELECT 1 FROM post_secrets.posting_history h
        JOIN content.posts p ON p.id=h.post_id AND p.board=h.board AND p.thread_id=h.thread_id
        WHERE h.post_id=p_post AND h.board=p_board
            AND h.registration_xid=pg_catalog.pg_current_xact_id()
            AND h.automatic_identity IS NULL) THEN
        RAISE EXCEPTION 'Anonymous post registration is unavailable.' USING ERRCODE='23514';
    END IF;
    PERFORM post_secrets.resolve_automatic_identity(p_token,p_minted,p_now,true);
    PERFORM post_secrets.advance_anonymous_session(p_token,p_network,p_address,p_environment,p_minted,v_kind,p_now);
    -- Valid activity owns the session lock. Allocate only now; COALESCE keeps
    -- the same private identity across activity/idle resets and cookie refresh.
    UPDATE post_secrets.anonymous_sessions a
        SET automatic_identity=coalesce(a.automatic_identity,pg_catalog.gen_random_uuid())
        WHERE a.token_hash=p_token RETURNING a.automatic_identity INTO v_identity;
    IF NOT FOUND THEN RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000'; END IF;
    INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof) VALUES(p_post,p_token,v_proof);
    UPDATE post_secrets.posting_history h SET automatic_identity=v_identity
        WHERE h.post_id=p_post AND h.board=p_board
            AND h.registration_xid=pg_catalog.pg_current_xact_id()
            AND h.automatic_identity IS NULL;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Anonymous post registration is unavailable.' USING ERRCODE='23514';
    END IF;
END $$;

CREATE OR REPLACE FUNCTION content.register_anonymous_report(
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_board text,p_report bigint,p_now bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_identity uuid;
BEGIN
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
    IF NOT EXISTS(SELECT 1 FROM content.reports r WHERE r.id=p_report AND r.board=p_board)
        OR EXISTS(SELECT 1 FROM post_secrets.anonymous_reports a WHERE a.report_id=p_report) THEN
        RAISE EXCEPTION 'Anonymous report registration is unavailable.' USING ERRCODE='23514';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM post_secrets.report_membership m
        JOIN content.reports r ON r.id=m.report_id AND r.board=m.board AND r.post_id=m.post_id
        WHERE m.report_id=p_report AND m.board=p_board
            AND m.registration_xid=pg_catalog.pg_current_xact_id()
            AND m.automatic_identity IS NULL) THEN
        RAISE EXCEPTION 'Anonymous report registration is unavailable.' USING ERRCODE='23514';
    END IF;
    PERFORM post_secrets.resolve_automatic_identity(p_token,p_minted,p_now,true);
    PERFORM post_secrets.advance_anonymous_session(p_token,p_network,p_address,p_environment,p_minted,8::smallint,p_now);
    UPDATE post_secrets.anonymous_sessions a
        SET automatic_identity=coalesce(a.automatic_identity,pg_catalog.gen_random_uuid())
        WHERE a.token_hash=p_token RETURNING a.automatic_identity INTO v_identity;
    IF NOT FOUND THEN RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000'; END IF;
    INSERT INTO post_secrets.anonymous_reports(report_id,token_hash) VALUES(p_report,p_token);
    UPDATE post_secrets.report_membership m SET automatic_identity=v_identity
        WHERE m.report_id=p_report AND m.board=p_board
            AND m.registration_xid=pg_catalog.pg_current_xact_id()
            AND m.automatic_identity IS NULL;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Anonymous report registration is unavailable.' USING ERRCODE='23514';
    END IF;
END $$;
-- Replacements retain 0065's owner and public EXECUTE contracts. The new
-- private helpers are executable only by their owner and the narrow grantees.
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_anonymous_owner;
-- No changes to the old IP-only quota/report APIs in this foundation step.
