-- Independent verification authority; public database credentials cannot mint
-- challenge answers, email tokens, verified sessions or delivery results.
CREATE SCHEMA verification_private;
REVOKE ALL ON SCHEMA verification_private FROM PUBLIC;
GRANT USAGE ON SCHEMA verification_private TO board_anonymous_owner;

CREATE TABLE verification_private.policy (
    singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
    challenge_limit integer NOT NULL CHECK(challenge_limit BETWEEN 1 AND 100000),
    receipt_limit integer NOT NULL CHECK(receipt_limit BETWEEN 1 AND 100000),
    request_limit integer NOT NULL CHECK(request_limit BETWEEN 1 AND 100000),
    outbox_limit integer NOT NULL CHECK(outbox_limit BETWEEN 1 AND 10000),
    identity_limit integer NOT NULL CHECK(identity_limit BETWEEN 1 AND 1000000)
);
INSERT INTO verification_private.policy VALUES(true,100000,100000,100000,10000,1000000);

CREATE TABLE verification_private.peers (
    peer_hash bytea PRIMARY KEY CHECK(octet_length(peer_hash)=32),
    unsolved smallint NOT NULL DEFAULT 0 CHECK(unsolved BETWEEN 0 AND 20),
    unsolved_until timestamptz NOT NULL,
    refresh_after timestamptz NOT NULL,
    expires_at timestamptz NOT NULL
);
CREATE INDEX verification_peers_expiry ON verification_private.peers(expires_at);

CREATE TABLE verification_private.challenges (
    token_hash bytea PRIMARY KEY CHECK(octet_length(token_hash)=32),
    answer_hash bytea NOT NULL CHECK(octet_length(answer_hash)=32),
    suspicious boolean NOT NULL,
    peer_hash bytea NOT NULL UNIQUE CHECK(octet_length(peer_hash)=32),
    session_hash bytea NOT NULL REFERENCES post_secrets.anonymous_sessions(token_hash) ON DELETE CASCADE,
    purpose text NOT NULL CHECK(purpose IN ('thread','reply','report','email_request','email_verify','ticket')),
    board text REFERENCES content.boards(slug) ON DELETE CASCADE,
    target bigint NOT NULL CHECK(target>=0),
    context_hash bytea NOT NULL CHECK(octet_length(context_hash)=32),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    CHECK(expires_at>created_at AND expires_at<=created_at+interval '120 seconds'),
    CHECK((purpose IN ('email_request','email_verify') AND board IS NULL AND target=0)
       OR (purpose='thread' AND board IS NOT NULL AND target=0)
       OR (purpose IN ('reply','report') AND board IS NOT NULL AND target>0)
       OR (purpose='ticket' AND board IS NOT NULL))
);
CREATE INDEX verification_challenges_expiry ON verification_private.challenges(expires_at);

CREATE TABLE verification_private.tickets (
    token_hash bytea PRIMARY KEY CHECK(octet_length(token_hash)=32),
    peer_hash bytea NOT NULL CHECK(octet_length(peer_hash)=32),
    board text NOT NULL REFERENCES content.boards(slug) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    CHECK(expires_at>created_at AND expires_at<=created_at+interval '1 hour')
);
CREATE INDEX verification_tickets_expiry ON verification_private.tickets(expires_at);

CREATE TABLE verification_private.receipts (
    token_hash bytea PRIMARY KEY CHECK(octet_length(token_hash)=32),
    peer_hash bytea NOT NULL CHECK(octet_length(peer_hash)=32),
    session_hash bytea NOT NULL REFERENCES post_secrets.anonymous_sessions(token_hash) ON DELETE CASCADE,
    purpose text NOT NULL CHECK(purpose IN ('thread','reply','report','email_request','email_verify','ticket')),
    board text REFERENCES content.boards(slug) ON DELETE CASCADE,
    target bigint NOT NULL CHECK(target>=0),
    context_hash bytea NOT NULL CHECK(octet_length(context_hash)=32),
    from_credit boolean NOT NULL DEFAULT false,
    suspicious boolean NOT NULL,
    unsolved_before smallint NOT NULL CHECK(unsolved_before BETWEEN 0 AND 20),
    may_grant_credit boolean NOT NULL,
    CHECK(NOT may_grant_credit OR (purpose IN ('thread','reply') AND NOT from_credit AND unsolved_before<2)),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    CHECK(expires_at>created_at AND expires_at<=created_at+interval '30 seconds'),
    CHECK((purpose IN ('email_request','email_verify') AND board IS NULL AND target=0 AND NOT from_credit)
       OR (purpose='thread' AND board IS NOT NULL AND target=0 AND NOT from_credit)
       OR (purpose IN ('reply','report') AND board IS NOT NULL AND target>0)
       OR (purpose='ticket' AND board IS NOT NULL AND NOT from_credit))
);
CREATE INDEX verification_receipts_expiry ON verification_private.receipts(expires_at);

CREATE TABLE verification_private.identities (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email_hash bytea NOT NULL UNIQUE CHECK(octet_length(email_hash)=32),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
-- Activity/reputation stays in each browser's anonymous_sessions row, matching
-- the primary UserPwd cookie payload. Only identity and ownership are shared.
CREATE TABLE verification_private.browser_identities (
    session_hash bytea PRIMARY KEY REFERENCES post_secrets.anonymous_sessions(token_hash) ON DELETE CASCADE,
    identity_id uuid NOT NULL REFERENCES verification_private.identities(id),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX verification_browsers_identity ON verification_private.browser_identities(identity_id);

CREATE TABLE verification_private.post_ownership (
    post_id bigint PRIMARY KEY REFERENCES content.posts(id) ON DELETE CASCADE,
    identity_id uuid NOT NULL REFERENCES verification_private.identities(id),
    password_proof bytea NOT NULL CHECK(octet_length(password_proof)=32)
);
CREATE INDEX verification_posts_identity ON verification_private.post_ownership(identity_id);

CREATE TABLE verification_private.credits (
    identity_key bytea PRIMARY KEY CHECK(octet_length(identity_key)=32),
    remaining smallint NOT NULL CHECK(remaining BETWEEN 0 AND 3),
    expires_at timestamptz NOT NULL
);
CREATE INDEX verification_credits_expiry ON verification_private.credits(expires_at);

CREATE TABLE verification_private.email_requests (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    token_hash bytea NOT NULL UNIQUE CHECK(octet_length(token_hash)=32),
    email_hash bytea NOT NULL CHECK(octet_length(email_hash)=32),
    recipient_hash bytea NOT NULL CHECK(octet_length(recipient_hash)=32),
    peer_hash bytea NOT NULL CHECK(octet_length(peer_hash)=32),
    country text NOT NULL CHECK(country ~ '^[A-Z]{2}$'),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    delivered_at timestamptz,
    used_at timestamptz,
    failed_attempts smallint NOT NULL DEFAULT 0 CHECK(failed_attempts BETWEEN 0 AND 5),
    captcha_failures smallint NOT NULL DEFAULT 0 CHECK(captcha_failures BETWEEN 0 AND 6),
    captcha_failure_limit smallint NOT NULL CHECK(captcha_failure_limit IN (3,6)),
    CHECK(expires_at>created_at AND expires_at<=created_at+interval '24 hours')
);
CREATE INDEX verification_email_peer_time ON verification_private.email_requests(peer_hash,created_at);
CREATE INDEX verification_email_identity_time ON verification_private.email_requests(email_hash,created_at);
CREATE INDEX verification_email_expiry ON verification_private.email_requests(expires_at);

CREATE TABLE verification_private.mail_outbox (
    request_id uuid PRIMARY KEY REFERENCES verification_private.email_requests(id) ON DELETE CASCADE,
    recipient_hash bytea NOT NULL UNIQUE CHECK(octet_length(recipient_hash)=32),
    encrypted_payload bytea NOT NULL CHECK(octet_length(encrypted_payload) BETWEEN 98 AND 352),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    lease_hash bytea CHECK(lease_hash IS NULL OR octet_length(lease_hash)=32),
    lease_until timestamptz,
    retry_after timestamptz NOT NULL,
    attempts smallint NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 60),
    CHECK((lease_hash IS NULL)=(lease_until IS NULL)),
    CHECK(expires_at>created_at AND expires_at<=created_at+interval '1 hour')
);
CREATE INDEX verification_mail_ready ON verification_private.mail_outbox(retry_after,created_at);
CREATE INDEX verification_mail_expiry ON verification_private.mail_outbox(expires_at);

REVOKE ALL ON ALL TABLES IN SCHEMA verification_private FROM PUBLIC;
GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA verification_private TO board_anonymous_owner;
GRANT USAGE ON SCHEMA content TO board_verifier,board_mailer;
-- No runtime table grants. Narrow issuer, sink and nested consumer functions
-- follow below; board_public must never receive their minting authority.
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET ROLE board_anonymous_owner;

CREATE FUNCTION content.verification_ensure_session(
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,p_minted boolean
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE s post_secrets.anonymous_sessions%ROWTYPE; v_now bigint; v_limit integer;
BEGIN
    IF p_token IS NULL OR octet_length(p_token)<>32
        OR p_network IS NULL OR octet_length(p_network)<>32
        OR p_address IS NULL OR octet_length(p_address)<>32
        OR p_environment IS NULL OR octet_length(p_environment)<>32 OR p_minted IS NULL THEN
        RAISE EXCEPTION 'Invalid anonymous verification context.' USING ERRCODE='23514';
    END IF;
    SELECT a.* INTO s FROM post_secrets.anonymous_sessions a WHERE a.token_hash=p_token FOR UPDATE;
    v_now:=extract(epoch FROM clock_timestamp())::bigint;
    IF FOUND THEN
        IF p_minted OR s.expires_at<=v_now THEN
            RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000';
        END IF;
        -- A challenge read never updates activity, environment, counters or
        -- expiry. The issuer resumes its snapshot for this request separately.
        RETURN false;
    END IF;
    IF NOT p_minted THEN
        RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000';
    END IF;
    SELECT session_limit INTO v_limit FROM post_secrets.anonymous_policy WHERE singleton FOR UPDATE;
    IF NOT FOUND OR v_limit IS NULL THEN
        RAISE EXCEPTION 'Anonymous session policy is unavailable.' USING ERRCODE='55000';
    END IF;
    DELETE FROM post_secrets.anonymous_sessions a WHERE a.token_hash IN (
        SELECT old.token_hash FROM post_secrets.anonymous_sessions old
        WHERE old.expires_at<=v_now ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED
    );
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.anonymous_sessions LIMIT v_limit) bounded)>=v_limit THEN
        RAISE EXCEPTION 'Anonymous session capacity is unavailable.' USING ERRCODE='54000';
    END IF;
    INSERT INTO post_secrets.anonymous_sessions(
        token_hash,network_hash,address_hash,environment_hash,created_at,network_at,
        address_at,environment_at,expires_at
    ) VALUES(p_token,p_network,p_address,p_environment,v_now,v_now,v_now,v_now,v_now+31536000);
    RETURN true;
END $$;
ALTER FUNCTION content.verification_ensure_session(bytea,bytea,bytea,bytea,boolean) OWNER TO board_anonymous_owner;
REVOKE ALL ON FUNCTION content.verification_ensure_session(bytea,bytea,bytea,bytea,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_ensure_session(bytea,bytea,bytea,bytea,boolean) TO board_verifier;
GRANT EXECUTE ON FUNCTION content.anonymous_session(bytea) TO board_verifier;

CREATE FUNCTION content.verification_issue_challenge(
    p_token bytea,p_answer bytea,p_peer bytea,p_session bytea,p_purpose text,
    p_board text,p_target bigint,p_context bytea,p_suspicious boolean
) RETURNS TABLE(issued boolean,cooldown_seconds integer,valid_until timestamptz)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now timestamptz; v_session_expiry bigint; v_limit integer;
    v_peer verification_private.peers%ROWTYPE; v_unsolved integer; v_cooldown integer;
BEGIN
    IF p_token IS NULL OR octet_length(p_token)<>32 OR p_answer IS NULL OR octet_length(p_answer)<>32
        OR p_peer IS NULL OR octet_length(p_peer)<>32 OR p_session IS NULL OR octet_length(p_session)<>32
        OR p_context IS NULL OR octet_length(p_context)<>32 OR p_target IS NULL OR p_target<0 OR p_suspicious IS NULL
        OR p_purpose IS NULL OR p_purpose NOT IN ('thread','reply','report','email_request','email_verify','ticket')
        OR NOT ((p_purpose IN ('email_request','email_verify') AND p_board IS NULL AND p_target=0)
          OR (p_purpose='thread' AND p_board IS NOT NULL AND p_target=0)
          OR (p_purpose IN ('reply','report') AND p_board IS NOT NULL AND p_target>0)
          OR (p_purpose='ticket' AND p_board IS NOT NULL)) THEN
        RAISE EXCEPTION 'Invalid challenge context.' USING ERRCODE='23514';
    END IF;
    -- Keep board-before-session ordering used by the protected submit paths.
    IF p_board IS NOT NULL THEN
        PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR KEY SHARE;
        IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
        IF p_purpose='reply' AND NOT EXISTS(SELECT 1 FROM content.visible_threads t WHERE t.board=p_board AND t.id=p_target)
            OR p_purpose='report' AND NOT EXISTS(SELECT 1 FROM content.posts p JOIN content.visible_threads t
                ON t.board=p.board AND t.id=p.thread_id WHERE p.board=p_board AND p.id=p_target AND NOT p.deleted) THEN
            RAISE EXCEPTION 'Public target not found.' USING ERRCODE='P0002';
        END IF;
    END IF;
    SELECT s.expires_at INTO v_session_expiry FROM post_secrets.anonymous_sessions s
        WHERE s.token_hash=p_session FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000'; END IF;
    SELECT challenge_limit INTO v_limit FROM verification_private.policy WHERE singleton FOR UPDATE;
    v_now:=clock_timestamp();
    IF v_limit IS NULL THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
    IF v_session_expiry<=extract(epoch FROM v_now)::bigint THEN
        RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000';
    END IF;
    DELETE FROM verification_private.challenges c WHERE c.token_hash IN (
        SELECT old.token_hash FROM verification_private.challenges old WHERE old.expires_at<=v_now
        ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED);
    DELETE FROM verification_private.peers p WHERE p.peer_hash IN (
        SELECT old.peer_hash FROM verification_private.peers old WHERE old.expires_at<=v_now
        ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED);
    SELECT p.* INTO v_peer FROM verification_private.peers p WHERE p.peer_hash=p_peer FOR UPDATE;
    IF FOUND AND v_peer.refresh_after>v_now THEN
        RETURN QUERY SELECT false,greatest(1,ceil(extract(epoch FROM v_peer.refresh_after-v_now))::integer),NULL::timestamptz;
        RETURN;
    END IF;
    IF NOT FOUND THEN
        IF (SELECT count(*) FROM (SELECT 1 FROM verification_private.peers LIMIT v_limit) bounded)>=v_limit THEN
            RAISE EXCEPTION 'Verification capacity is unavailable.' USING ERRCODE='54000';
        END IF;
        INSERT INTO verification_private.peers(peer_hash,unsolved,unsolved_until,refresh_after,expires_at)
            VALUES(p_peer,0,v_now,v_now,v_now+interval '600 seconds') RETURNING * INTO v_peer;
    END IF;
    v_unsolved:=CASE WHEN v_peer.unsolved_until>v_now THEN v_peer.unsolved ELSE 0 END;
    v_cooldown:=CASE WHEN v_unsolved>10 THEN 300 WHEN v_unsolved>2 THEN v_unsolved*8 ELSE 5 END;
    -- Refresh replaces the old per-peer challenge without inventing a solution.
    DELETE FROM verification_private.challenges WHERE peer_hash=p_peer;
    IF (SELECT count(*) FROM (SELECT 1 FROM verification_private.challenges LIMIT v_limit) bounded)>=v_limit THEN
        RAISE EXCEPTION 'Verification capacity is unavailable.' USING ERRCODE='54000';
    END IF;
    INSERT INTO verification_private.challenges(token_hash,answer_hash,suspicious,peer_hash,session_hash,purpose,board,target,context_hash,created_at,expires_at)
        VALUES(p_token,p_answer,p_suspicious,p_peer,p_session,p_purpose,p_board,p_target,p_context,v_now,v_now+interval '120 seconds');
    UPDATE verification_private.peers SET unsolved=least(v_unsolved+1,20),unsolved_until=v_now+interval '600 seconds',
        refresh_after=v_now+make_interval(secs=>v_cooldown),expires_at=v_now+interval '600 seconds' WHERE peer_hash=p_peer;
    RETURN QUERY SELECT true,v_cooldown,v_now+interval '120 seconds';
END $$;
REVOKE ALL ON FUNCTION content.verification_issue_challenge(bytea,bytea,bytea,bytea,text,text,bigint,bytea,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_issue_challenge(bytea,bytea,bytea,bytea,text,text,bigint,bytea,boolean) TO board_verifier;

-- The issuer authenticates the answer; the public writer receives only an
-- opaque receipt, consumed later in the protected write's transaction.
CREATE FUNCTION content.verification_check_challenge(
    p_token bytea,p_answer bytea,p_peer bytea,p_session bytea,p_purpose text,
    p_board text,p_target bigint,p_context bytea,p_receipt bytea
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now timestamptz; v_session_expiry bigint; v_limit integer;
    v_challenge verification_private.challenges%ROWTYPE;
    v_peer verification_private.peers%ROWTYPE; v_unsolved smallint; v_email verification_private.email_requests%ROWTYPE;
BEGIN
    IF p_token IS NULL OR octet_length(p_token)<>32
        OR (p_answer IS NOT NULL AND octet_length(p_answer)<>32)
        OR p_peer IS NULL OR octet_length(p_peer)<>32
        OR p_session IS NULL OR octet_length(p_session)<>32
        OR p_context IS NULL OR octet_length(p_context)<>32
        OR p_receipt IS NULL OR octet_length(p_receipt)<>32
        OR p_purpose IS NULL OR p_target IS NULL THEN
        RAISE EXCEPTION 'Invalid challenge context.' USING ERRCODE='23514';
    END IF;
    IF p_board IS NOT NULL THEN
        PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR KEY SHARE;
        IF NOT FOUND THEN RETURN false; END IF;
    END IF;
    SELECT s.expires_at INTO v_session_expiry FROM post_secrets.anonymous_sessions s
        WHERE s.token_hash=p_session FOR SHARE;
    IF NOT FOUND THEN RETURN false; END IF;
    SELECT receipt_limit INTO v_limit FROM verification_private.policy WHERE singleton FOR UPDATE;
    IF v_limit IS NULL THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
    SELECT p.* INTO v_peer FROM verification_private.peers p WHERE p.peer_hash=p_peer FOR UPDATE;
    IF NOT FOUND THEN RETURN false; END IF;
    SELECT c.* INTO v_challenge FROM verification_private.challenges c WHERE c.token_hash=p_token FOR UPDATE;
    IF NOT FOUND THEN RETURN false; END IF;
    -- A mismatched context cannot burn another browser's pending challenge.
    IF v_challenge.peer_hash<>p_peer OR v_challenge.session_hash<>p_session
        OR v_challenge.purpose<>p_purpose OR v_challenge.board IS DISTINCT FROM p_board
        OR v_challenge.target<>p_target OR v_challenge.context_hash<>p_context THEN
        RETURN false;
    END IF;
    v_now:=clock_timestamp();
    DELETE FROM verification_private.challenges WHERE token_hash=p_token;
    IF v_session_expiry<=extract(epoch FROM v_now)::bigint OR v_challenge.expires_at<=v_now THEN RETURN false; END IF;
    IF p_purpose='email_verify' THEN
        SELECT r.* INTO v_email FROM verification_private.email_requests r WHERE r.token_hash=p_context FOR UPDATE;
        IF NOT FOUND OR v_email.expires_at<=v_now OR v_email.used_at IS NOT NULL OR v_email.failed_attempts>=5
            OR v_email.captcha_failures>=least(v_email.captcha_failure_limit,CASE WHEN v_challenge.suspicious THEN 3 ELSE 6 END) THEN
            RETURN false;
        END IF;
        IF p_answer IS NULL OR v_challenge.answer_hash<>p_answer THEN
            UPDATE verification_private.email_requests SET captcha_failures=least(captcha_failures+1,6) WHERE id=v_email.id;
            RETURN false;
        END IF;
    ELSIF p_answer IS NULL OR v_challenge.answer_hash<>p_answer THEN RETURN false; END IF;
    DELETE FROM verification_private.receipts r WHERE r.token_hash IN (
        SELECT old.token_hash FROM verification_private.receipts old WHERE old.expires_at<=v_now
        ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED);
    IF (SELECT count(*) FROM (SELECT 1 FROM verification_private.receipts LIMIT v_limit) bounded)>=v_limit THEN
        RAISE EXCEPTION 'Verification capacity is unavailable.' USING ERRCODE='54000';
    END IF;
    v_unsolved:=CASE WHEN v_peer.unsolved_until>v_now THEN v_peer.unsolved ELSE 0 END;
    INSERT INTO verification_private.receipts(
        token_hash,peer_hash,session_hash,purpose,board,target,context_hash,from_credit,suspicious,
        unsolved_before,may_grant_credit,created_at,expires_at
    ) VALUES(p_receipt,p_peer,p_session,p_purpose,p_board,p_target,p_context,false,v_challenge.suspicious,
        v_unsolved,p_purpose IN ('thread','reply') AND NOT v_challenge.suspicious AND v_unsolved<2,
        v_now,v_now+interval '30 seconds');
    -- Primary CAPTCHA success removes two unsolved attempts, but qualification
    -- uses the count from before this decrement.
    UPDATE verification_private.peers SET unsolved=greatest(0,v_unsolved-2) WHERE peer_hash=p_peer;
    RETURN true;
END $$;
REVOKE ALL ON FUNCTION content.verification_check_challenge(bytea,bytea,bytea,bytea,text,text,bigint,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_check_challenge(bytea,bytea,bytea,bytea,text,text,bigint,bytea,bytea) TO board_verifier;

RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
