-- Email verification must retain existing account restrictions. Recipient
-- policy stores keyed hashes only; runtimes cannot enumerate or edit the list.
SET ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction catalog drift before verification admission'; END IF; END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer catalog drift before verification admission'; END IF; END $$;
RESET ROLE;
CREATE TABLE verification_private.blocked_recipients (
 recipient_hash bytea PRIMARY KEY CHECK(octet_length(recipient_hash)=32),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON verification_private.blocked_recipients FROM PUBLIC,board_public,board_staff,board_auth,board_verifier,board_mailer;
GRANT SELECT,TRIGGER ON verification_private.blocked_recipients TO board_anonymous_owner;
GRANT CREATE ON SCHEMA verification_private TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION verification_private.lock_recipient_policy()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_TABLE_SCHEMA<>'verification_private' OR TG_TABLE_NAME<>'blocked_recipients' OR TG_LEVEL<>'ROW' OR TG_WHEN<>'BEFORE'
    OR TG_NARGS<>0 OR TG_OP NOT IN('INSERT','UPDATE','DELETE') THEN
  RAISE EXCEPTION 'Invalid verification policy trigger' USING ERRCODE='23514';
 END IF;
 PERFORM singleton FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable' USING ERRCODE='55000'; END IF;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION verification_private.lock_recipient_policy() FROM PUBLIC;
CREATE TRIGGER verification_recipient_policy_guard BEFORE INSERT OR UPDATE OR DELETE
 ON verification_private.blocked_recipients FOR EACH ROW EXECUTE FUNCTION verification_private.lock_recipient_policy();
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private FROM board_anonymous_owner;
REVOKE TRIGGER ON verification_private.blocked_recipients FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA content TO board_restriction_owner;
SET ROLE board_restriction_owner;
CREATE FUNCTION content.verification_identity_banned(p_identity uuid)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 -- This share lock serializes the verification decision with staff changes.
 -- NOWAIT in gate preserves the existing lock-order failure contract.
 PERFORM restrictions.gate(false);
 RETURN p_identity IS NOT NULL AND EXISTS(SELECT 1 FROM restrictions.bans b
   WHERE b.active AND b.automatic_identity=p_identity AND NOT b.warning
     AND b.starts_at<=clock_timestamp() AND (b.ends_at IS NULL OR b.ends_at>clock_timestamp()));
END $$;
REVOKE ALL ON FUNCTION content.verification_identity_banned(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_identity_banned(uuid) TO board_anonymous_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_restriction_owner;
SET ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction catalog did not qualify after verification admission'; END IF; END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION content.verification_queue_email(
    p_token bytea,p_email bytea,p_recipient bytea,p_peer bytea,p_session bytea,
    p_country text,p_payload bytea,p_receipt bytea,p_context bytea
) RETURNS TABLE(status text,cooldown_seconds integer)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now timestamptz; v_expiry bigint; v_policy verification_private.policy%ROWTYPE;
    v_receipt verification_private.receipts%ROWTYPE; v_advice text; v_cooldown integer; v_id uuid;
BEGIN
    IF p_token IS NULL OR octet_length(p_token)<>32 OR p_email IS NULL OR octet_length(p_email)<>32
        OR p_recipient IS NULL OR octet_length(p_recipient)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
        OR p_session IS NULL OR octet_length(p_session)<>32 OR p_receipt IS NULL OR octet_length(p_receipt)<>32
        OR p_context IS NULL OR octet_length(p_context)<>32
        OR p_country IS NULL OR p_country !~ '^[A-Z]{2}$'
        OR p_payload IS NULL OR octet_length(p_payload) NOT BETWEEN 98 AND 352 THEN
        RAISE EXCEPTION 'Invalid email request.' USING ERRCODE='23514';
    END IF;
    SELECT s.expires_at INTO v_expiry FROM post_secrets.anonymous_sessions s WHERE s.token_hash=p_session FOR SHARE;
    IF NOT FOUND THEN RETURN QUERY SELECT 'authorization_changed'::text,0; RETURN; END IF;
    SELECT p.* INTO v_policy FROM verification_private.policy p WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
    SELECT r.* INTO v_receipt FROM verification_private.receipts r WHERE r.token_hash=p_receipt FOR UPDATE;
    v_now:=clock_timestamp();
    IF NOT FOUND OR v_receipt.expires_at<=v_now OR v_expiry<=extract(epoch FROM v_now)::bigint
        OR v_receipt.peer_hash<>p_peer OR v_receipt.session_hash<>p_session
        OR v_receipt.purpose<>'email_request' OR v_receipt.board IS NOT NULL OR v_receipt.target<>0
        OR v_receipt.context_hash<>p_context OR v_receipt.from_credit THEN
        RETURN QUERY SELECT 'challenge_required'::text,0; RETURN;
    END IF;
    DELETE FROM verification_private.mail_outbox q WHERE q.request_id IN (
        SELECT old.request_id FROM verification_private.mail_outbox old WHERE old.expires_at<=v_now
        ORDER BY old.expires_at LIMIT 100 FOR UPDATE SKIP LOCKED);
    DELETE FROM verification_private.email_requests r WHERE r.id IN (
        SELECT old.id FROM verification_private.email_requests old WHERE old.expires_at<=v_now
        ORDER BY old.expires_at LIMIT 100 FOR UPDATE SKIP LOCKED);
    -- Also prune this exact recipient when a larger expired backlog remains.
    DELETE FROM verification_private.mail_outbox WHERE recipient_hash=p_recipient AND expires_at<=v_now;
    SELECT c.status,c.cooldown_seconds INTO v_advice,v_cooldown
        FROM verification_private.email_cooldown(p_peer,p_email,p_recipient,v_now) c;
    IF v_advice<>'ready' THEN RETURN QUERY SELECT v_advice,v_cooldown; RETURN; END IF;
    -- Preserve the source's indistinguishable success for an active password
    -- ban or exact-recipient blacklist. No token or mail job is created.
    IF EXISTS(SELECT 1 FROM verification_private.blocked_recipients WHERE recipient_hash=p_recipient)
        OR content.verification_identity_banned((SELECT coalesce(b.identity_id,s.automatic_identity)
            FROM post_secrets.anonymous_sessions s LEFT JOIN verification_private.browser_identities b ON b.session_hash=s.token_hash
            WHERE s.token_hash=p_session)) THEN
        DELETE FROM verification_private.receipts WHERE token_hash=p_receipt;
        RETURN QUERY SELECT 'queued'::text,0; RETURN;
    END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM verification_private.email_requests LIMIT v_policy.request_limit) bounded)>=v_policy.request_limit
        OR (SELECT count(*) FROM (SELECT 1 FROM verification_private.mail_outbox LIMIT v_policy.outbox_limit) bounded)>=v_policy.outbox_limit THEN
        RAISE EXCEPTION 'Verification capacity is unavailable.' USING ERRCODE='54000';
    END IF;
    INSERT INTO verification_private.email_requests(token_hash,email_hash,recipient_hash,peer_hash,country,captcha_failure_limit,created_at,expires_at)
        VALUES(p_token,p_email,p_recipient,p_peer,p_country,CASE WHEN v_receipt.suspicious THEN 3 ELSE 6 END,v_now,v_now+interval '24 hours') RETURNING id INTO v_id;
    INSERT INTO verification_private.mail_outbox(request_id,recipient_hash,encrypted_payload,created_at,expires_at,retry_after)
        VALUES(v_id,p_recipient,p_payload,v_now,v_now+interval '1 hour',v_now);
    DELETE FROM verification_private.receipts WHERE token_hash=p_receipt;
    RETURN QUERY SELECT 'queued'::text,0;
END $$;
REVOKE ALL ON FUNCTION content.verification_queue_email(bytea,bytea,bytea,bytea,bytea,text,bytea,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_queue_email(bytea,bytea,bytea,bytea,bytea,text,bytea,bytea,bytea) TO board_verifier;

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
    IF r.failed_attempts>=5 OR r.captcha_failures>=least(r.captcha_failure_limit,CASE WHEN c.suspicious THEN 3 ELSE 6 END) THEN RETURN 'invalid'; END IF;
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
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Verification admission changed transfer authority'; END IF; END $$;
RESET ROLE;
