-- Public email identities are independent of staff accounts. Browser activity
-- remains browser-local; a successful link rotates the bearer capability.
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION content.verification_email_state(p_token bytea)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE r verification_private.email_requests%ROWTYPE;
BEGIN
    IF p_token IS NULL OR octet_length(p_token)<>32 THEN RETURN 'invalid'; END IF;
    SELECT q.* INTO r FROM verification_private.email_requests q WHERE q.token_hash=p_token;
    IF NOT FOUND THEN RETURN 'invalid'; END IF;
    IF r.used_at IS NOT NULL THEN RETURN 'already_used'; END IF;
    IF r.expires_at<=clock_timestamp() THEN RETURN 'expired'; END IF;
    IF r.failed_attempts>=5 OR r.captcha_failures>=r.captcha_failure_limit THEN RETURN 'invalid'; END IF;
    RETURN 'pending';
END $$;
REVOKE ALL ON FUNCTION content.verification_email_state(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_email_state(bytea) TO board_verifier;

CREATE FUNCTION content.verification_accept_email(
    p_token bytea,p_peer bytea,p_country text,p_receipt bytea,
    p_old bytea,p_old_network bytea,p_old_address bytea,p_old_environment bytea,
    p_new bytea,p_new_network bytea,p_new_address bytea,p_new_environment bytea
) RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE s post_secrets.anonymous_sessions%ROWTYPE; r verification_private.email_requests%ROWTYPE;
    c verification_private.receipts%ROWTYPE; v_now timestamptz; v_second bigint;
    v_last bigint; v_identity uuid; v_identity_limit integer; v_session_limit integer;
BEGIN
    IF p_token IS NULL OR octet_length(p_token)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
        OR p_receipt IS NULL OR octet_length(p_receipt)<>32 OR p_country IS NULL OR p_country !~ '^[A-Z]{2}$'
        OR p_old IS NULL OR octet_length(p_old)<>32 OR p_old_network IS NULL OR octet_length(p_old_network)<>32
        OR p_old_address IS NULL OR octet_length(p_old_address)<>32 OR p_old_environment IS NULL OR octet_length(p_old_environment)<>32
        OR p_new IS NULL OR octet_length(p_new)<>32 OR p_new_network IS NULL OR octet_length(p_new_network)<>32
        OR p_new_address IS NULL OR octet_length(p_new_address)<>32 OR p_new_environment IS NULL OR octet_length(p_new_environment)<>32
        OR p_new=p_old THEN RAISE EXCEPTION 'Invalid verification context.' USING ERRCODE='23514'; END IF;
    SELECT a.* INTO s FROM post_secrets.anonymous_sessions a WHERE a.token_hash=p_old FOR UPDATE;
    IF NOT FOUND THEN RETURN 'authorization_changed'; END IF;
    -- Session capacity precedes verification capacity, including when cleanup
    -- or another browser is allocating a fresh anonymous session.
    SELECT session_limit INTO v_session_limit FROM post_secrets.anonymous_policy WHERE singleton FOR UPDATE;
    SELECT identity_limit INTO v_identity_limit FROM verification_private.policy WHERE singleton FOR UPDATE;
    IF v_session_limit IS NULL OR v_identity_limit IS NULL THEN
        RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000';
    END IF;
    SELECT q.* INTO c FROM verification_private.receipts q WHERE q.token_hash=p_receipt FOR UPDATE;
    v_now:=clock_timestamp(); v_second:=extract(epoch FROM v_now)::bigint;
    IF s.expires_at<=v_second THEN RETURN 'authorization_changed'; END IF;
    IF NOT FOUND OR c.expires_at<=v_now OR c.peer_hash<>p_peer OR c.session_hash<>p_old
        OR c.purpose<>'email_verify' OR c.board IS NOT NULL OR c.target<>0
        OR c.context_hash<>p_token OR c.from_credit THEN RETURN 'challenge_required'; END IF;
    DELETE FROM verification_private.receipts WHERE token_hash=p_receipt;
    SELECT q.* INTO r FROM verification_private.email_requests q WHERE q.token_hash=p_token FOR UPDATE;
    IF NOT FOUND THEN RETURN 'invalid'; END IF;
    IF r.used_at IS NOT NULL THEN RETURN 'already_used'; END IF;
    IF r.expires_at<=v_now THEN RETURN 'expired'; END IF;
    IF r.failed_attempts>=5 OR r.captcha_failures>=least(r.captcha_failure_limit,CASE WHEN c.suspicious THEN 3 ELSE 6 END) THEN RETURN 'invalid'; END IF;
    IF r.country<>p_country THEN
        UPDATE verification_private.email_requests SET failed_attempts=least(failed_attempts+1,5) WHERE id=r.id;
        RETURN 'invalid';
    END IF;
    IF EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions WHERE token_hash=p_new) THEN
        RAISE EXCEPTION 'New browser capability already exists.' USING ERRCODE='28000';
    END IF;
    -- Rotation replaces one live row. Reject a lowered limit that is already
    -- exceeded rather than creating another persistent browser row.
    IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.anonymous_sessions LIMIT v_session_limit+1) bounded)>v_session_limit THEN
        RAISE EXCEPTION 'Anonymous session capacity is unavailable.' USING ERRCODE='54000';
    END IF;
    SELECT i.id INTO v_identity FROM verification_private.identities i WHERE i.email_hash=r.email_hash;
    IF NOT FOUND THEN
        IF (SELECT count(*) FROM (SELECT 1 FROM verification_private.identities LIMIT v_identity_limit) bounded)>=v_identity_limit THEN
            RAISE EXCEPTION 'Verification capacity is unavailable.' USING ERRCODE='54000';
        END IF;
        INSERT INTO verification_private.identities(email_hash) VALUES(r.email_hash) RETURNING id INTO v_identity;
    END IF;
    v_last:=CASE WHEN s.activity_at>0 THEN s.activity_at ELSE s.created_at END;
    IF greatest(0,v_second-v_last)>=604800 THEN
        s.created_at:=v_second; s.network_at:=v_second; s.address_at:=v_second; s.environment_at:=v_second;
        s.action_at:=v_second; s.activity_at:=0; s.verified_level:=0; s.posts:=0; s.images:=0;
        s.threads:=0; s.reports:=0; s.pending:=0; s.change_score:=0;
    ELSE
        IF s.environment_hash<>p_old_environment THEN s.environment_at:=v_second; END IF;
        IF s.network_hash<>p_old_network THEN s.network_at:=v_second; s.address_at:=v_second;
        ELSIF s.address_hash<>p_old_address THEN s.address_at:=v_second; END IF;
    END IF;
    -- The primary controller creates a fresh UserPwd when its resumed state
    -- is already verified. A first verification retains unverified activity.
    IF s.verified_level>0 THEN
        s.created_at:=v_second; s.network_at:=v_second; s.address_at:=v_second; s.environment_at:=v_second;
        s.action_at:=0; s.activity_at:=0; s.posts:=0; s.images:=0; s.threads:=0; s.reports:=0;
        s.pending:=0; s.change_score:=0;
    END IF;
    INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
        created_at,network_at,address_at,environment_at,activity_at,action_at,expires_at,verified_level,
        posts,images,threads,reports,pending,change_score)
    VALUES(p_new,p_new_network,p_new_address,p_new_environment,s.created_at,s.network_at,s.address_at,
        s.environment_at,s.activity_at,s.action_at,v_second+31536000,1,s.posts,s.images,s.threads,
        s.reports,s.pending,s.change_score);
    INSERT INTO verification_private.browser_identities(session_hash,identity_id) VALUES(p_new,v_identity);
    UPDATE verification_private.email_requests SET used_at=v_now WHERE id=r.id;
    -- Revoke the old browser capability. Its anonymous post memberships must
    -- never become ownership of the newly verified email identity.
    DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=p_old;
    RETURN 'verified';
END $$;
REVOKE ALL ON FUNCTION content.verification_accept_email(bytea,bytea,text,bytea,bytea,bytea,bytea,bytea,bytea,bytea,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_accept_email(bytea,bytea,text,bytea,bytea,bytea,bytea,bytea,bytea,bytea,bytea,bytea) TO board_verifier;

CREATE FUNCTION content.verification_signout(p_session bytea)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF p_session IS NULL OR octet_length(p_session)<>32 THEN RETURN false; END IF;
    DELETE FROM post_secrets.anonymous_sessions s WHERE s.token_hash=p_session
        AND EXISTS(SELECT 1 FROM verification_private.browser_identities b WHERE b.session_hash=s.token_hash);
    RETURN FOUND;
END $$;
REVOKE ALL ON FUNCTION content.verification_signout(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_signout(bytea) TO board_verifier;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
