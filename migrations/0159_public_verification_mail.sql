-- Bounded encrypted delivery queue. Only the issuer can enqueue and only the
-- separate mail worker can claim or acknowledge delivery.
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION content.verification_queue_email(
    p_token bytea,p_email bytea,p_recipient bytea,p_peer bytea,p_session bytea,
    p_country text,p_payload bytea,p_receipt bytea
) RETURNS TABLE(status text,cooldown_seconds integer)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now timestamptz; v_expiry bigint; v_policy verification_private.policy%ROWTYPE;
    v_receipt verification_private.receipts%ROWTYPE; v_latest timestamptz;
    v_first timestamptz; v_count integer; v_id uuid;
BEGIN
    IF p_token IS NULL OR octet_length(p_token)<>32 OR p_email IS NULL OR octet_length(p_email)<>32
        OR p_recipient IS NULL OR octet_length(p_recipient)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
        OR p_session IS NULL OR octet_length(p_session)<>32 OR p_receipt IS NULL OR octet_length(p_receipt)<>32
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
        OR v_receipt.context_hash<>p_recipient OR v_receipt.from_credit THEN
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
    SELECT max(created_at) INTO v_latest FROM verification_private.email_requests WHERE peer_hash=p_peer;
    IF v_latest>v_now-interval '5 minutes' THEN
        RETURN QUERY SELECT 'cooldown'::text,ceil(extract(epoch FROM v_latest+interval '5 minutes'-v_now))::integer; RETURN;
    END IF;
    IF EXISTS(SELECT 1 FROM verification_private.mail_outbox WHERE recipient_hash=p_recipient) THEN
        RETURN QUERY SELECT 'already_queued'::text,0; RETURN;
    END IF;
    SELECT count(*),min(created_at) INTO v_count,v_first FROM (
        SELECT created_at FROM verification_private.email_requests WHERE peer_hash=p_peer
        AND created_at>v_now-interval '1 hour' ORDER BY created_at LIMIT 2
    ) requests;
    IF v_count>=2 THEN
        RETURN QUERY SELECT 'cooldown'::text,ceil(extract(epoch FROM v_first+interval '1 hour'-v_now))::integer; RETURN;
    END IF;
    SELECT count(*),min(created_at) INTO v_count,v_first FROM (
        SELECT created_at FROM verification_private.email_requests WHERE email_hash=p_email
        AND created_at>v_now-interval '24 hours' ORDER BY created_at LIMIT 3
    ) requests;
    IF v_count>=3 THEN
        RETURN QUERY SELECT 'cooldown'::text,ceil(extract(epoch FROM v_first+interval '24 hours'-v_now))::integer; RETURN;
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
REVOKE ALL ON FUNCTION content.verification_queue_email(bytea,bytea,bytea,bytea,bytea,text,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_queue_email(bytea,bytea,bytea,bytea,bytea,text,bytea,bytea) TO board_verifier;

CREATE FUNCTION content.verification_claim_mail(p_lease bytea,p_limit integer)
RETURNS TABLE(request_id text,token_hash bytea,encrypted_payload bytea)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now timestamptz;
BEGIN
    IF p_lease IS NULL OR octet_length(p_lease)<>32 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
        RAISE EXCEPTION 'Invalid delivery batch.' USING ERRCODE='23514';
    END IF;
    -- Share the short queue mutation lock with enqueue/acknowledgment. Never
    -- keep it across filesystem or external delivery work.
    PERFORM singleton FROM verification_private.policy WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
    v_now:=clock_timestamp();
    DELETE FROM verification_private.mail_outbox q WHERE q.request_id IN (
        SELECT old.request_id FROM verification_private.mail_outbox old WHERE old.expires_at<=v_now
        ORDER BY old.expires_at LIMIT 100 FOR UPDATE SKIP LOCKED);
    RETURN QUERY WITH claimed AS (
        SELECT q.request_id FROM verification_private.mail_outbox q
        WHERE q.expires_at>v_now AND q.retry_after<=v_now AND q.attempts<60
          AND (q.lease_until IS NULL OR q.lease_until<=v_now)
        ORDER BY q.created_at,q.request_id LIMIT p_limit FOR UPDATE SKIP LOCKED
    ), updated AS (
        UPDATE verification_private.mail_outbox q SET lease_hash=p_lease,
            lease_until=least(v_now+interval '60 seconds',q.expires_at),attempts=q.attempts+1
        FROM claimed c WHERE q.request_id=c.request_id RETURNING q.request_id,q.encrypted_payload
    ) SELECT u.request_id::text,r.token_hash,u.encrypted_payload FROM updated u
      JOIN verification_private.email_requests r ON r.id=u.request_id;
END $$;
REVOKE ALL ON FUNCTION content.verification_claim_mail(bytea,integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_claim_mail(bytea,integer) TO board_mailer;

CREATE FUNCTION content.verification_finish_mail(p_request text,p_lease bytea,p_success boolean)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now timestamptz; v_queue verification_private.mail_outbox%ROWTYPE;
BEGIN
    IF p_request IS NULL OR p_request !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        OR p_lease IS NULL OR octet_length(p_lease)<>32 OR p_success IS NULL THEN RETURN false; END IF;
    PERFORM singleton FROM verification_private.policy WHERE singleton FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
    SELECT q.* INTO v_queue FROM verification_private.mail_outbox q WHERE q.request_id=p_request::uuid FOR UPDATE;
    v_now:=clock_timestamp();
    IF NOT FOUND OR v_queue.lease_hash IS DISTINCT FROM p_lease OR v_queue.lease_until<=v_now
        OR v_queue.expires_at<=v_now THEN RETURN false; END IF;
    IF p_success THEN
        UPDATE verification_private.email_requests SET delivered_at=v_now WHERE id=v_queue.request_id;
        DELETE FROM verification_private.mail_outbox WHERE request_id=v_queue.request_id;
    ELSE
        UPDATE verification_private.mail_outbox SET lease_hash=NULL,lease_until=NULL,
            retry_after=v_now+interval '60 seconds' WHERE request_id=v_queue.request_id;
    END IF;
    RETURN true;
END $$;
REVOKE ALL ON FUNCTION content.verification_finish_mail(text,bytea,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_finish_mail(text,bytea,boolean) TO board_mailer;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
