-- Trusted operator risk input. Browser headers never set these values.
CREATE TABLE verification_private.peer_risk (
 peer_hash bytea PRIMARY KEY CHECK(octet_length(peer_hash)=32),
 score smallint NOT NULL CHECK(score BETWEEN 0 AND 100),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 expires_at timestamptz NOT NULL,
 CHECK(expires_at>created_at AND expires_at<=created_at+interval '24 hours')
);
REVOKE ALL ON verification_private.peer_risk FROM PUBLIC,board_public,board_staff,board_auth,board_verifier,board_mailer;
GRANT SELECT,TRIGGER ON verification_private.peer_risk TO board_anonymous_owner;
ALTER TABLE verification_private.email_requests ADD COLUMN bot_score smallint NOT NULL DEFAULT 100 CHECK(bot_score BETWEEN 0 AND 100);
GRANT TRIGGER ON verification_private.email_requests TO board_anonymous_owner;
GRANT CREATE ON SCHEMA verification_private,content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION verification_private.lock_recipient_policy()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_TABLE_SCHEMA<>'verification_private' OR TG_TABLE_NAME NOT IN('blocked_recipients','peer_risk') OR TG_LEVEL<>'ROW' OR TG_WHEN<>'BEFORE'
    OR TG_NARGS<>0 OR TG_OP NOT IN('INSERT','UPDATE','DELETE') THEN
  RAISE EXCEPTION 'Invalid verification policy trigger' USING ERRCODE='23514';
 END IF;
 PERFORM singleton FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable' USING ERRCODE='55000'; END IF;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER verification_peer_risk_guard BEFORE INSERT OR UPDATE OR DELETE
 ON verification_private.peer_risk FOR EACH ROW EXECUTE FUNCTION verification_private.lock_recipient_policy();
CREATE FUNCTION content.verification_risk(p_peer bytea,p_token bytea)
RETURNS TABLE(current_score smallint,token_score smallint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF p_peer IS NULL OR octet_length(p_peer)<>32 OR (p_token IS NOT NULL AND octet_length(p_token)<>32) THEN
  RAISE EXCEPTION 'Invalid verification risk context' USING ERRCODE='23514';
 END IF;
 RETURN QUERY SELECT
  coalesce((SELECT r.score FROM verification_private.peer_risk r WHERE r.peer_hash=p_peer AND r.expires_at>clock_timestamp()),100::smallint),
  coalesce((SELECT q.bot_score FROM verification_private.email_requests q WHERE q.token_hash=p_token),100::smallint);
END $$;
REVOKE ALL ON FUNCTION content.verification_risk(bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_risk(bytea,bytea) TO board_verifier;
CREATE FUNCTION verification_private.snapshot_email_risk()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_TABLE_SCHEMA<>'verification_private' OR TG_TABLE_NAME<>'email_requests' OR TG_LEVEL<>'ROW'
    OR TG_WHEN<>'BEFORE' OR TG_NARGS<>0 OR TG_OP<>'INSERT' THEN
  RAISE EXCEPTION 'Invalid verification risk trigger' USING ERRCODE='23514';
 END IF;
 PERFORM singleton FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable' USING ERRCODE='55000'; END IF;
 SELECT r.current_score INTO NEW.bot_score FROM content.verification_risk(NEW.peer_hash,NULL) r;
 IF NEW.bot_score<80 THEN NEW.captcha_failure_limit:=3; END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION verification_private.snapshot_email_risk() FROM PUBLIC;
CREATE TRIGGER verification_email_risk_snapshot BEFORE INSERT
 ON verification_private.email_requests FOR EACH ROW EXECUTE FUNCTION verification_private.snapshot_email_risk();
CREATE OR REPLACE FUNCTION content.verification_check_challenge(
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
        v_challenge.suspicious:=v_challenge.suspicious OR (SELECT current_score<80 OR token_score<80 FROM content.verification_risk(p_peer,p_context));
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

CREATE OR REPLACE FUNCTION content.verification_accept_email(
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
    IF r.failed_attempts>=5 OR r.captcha_failures>=least(r.captcha_failure_limit,CASE WHEN c.suspicious OR (SELECT current_score<80 OR token_score<80 FROM content.verification_risk(p_peer,p_token)) THEN 3 ELSE 6 END) THEN RETURN 'invalid'; END IF;
    IF r.country<>p_country THEN
        UPDATE verification_private.email_requests SET failed_attempts=least(failed_attempts+1,5) WHERE id=r.id;
        RETURN 'invalid';
    END IF;
    -- Recheck policy at consumption too. A queued link cannot clear a current
    -- identity ban or bypass a blacklist added after delivery.
    IF EXISTS(SELECT 1 FROM verification_private.blocked_recipients WHERE recipient_hash=r.recipient_hash)
        OR content.verification_identity_banned(coalesce((SELECT b.identity_id FROM verification_private.browser_identities b WHERE b.session_hash=p_old),s.automatic_identity))
        OR content.verification_identity_banned((SELECT i.id FROM verification_private.identities i WHERE i.email_hash=r.email_hash)) THEN
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

RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private,content FROM board_anonymous_owner;
REVOKE TRIGGER ON verification_private.peer_risk,verification_private.email_requests FROM board_anonymous_owner;
