-- Bounded encrypted Pass contacts and delivery-only notice leases.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass PIN recovery'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass PIN recovery'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass PIN recovery'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass PIN recovery'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass PIN recovery'; END IF; END $$;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
ALTER TABLE verification_private.pass_policy ADD COLUMN recovery_limit integer NOT NULL DEFAULT 100000 CHECK(recovery_limit BETWEEN 1 AND 1000000);
CREATE TABLE verification_private.pass_recovery_attempts (
 ticket bytea PRIMARY KEY CHECK(octet_length(ticket)=32),
 peer_hash bytea NOT NULL CHECK(octet_length(peer_hash)=32),
 identifier_hash bytea NOT NULL CHECK(octet_length(identifier_hash)=32),
 recipient_hash bytea NOT NULL CHECK(octet_length(recipient_hash)=32),
 entitlement_id bigint REFERENCES verification_private.pass_entitlements(id) ON DELETE CASCADE,
 credential_version bigint,
 contact_version bigint,
 created_at timestamptz NOT NULL CHECK(isfinite(created_at)),
 expires_at timestamptz NOT NULL CHECK(isfinite(expires_at) AND expires_at>created_at)
);
CREATE INDEX pass_recovery_attempt_peer ON verification_private.pass_recovery_attempts(peer_hash,created_at);
CREATE INDEX pass_recovery_attempt_recipient ON verification_private.pass_recovery_attempts(recipient_hash,created_at);
CREATE INDEX pass_recovery_attempt_expiry ON verification_private.pass_recovery_attempts(expires_at);
CREATE TABLE verification_private.pass_recoveries (
 token_hash bytea PRIMARY KEY CHECK(octet_length(token_hash)=32),
 request_id uuid NOT NULL UNIQUE,
 entitlement_id bigint NOT NULL UNIQUE REFERENCES verification_private.pass_entitlements(id) ON DELETE CASCADE,
 credential_version bigint NOT NULL CHECK(credential_version>0),
 contact_version bigint NOT NULL CHECK(contact_version>0),
 encrypted_payload bytea NOT NULL CHECK(octet_length(encrypted_payload) BETWEEN 29 AND 512),
 created_at timestamptz NOT NULL CHECK(isfinite(created_at)),
 expires_at timestamptz NOT NULL CHECK(isfinite(expires_at) AND expires_at>created_at),
 delivered_at timestamptz CHECK(delivered_at IS NULL OR isfinite(delivered_at)),
 used_at timestamptz CHECK(used_at IS NULL OR isfinite(used_at)),
 retry_after timestamptz NOT NULL CHECK(isfinite(retry_after)),
 attempts integer NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 30),
 lease_hash bytea CHECK(lease_hash IS NULL OR octet_length(lease_hash)=32),
 lease_until timestamptz,
 CHECK((lease_hash IS NULL)=(lease_until IS NULL)),
 CHECK(lease_until IS NULL OR (isfinite(lease_until) AND lease_until<=expires_at))
);
CREATE INDEX pass_recoveries_expiry ON verification_private.pass_recoveries(expires_at);
REVOKE ALL ON verification_private.pass_recovery_attempts,verification_private.pass_recoveries FROM PUBLIC;
CREATE FUNCTION content.pass_begin_recovery(p_identifier bytea,p_recipient bytea,p_peer bytea,p_ticket bytea)
RETURNS TABLE(status text,encrypted_contact bytea) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE stamp timestamptz; maximum integer; account bigint; credentials bigint; contact bigint; encrypted bytea;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Recovery issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_identifier IS NULL OR octet_length(p_identifier)<>32 OR p_recipient IS NULL OR octet_length(p_recipient)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32 OR p_ticket IS NULL OR octet_length(p_ticket)<>32 THEN RAISE EXCEPTION 'Invalid recovery material' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate();
 SELECT recovery_limit INTO STRICT maximum FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 stamp:=clock_timestamp();
 DELETE FROM verification_private.pass_recovery_attempts WHERE ticket IN(SELECT ticket FROM verification_private.pass_recovery_attempts WHERE expires_at<=stamp ORDER BY expires_at LIMIT 100);
 DELETE FROM verification_private.pass_recoveries WHERE token_hash IN(SELECT token_hash FROM verification_private.pass_recoveries WHERE expires_at<=stamp ORDER BY expires_at LIMIT 100);
 IF (SELECT count(*) FROM verification_private.pass_recovery_attempts WHERE peer_hash=p_peer AND created_at>stamp-interval '1 hour')>=3
 OR (SELECT count(*) FROM verification_private.pass_recovery_attempts WHERE recipient_hash=p_recipient AND created_at>stamp-interval '1 hour')>=3
 OR EXISTS(SELECT 1 FROM verification_private.pass_recovery_attempts WHERE identifier_hash=p_identifier AND recipient_hash=p_recipient AND created_at>stamp-interval '60 seconds') THEN RETURN QUERY SELECT 'accepted'::text,NULL::bytea; RETURN; END IF;
 IF (SELECT count(*) FROM verification_private.pass_recovery_attempts)>=maximum OR (SELECT count(*) FROM verification_private.pass_recoveries)>=maximum THEN RETURN QUERY SELECT 'capacity'::text,NULL::bytea; RETURN; END IF;
 SELECT e.id,e.credential_version,c.version,c.encrypted_contact INTO account,credentials,contact,encrypted FROM verification_private.pass_entitlements e JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id WHERE e.token_hash=p_identifier AND c.recipient_hash=p_recipient;
 INSERT INTO verification_private.pass_recovery_attempts(ticket,peer_hash,identifier_hash,recipient_hash,entitlement_id,credential_version,contact_version,created_at,expires_at) VALUES(p_ticket,p_peer,p_identifier,p_recipient,account,credentials,contact,stamp,stamp+interval '1 hour');
 RETURN QUERY SELECT CASE WHEN account IS NULL THEN 'accepted' ELSE 'prepare' END,encrypted;
END $$;
REVOKE ALL ON FUNCTION content.pass_begin_recovery(bytea,bytea,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_begin_recovery(bytea,bytea,bytea,bytea) TO board_verifier;
CREATE FUNCTION content.pass_queue_recovery(p_ticket bytea,p_token bytea,p_request uuid,p_payload bytea)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE attempt verification_private.pass_recovery_attempts%ROWTYPE; stamp timestamptz;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Recovery issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_ticket IS NULL OR octet_length(p_ticket)<>32 OR p_token IS NULL OR octet_length(p_token)<>32 OR p_request IS NULL OR p_payload IS NULL OR octet_length(p_payload) NOT BETWEEN 29 AND 512 THEN RAISE EXCEPTION 'Invalid recovery envelope' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate(); PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 SELECT * INTO attempt FROM verification_private.pass_recovery_attempts WHERE ticket=p_ticket FOR UPDATE;
 stamp:=clock_timestamp();
 IF NOT FOUND OR attempt.entitlement_id IS NULL OR attempt.created_at<=stamp-interval '30 seconds' OR attempt.created_at>stamp THEN RETURN false; END IF;
 PERFORM e.id FROM verification_private.pass_entitlements e JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id WHERE e.id=attempt.entitlement_id AND e.credential_version=attempt.credential_version AND c.version=attempt.contact_version AND c.recipient_hash=attempt.recipient_hash FOR UPDATE OF e,c;
 IF NOT FOUND THEN RETURN false; END IF;
 IF NOT EXISTS(SELECT 1 FROM verification_private.pass_recoveries WHERE entitlement_id=attempt.entitlement_id) AND (SELECT count(*) FROM verification_private.pass_recoveries)>=(SELECT recovery_limit FROM verification_private.pass_policy WHERE singleton) THEN RETURN false; END IF;
 DELETE FROM verification_private.pass_recoveries WHERE entitlement_id=attempt.entitlement_id;
 INSERT INTO verification_private.pass_recoveries(token_hash,request_id,entitlement_id,credential_version,contact_version,encrypted_payload,created_at,expires_at,retry_after) VALUES(p_token,p_request,attempt.entitlement_id,attempt.credential_version,attempt.contact_version,p_payload,stamp,stamp+interval '30 minutes',stamp);
 UPDATE verification_private.pass_recovery_attempts SET entitlement_id=NULL WHERE ticket=p_ticket;
 RETURN true;
END $$;
REVOKE ALL ON FUNCTION content.pass_queue_recovery(bytea,bytea,uuid,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_queue_recovery(bytea,bytea,uuid,bytea) TO board_verifier;
CREATE FUNCTION content.pass_recovery_state(p_token bytea) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Recovery issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_token IS NULL OR octet_length(p_token)<>32 THEN RETURN false; END IF;
 PERFORM verification_private.pass_gate();
 RETURN EXISTS(SELECT 1 FROM verification_private.pass_recoveries r JOIN verification_private.pass_entitlements e ON e.id=r.entitlement_id JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id
 WHERE r.token_hash=p_token AND r.used_at IS NULL AND r.delivered_at IS NOT NULL AND r.expires_at>clock_timestamp() AND r.credential_version=e.credential_version AND r.contact_version=c.version);
END $$;
REVOKE ALL ON FUNCTION content.pass_recovery_state(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_recovery_state(bytea) TO board_verifier;
CREATE FUNCTION content.pass_complete_recovery(p_token bytea,p_pin text) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account bigint;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Recovery issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_token IS NULL OR octet_length(p_token)<>32 OR p_pin IS NULL OR octet_length(p_pin) NOT BETWEEN 40 AND 256 OR p_pin NOT LIKE '$argon2id$v=19$m=19456,t=2,p=1$%' THEN RETURN false; END IF;
 PERFORM verification_private.pass_gate(); PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 SELECT e.id INTO account FROM verification_private.pass_recoveries r JOIN verification_private.pass_entitlements e ON e.id=r.entitlement_id JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id
 WHERE r.token_hash=p_token AND r.used_at IS NULL AND r.delivered_at IS NOT NULL AND r.expires_at>clock_timestamp() AND r.credential_version=e.credential_version AND r.contact_version=c.version FOR UPDATE OF r,e,c;
 IF NOT FOUND THEN RETURN false; END IF;
 UPDATE verification_private.pass_recoveries SET used_at=clock_timestamp() WHERE token_hash=p_token;
 UPDATE verification_private.pass_entitlements SET pin_hash=p_pin WHERE id=account;
 RETURN true;
END $$;
REVOKE ALL ON FUNCTION content.pass_complete_recovery(bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_complete_recovery(bytea,text) TO board_verifier;
CREATE FUNCTION content.pass_claim_recovery_mail(p_lease bytea,p_limit integer)
RETURNS TABLE(request_id text,token_hash bytea,encrypted_payload bytea) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE stamp timestamptz;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_mailer' THEN RAISE EXCEPTION 'Recovery delivery unavailable' USING ERRCODE='42501'; END IF;
 IF p_lease IS NULL OR octet_length(p_lease)<>32 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RAISE EXCEPTION 'Invalid recovery lease' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate(); PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 stamp:=clock_timestamp();
 RETURN QUERY WITH claimed AS (
  SELECT r.token_hash FROM verification_private.pass_recoveries r JOIN verification_private.pass_entitlements e ON e.id=r.entitlement_id JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id
  WHERE r.expires_at>stamp AND r.used_at IS NULL AND r.delivered_at IS NULL AND r.retry_after<=stamp AND r.attempts<30
   AND (r.lease_until IS NULL OR r.lease_until<=stamp) AND r.credential_version=e.credential_version AND r.contact_version=c.version
  ORDER BY r.created_at,r.request_id LIMIT p_limit FOR UPDATE OF r SKIP LOCKED
 ), updated AS (
  UPDATE verification_private.pass_recoveries r SET lease_hash=p_lease,lease_until=least(stamp+interval '60 seconds',r.expires_at),attempts=r.attempts+1
  FROM claimed c WHERE r.token_hash=c.token_hash RETURNING r.request_id,r.token_hash,r.encrypted_payload
 ) SELECT u.request_id::text,u.token_hash,u.encrypted_payload FROM updated u;
END $$;
REVOKE ALL ON FUNCTION content.pass_claim_recovery_mail(bytea,integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_claim_recovery_mail(bytea,integer) TO board_mailer;
CREATE FUNCTION content.pass_finish_recovery_mail(p_request text,p_lease bytea,p_success boolean)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE stamp timestamptz;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_mailer' THEN RAISE EXCEPTION 'Recovery delivery unavailable' USING ERRCODE='42501'; END IF;
 IF p_request IS NULL OR p_request !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' OR p_lease IS NULL OR octet_length(p_lease)<>32 OR p_success IS NULL THEN RETURN false; END IF;
 PERFORM verification_private.pass_gate(); PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 stamp:=clock_timestamp();
 UPDATE verification_private.pass_recoveries SET lease_hash=NULL,lease_until=NULL,
 delivered_at=CASE WHEN p_success THEN stamp ELSE NULL END,retry_after=stamp+interval '60 seconds'
 WHERE request_id=p_request::uuid AND lease_hash=p_lease AND lease_until>stamp AND expires_at>stamp AND delivered_at IS NULL AND used_at IS NULL;
 RETURN FOUND;
END $$;
REVOKE ALL ON FUNCTION content.pass_finish_recovery_mail(text,bytea,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_finish_recovery_mail(text,bytea,boolean) TO board_mailer;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed posting authority'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass report identity changed report profile'; END IF; END $$;
