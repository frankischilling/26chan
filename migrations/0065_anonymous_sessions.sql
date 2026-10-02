-- Opaque capability hashes and private activity never enter public content.
CREATE TABLE post_secrets.anonymous_policy (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    session_limit integer NOT NULL CHECK (session_limit BETWEEN 1 AND 1000000)
);
INSERT INTO post_secrets.anonymous_policy VALUES(true,100000);
CREATE TABLE post_secrets.anonymous_sessions (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash)=32),
    network_hash bytea NOT NULL CHECK (octet_length(network_hash)=32),
    address_hash bytea NOT NULL CHECK (octet_length(address_hash)=32),
    environment_hash bytea NOT NULL CHECK (octet_length(environment_hash)=32),
    created_at bigint NOT NULL CHECK (created_at>0),
    network_at bigint NOT NULL CHECK (network_at>0),
    address_at bigint NOT NULL CHECK (address_at>0),
    environment_at bigint NOT NULL CHECK (environment_at>0),
    activity_at bigint NOT NULL DEFAULT 0 CHECK (activity_at>=0),
    action_at bigint NOT NULL DEFAULT 0 CHECK (action_at>=0),
    expires_at bigint NOT NULL CHECK (expires_at>0),
    verified_level smallint NOT NULL DEFAULT 0 CHECK (verified_level BETWEEN 0 AND 255),
    posts smallint NOT NULL DEFAULT 0 CHECK (posts BETWEEN 0 AND 255),
    images smallint NOT NULL DEFAULT 0 CHECK (images BETWEEN 0 AND 255),
    threads smallint NOT NULL DEFAULT 0 CHECK (threads BETWEEN 0 AND 255),
    reports smallint NOT NULL DEFAULT 0 CHECK (reports BETWEEN 0 AND 255),
    pending smallint NOT NULL DEFAULT 0 CHECK (pending BETWEEN 0 AND 15),
    change_score smallint NOT NULL DEFAULT 0 CHECK (change_score BETWEEN 0 AND 32)
);
CREATE INDEX anonymous_sessions_expiry ON post_secrets.anonymous_sessions(expires_at);
CREATE TABLE post_secrets.anonymous_posts (
    post_id bigint PRIMARY KEY REFERENCES content.posts(id) ON DELETE CASCADE,
    token_hash bytea NOT NULL REFERENCES post_secrets.anonymous_sessions(token_hash) ON DELETE CASCADE,
    password_proof bytea NOT NULL CHECK (octet_length(password_proof)=32)
);
CREATE TABLE post_secrets.anonymous_reports (
    report_id bigint PRIMARY KEY REFERENCES content.reports(id) ON DELETE CASCADE,
    token_hash bytea NOT NULL REFERENCES post_secrets.anonymous_sessions(token_hash) ON DELETE CASCADE
);
REVOKE ALL ON post_secrets.anonymous_policy,post_secrets.anonymous_sessions,
    post_secrets.anonymous_posts,post_secrets.anonymous_reports FROM PUBLIC;
GRANT USAGE ON SCHEMA content,post_secrets TO board_anonymous_owner;
GRANT SELECT,UPDATE ON post_secrets.anonymous_policy TO board_anonymous_owner;
GRANT SELECT,INSERT,UPDATE,DELETE ON post_secrets.anonymous_sessions TO board_anonymous_owner;
GRANT SELECT,INSERT ON post_secrets.anonymous_posts,post_secrets.anonymous_reports TO board_anonymous_owner;
GRANT UPDATE(token_hash) ON post_secrets.anonymous_posts TO board_anonymous_owner;
GRANT SELECT(slug,staff_only),UPDATE(slug) ON content.boards TO board_anonymous_owner;
GRANT SELECT(id,board,thread_id,deleted) ON content.posts TO board_anonymous_owner;
GRANT SELECT(id,board,post_id) ON content.reports TO board_anonymous_owner;
GRANT SELECT(post_id,password_hash) ON post_secrets.deletion TO board_anonymous_owner;
GRANT SELECT ON content.visible_post_media,content.visible_threads TO board_anonymous_owner;

CREATE FUNCTION content.anonymous_session(p_token bytea)
RETURNS TABLE(created_at bigint,network_at bigint,address_at bigint,environment_at bigint,
    activity_at bigint,action_at bigint,verified_level smallint,posts smallint,images smallint,
    threads smallint,reports smallint,pending smallint,change_score smallint,
    network_hash bytea,address_hash bytea,environment_hash bytea)
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
    SELECT s.created_at,s.network_at,s.address_at,s.environment_at,s.activity_at,s.action_at,
        s.verified_level,s.posts,s.images,s.threads,s.reports,s.pending,s.change_score,
        s.network_hash,s.address_hash,s.environment_hash
    FROM post_secrets.anonymous_sessions s
    WHERE s.token_hash=p_token AND octet_length(p_token)=32
        AND s.expires_at>extract(epoch FROM clock_timestamp())::bigint
$$;
REVOKE ALL ON FUNCTION content.anonymous_session(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.anonymous_session(bytea) TO board_public;

-- Called only by the fixed post/report registration functions. The runtime
-- cannot write ages, counters or verification levels through an argument.
CREATE FUNCTION post_secrets.advance_anonymous_session(
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_kind smallint,p_now bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE
    s post_secrets.anonymous_sessions%ROWTYPE;
    v_limit integer; v_last bigint; v_idle bigint; v_elapsed bigint; v_delta integer;
BEGIN
    IF p_token IS NULL OR octet_length(p_token)<>32
        OR p_network IS NULL OR octet_length(p_network)<>32
        OR p_address IS NULL OR octet_length(p_address)<>32
        OR p_environment IS NULL OR octet_length(p_environment)<>32
        OR p_minted IS NULL OR p_kind NOT IN (1,3,5,7,8) OR p_kind IS NULL
        OR p_now IS NULL OR p_now<=0
        OR abs(p_now-extract(epoch FROM clock_timestamp())::bigint)>30 THEN
        RAISE EXCEPTION 'Invalid anonymous activity context.' USING ERRCODE='23514';
    END IF;
    SELECT a.* INTO s FROM post_secrets.anonymous_sessions a WHERE a.token_hash=p_token FOR UPDATE;
    IF NOT FOUND THEN
        IF NOT p_minted THEN
            RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000';
        END IF;
        -- New identities serialize on the private policy row for the capacity
        -- check. Existing identities only lock their own activity row.
        SELECT session_limit INTO v_limit FROM post_secrets.anonymous_policy WHERE singleton FOR UPDATE;
        IF NOT FOUND OR v_limit IS NULL THEN
            RAISE EXCEPTION 'Anonymous session policy is unavailable.' USING ERRCODE='55000';
        END IF;
        DELETE FROM post_secrets.anonymous_sessions a WHERE a.token_hash IN (
            SELECT old.token_hash FROM post_secrets.anonymous_sessions old
            WHERE old.expires_at<=p_now ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED
        );
        IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.anonymous_sessions LIMIT v_limit) bounded)>=v_limit THEN
            RAISE EXCEPTION 'Anonymous session capacity is exhausted.' USING ERRCODE='53300';
        END IF;
        INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
            created_at,network_at,address_at,environment_at,expires_at)
        VALUES(p_token,p_network,p_address,p_environment,p_now,p_now,p_now,p_now,p_now+31536000)
        RETURNING * INTO s;
    ELSIF p_minted OR s.expires_at<=p_now THEN
        RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000';
    ELSE
        v_last:=CASE WHEN s.activity_at>0 THEN s.activity_at ELSE s.created_at END;
        IF greatest(0,p_now-v_last)>=604800 THEN
            -- The source keeps its decoded password while resetting all loaded
            -- activity. The token and post memberships remain the same here.
            s.created_at:=p_now; s.network_at:=p_now; s.address_at:=p_now;
            s.environment_at:=p_now; s.action_at:=p_now; s.activity_at:=0;
            s.verified_level:=0; s.posts:=0; s.images:=0; s.threads:=0;
            s.reports:=0; s.pending:=0; s.change_score:=0;
        ELSE
            IF s.environment_hash<>p_environment THEN s.environment_at:=p_now; END IF;
            IF s.network_hash<>p_network THEN
                s.network_at:=p_now; s.address_at:=p_now;
            ELSIF s.address_hash<>p_address THEN s.address_at:=p_now;
            END IF;
        END IF;
    END IF;
    s.pending:=s.pending | p_kind;
    v_idle:=CASE WHEN s.activity_at>0 THEN greatest(0,p_now-s.activity_at) ELSE s.created_at END;
    v_delta:=-1;
    IF v_idle<1800 AND s.created_at<>p_now THEN
        IF s.network_at=p_now THEN v_delta:=3;
        ELSIF s.address_at=p_now THEN v_delta:=1;
        END IF;
    END IF;
    s.change_score:=least(32,greatest(0,s.change_score+v_delta));
    IF s.change_score>=32 THEN
        s.posts:=0; s.images:=0; s.threads:=0; s.reports:=0; s.pending:=0;
    END IF;
    IF s.action_at=0 THEN s.action_at:=p_now;
    ELSE
        v_elapsed:=greatest(0,p_now-s.action_at);
        IF v_elapsed>=14400 THEN
            IF (s.pending & 1)<>0 THEN s.posts:=least(255,s.posts+1); END IF;
            IF (s.pending & 2)<>0 THEN s.images:=least(255,s.images+1); END IF;
            IF (s.pending & 4)<>0 THEN s.threads:=least(255,s.threads+1); END IF;
            IF (s.pending & 8)<>0 THEN s.reports:=least(255,s.reports+1); END IF;
            s.pending:=0; s.action_at:=p_now;
        END IF;
    END IF;
    UPDATE post_secrets.anonymous_sessions a SET
        network_hash=p_network,address_hash=p_address,environment_hash=p_environment,
        created_at=s.created_at,network_at=s.network_at,address_at=s.address_at,
        environment_at=s.environment_at,activity_at=p_now,action_at=s.action_at,
        expires_at=p_now+31536000,verified_level=s.verified_level,posts=s.posts,
        images=s.images,threads=s.threads,reports=s.reports,pending=s.pending,change_score=s.change_score
    WHERE a.token_hash=p_token;
END $$;
REVOKE ALL ON FUNCTION post_secrets.advance_anonymous_session(bytea,bytea,bytea,bytea,boolean,smallint,bigint) FROM PUBLIC;

CREATE FUNCTION content.register_anonymous_post(
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_board text,p_post bigint,p_now bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_kind smallint; v_proof bytea;
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
    PERFORM post_secrets.advance_anonymous_session(p_token,p_network,p_address,p_environment,p_minted,v_kind,p_now);
    INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof) VALUES(p_post,p_token,v_proof);
END $$;
REVOKE ALL ON FUNCTION content.register_anonymous_post(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.register_anonymous_post(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint) TO board_public;

CREATE FUNCTION content.register_anonymous_report(
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_board text,p_report bigint,p_now bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
    IF NOT EXISTS(SELECT 1 FROM content.reports r WHERE r.id=p_report AND r.board=p_board)
        OR EXISTS(SELECT 1 FROM post_secrets.anonymous_reports a WHERE a.report_id=p_report) THEN
        RAISE EXCEPTION 'Anonymous report registration is unavailable.' USING ERRCODE='23514';
    END IF;
    PERFORM post_secrets.advance_anonymous_session(p_token,p_network,p_address,p_environment,p_minted,8::smallint,p_now);
    INSERT INTO post_secrets.anonymous_reports(report_id,token_hash) VALUES(p_report,p_token);
END $$;
REVOKE ALL ON FUNCTION content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint) TO board_public;

CREATE FUNCTION content.anonymous_post_proof(p_token bytea,p_board text,p_post bigint)
RETURNS bytea LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
    SELECT a.password_proof FROM post_secrets.anonymous_posts a
    JOIN post_secrets.anonymous_sessions s ON s.token_hash=a.token_hash
    JOIN content.posts p ON p.id=a.post_id
    JOIN content.boards b ON b.slug=p.board
    JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board
    JOIN post_secrets.deletion d ON d.post_id=p.id
    WHERE a.token_hash=p_token AND octet_length(p_token)=32
        AND s.expires_at>extract(epoch FROM clock_timestamp())::bigint
        AND p.board=p_board AND p.id=p_post AND NOT p.deleted AND NOT t.deleted AND NOT b.staff_only
        AND a.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
$$;
REVOKE ALL ON FUNCTION content.anonymous_post_proof(bytea,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.anonymous_post_proof(bytea,text,bigint) TO board_public;

CREATE FUNCTION content.lock_anonymous_post_proof(p_token bytea,p_board text,p_post bigint)
RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_proof bytea;
BEGIN
    -- The mutation caller holds the board lock. Lock the session before its
    -- membership, matching registration and expired-session cleanup order.
    -- Posting later updates activity. Take its final lock strength now so
    -- same-session replies on different boards cannot deadlock on an upgrade.
    PERFORM s.token_hash FROM post_secrets.anonymous_sessions s
        WHERE s.token_hash=p_token AND octet_length(p_token)=32
            AND s.expires_at>extract(epoch FROM clock_timestamp())::bigint FOR UPDATE;
    IF NOT FOUND THEN RETURN NULL; END IF;
    SELECT a.password_proof INTO v_proof FROM post_secrets.anonymous_posts a
    JOIN content.posts p ON p.id=a.post_id
    JOIN content.boards b ON b.slug=p.board
    JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board
    JOIN post_secrets.deletion d ON d.post_id=p.id
    WHERE a.token_hash=p_token AND p.board=p_board AND p.id=p_post
        AND NOT p.deleted AND NOT t.deleted AND NOT b.staff_only
        AND a.password_proof=sha256(convert_to(d.password_hash,'UTF8')) FOR SHARE OF a;
    RETURN v_proof;
END $$;
REVOKE ALL ON FUNCTION content.lock_anonymous_post_proof(bytea,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.lock_anonymous_post_proof(bytea,text,bigint) TO board_public;

GRANT CREATE ON SCHEMA content,post_secrets TO board_anonymous_owner;
ALTER FUNCTION content.anonymous_session(bytea) OWNER TO board_anonymous_owner;
ALTER FUNCTION post_secrets.advance_anonymous_session(bytea,bytea,bytea,bytea,boolean,smallint,bigint) OWNER TO board_anonymous_owner;
ALTER FUNCTION content.register_anonymous_post(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint) OWNER TO board_anonymous_owner;
ALTER FUNCTION content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint) OWNER TO board_anonymous_owner;
ALTER FUNCTION content.anonymous_post_proof(bytea,text,bigint) OWNER TO board_anonymous_owner;
ALTER FUNCTION content.lock_anonymous_post_proof(bytea,text,bigint) OWNER TO board_anonymous_owner;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_anonymous_owner;
