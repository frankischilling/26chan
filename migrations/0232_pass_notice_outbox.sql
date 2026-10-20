-- Bounded encrypted Pass contacts and delivery-only notice leases.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass notice delivery'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass notice delivery'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass notice delivery'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass notice delivery'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass notice delivery'; END IF; END $$;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
ALTER TABLE verification_private.pass_policy ADD COLUMN mail_outbox_limit integer NOT NULL DEFAULT 100000 CHECK(mail_outbox_limit BETWEEN 1 AND 1000000);
CREATE TABLE verification_private.pass_contacts (
 entitlement_id bigint PRIMARY KEY REFERENCES verification_private.pass_entitlements(id) ON DELETE CASCADE,
 recipient_hash bytea NOT NULL CHECK(octet_length(recipient_hash)=32),
 encrypted_contact bytea NOT NULL CHECK(octet_length(encrypted_contact) BETWEEN 29 AND 512),
 version bigint NOT NULL DEFAULT 1 CHECK(version>0),
 reminded_expiry timestamptz CHECK(reminded_expiry IS NULL OR isfinite(reminded_expiry)),
 expired_expiry timestamptz CHECK(expired_expiry IS NULL OR isfinite(expired_expiry))
);
CREATE INDEX pass_contacts_recipient ON verification_private.pass_contacts(recipient_hash);
CREATE TABLE verification_private.pass_mail_outbox (
 request_id uuid PRIMARY KEY,
 entitlement_id bigint NOT NULL REFERENCES verification_private.pass_contacts(entitlement_id) ON DELETE CASCADE,
 contact_version bigint NOT NULL CHECK(contact_version>0),
 expiry_snapshot timestamptz NOT NULL CHECK(isfinite(expiry_snapshot)),
 notice_kind text NOT NULL CHECK(notice_kind IN('reminder','expired')),
 token_hash bytea NOT NULL CHECK(octet_length(token_hash)=32),
 encrypted_payload bytea NOT NULL CHECK(octet_length(encrypted_payload) BETWEEN 29 AND 512),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 expires_at timestamptz NOT NULL,
 retry_after timestamptz NOT NULL,
 attempts integer NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 60),
 lease_hash bytea CHECK(lease_hash IS NULL OR octet_length(lease_hash)=32),
 lease_until timestamptz,
 UNIQUE(entitlement_id,expiry_snapshot,notice_kind),
 CHECK(isfinite(created_at) AND isfinite(expires_at) AND isfinite(retry_after) AND expires_at>created_at),
 CHECK((lease_hash IS NULL)=(lease_until IS NULL)),
 CHECK(lease_until IS NULL OR (isfinite(lease_until) AND lease_until<=expires_at))
);
CREATE INDEX pass_mail_due ON verification_private.pass_mail_outbox(retry_after,created_at);
REVOKE ALL ON verification_private.pass_contacts,verification_private.pass_mail_outbox FROM PUBLIC;
CREATE FUNCTION verification_private.pass_contact_version() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF NEW.entitlement_id<>OLD.entitlement_id THEN RAISE EXCEPTION 'Contact identity is immutable' USING ERRCODE='23514'; END IF;
 IF NEW.recipient_hash<>OLD.recipient_hash OR NEW.encrypted_contact<>OLD.encrypted_contact THEN NEW.version:=OLD.version+1;
 ELSIF NEW.version<>OLD.version THEN RAISE EXCEPTION 'Contact version is owned by its lifecycle' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_contact_version() FROM PUBLIC;
CREATE TRIGGER pass_contact_version BEFORE UPDATE ON verification_private.pass_contacts FOR EACH ROW EXECUTE FUNCTION verification_private.pass_contact_version();
CREATE FUNCTION verification_private.pass_notice_kind(p_status smallint,p_expiry timestamptz,p_reminded timestamptz,p_expired timestamptz,p_now timestamptz)
RETURNS text LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' AS $$
 SELECT CASE WHEN p_status IS NULL OR p_now IS NULL OR NOT isfinite(p_now) OR p_status NOT IN(0,6) OR p_expiry IS NULL OR NOT isfinite(p_expiry) OR p_expiry>p_now+interval '7 days' OR p_expired IS NOT DISTINCT FROM p_expiry THEN NULL
 WHEN p_expiry<=p_now THEN 'expired' WHEN p_reminded IS DISTINCT FROM p_expiry THEN 'reminder' ELSE NULL END
$$;
REVOKE ALL ON FUNCTION verification_private.pass_notice_kind(smallint,timestamptz,timestamptz,timestamptz,timestamptz) FROM PUBLIC;
CREATE FUNCTION content.pass_due_notices(p_limit integer)
RETURNS TABLE(entitlement_id bigint,identifier_hash bytea,contact_version bigint,recipient_hash bytea,encrypted_contact bytea,expiry timestamptz,notice_kind text)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Notice producer is unavailable' USING ERRCODE='42501'; END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RAISE EXCEPTION 'Invalid notice batch' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate();
 RETURN QUERY SELECT e.id,e.token_hash,c.version,c.recipient_hash,c.encrypted_contact,e.expires_at,n.kind
 FROM verification_private.pass_entitlements e JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id
 CROSS JOIN LATERAL (SELECT verification_private.pass_notice_kind(e.status,e.expires_at,c.reminded_expiry,c.expired_expiry,clock_timestamp()) AS kind) n
 WHERE n.kind IS NOT NULL AND NOT EXISTS(SELECT 1 FROM verification_private.pass_mail_outbox q
  WHERE q.entitlement_id=e.id AND q.contact_version=c.version AND q.expiry_snapshot=e.expires_at AND q.notice_kind=n.kind AND q.expires_at>clock_timestamp())
 ORDER BY e.expires_at,e.id LIMIT p_limit;
END $$;
REVOKE ALL ON FUNCTION content.pass_due_notices(integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_due_notices(integer) TO board_verifier;
CREATE FUNCTION content.pass_queue_notice(p_account bigint,p_version bigint,p_expiry timestamptz,p_kind text,p_request uuid,p_context bytea,p_payload bytea)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE account verification_private.pass_entitlements%ROWTYPE; contact verification_private.pass_contacts%ROWTYPE; stamp timestamptz; maximum integer;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Notice producer is unavailable' USING ERRCODE='42501'; END IF;
 IF p_account IS NULL OR p_account<=0 OR p_version IS NULL OR p_version<=0 OR p_expiry IS NULL OR NOT isfinite(p_expiry)
 OR p_kind IS NULL OR p_kind NOT IN('reminder','expired') OR p_request IS NULL OR p_context IS NULL OR octet_length(p_context)<>32
 OR p_payload IS NULL OR octet_length(p_payload) NOT BETWEEN 29 AND 512 THEN RAISE EXCEPTION 'Invalid notice material' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate();
 SELECT mail_outbox_limit INTO STRICT maximum FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 SELECT * INTO account FROM verification_private.pass_entitlements WHERE id=p_account FOR UPDATE;
 IF NOT FOUND THEN RETURN 'stale'; END IF;
 SELECT * INTO contact FROM verification_private.pass_contacts WHERE entitlement_id=p_account FOR UPDATE;
 stamp:=clock_timestamp();
 IF NOT FOUND OR contact.version<>p_version OR account.expires_at IS DISTINCT FROM p_expiry
 OR verification_private.pass_notice_kind(account.status,account.expires_at,contact.reminded_expiry,contact.expired_expiry,stamp) IS DISTINCT FROM p_kind THEN RETURN 'stale'; END IF;
 DELETE FROM verification_private.pass_mail_outbox WHERE request_id IN(
  SELECT request_id FROM verification_private.pass_mail_outbox WHERE expires_at<=stamp ORDER BY expires_at LIMIT 100);
 DELETE FROM verification_private.pass_mail_outbox WHERE entitlement_id=p_account
 AND (contact_version<>p_version OR expiry_snapshot<>p_expiry OR notice_kind<>p_kind);
 IF EXISTS(SELECT 1 FROM verification_private.pass_mail_outbox WHERE entitlement_id=p_account AND expiry_snapshot=p_expiry AND notice_kind=p_kind) THEN RETURN 'queued'; END IF;
 IF (SELECT count(*) FROM verification_private.pass_mail_outbox)>=maximum THEN RETURN 'capacity'; END IF;
 INSERT INTO verification_private.pass_mail_outbox(request_id,entitlement_id,contact_version,expiry_snapshot,notice_kind,token_hash,encrypted_payload,created_at,expires_at,retry_after)
 VALUES(p_request,p_account,p_version,p_expiry,p_kind,p_context,p_payload,stamp,stamp+interval '1 hour',stamp);
 RETURN 'queued';
END $$;
REVOKE ALL ON FUNCTION content.pass_queue_notice(bigint,bigint,timestamptz,text,uuid,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_queue_notice(bigint,bigint,timestamptz,text,uuid,bytea,bytea) TO board_verifier;
CREATE FUNCTION content.pass_claim_mail(p_lease bytea,p_limit integer)
RETURNS TABLE(request_id text,token_hash bytea,encrypted_payload bytea)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE stamp timestamptz;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_mailer' THEN RAISE EXCEPTION 'Notice delivery is unavailable' USING ERRCODE='42501'; END IF;
 IF p_lease IS NULL OR octet_length(p_lease)<>32 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RAISE EXCEPTION 'Invalid notice lease' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate();
 PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 stamp:=clock_timestamp();
 DELETE FROM verification_private.pass_mail_outbox q WHERE q.request_id IN(
  SELECT old.request_id FROM verification_private.pass_mail_outbox old JOIN verification_private.pass_entitlements e ON e.id=old.entitlement_id
  JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id
  WHERE old.expires_at<=stamp OR old.contact_version<>c.version OR old.expiry_snapshot IS DISTINCT FROM e.expires_at
   OR verification_private.pass_notice_kind(e.status,e.expires_at,c.reminded_expiry,c.expired_expiry,stamp) IS DISTINCT FROM old.notice_kind
  ORDER BY old.created_at LIMIT 100);
 RETURN QUERY WITH claimed AS (
  SELECT q.request_id FROM verification_private.pass_mail_outbox q JOIN verification_private.pass_entitlements e ON e.id=q.entitlement_id
  JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id
  WHERE q.expires_at>stamp AND q.retry_after<=stamp AND q.attempts<60 AND (q.lease_until IS NULL OR q.lease_until<=stamp)
   AND q.contact_version=c.version AND q.expiry_snapshot=e.expires_at
   AND verification_private.pass_notice_kind(e.status,e.expires_at,c.reminded_expiry,c.expired_expiry,stamp)=q.notice_kind
  ORDER BY q.created_at,q.request_id LIMIT p_limit FOR UPDATE OF q SKIP LOCKED
 ), updated AS (
  UPDATE verification_private.pass_mail_outbox q SET lease_hash=p_lease,lease_until=least(stamp+interval '60 seconds',q.expires_at),attempts=q.attempts+1
  FROM claimed c WHERE q.request_id=c.request_id RETURNING q.request_id,q.token_hash,q.encrypted_payload
 ) SELECT u.request_id::text,u.token_hash,u.encrypted_payload FROM updated u;
END $$;
REVOKE ALL ON FUNCTION content.pass_claim_mail(bytea,integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_claim_mail(bytea,integer) TO board_mailer;
CREATE FUNCTION content.pass_finish_mail(p_request text,p_lease bytea,p_success boolean)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE queued verification_private.pass_mail_outbox%ROWTYPE; stamp timestamptz;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_mailer' THEN RAISE EXCEPTION 'Notice delivery is unavailable' USING ERRCODE='42501'; END IF;
 IF p_request IS NULL OR p_request !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR p_lease IS NULL OR octet_length(p_lease)<>32 OR p_success IS NULL THEN RETURN false; END IF;
 PERFORM verification_private.pass_gate();
 PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 SELECT * INTO queued FROM verification_private.pass_mail_outbox WHERE request_id=p_request::uuid FOR UPDATE;
 stamp:=clock_timestamp();
 IF NOT FOUND OR queued.lease_hash IS DISTINCT FROM p_lease OR queued.lease_until<=stamp OR queued.expires_at<=stamp THEN RETURN false; END IF;
 IF p_success THEN
  -- A delayed acknowledgement cannot expire a renewed or changed entitlement.
  PERFORM id FROM verification_private.pass_entitlements WHERE id=queued.entitlement_id FOR UPDATE;
  UPDATE verification_private.pass_contacts c SET
   reminded_expiry=CASE WHEN queued.notice_kind='reminder' THEN queued.expiry_snapshot ELSE c.reminded_expiry END,
   expired_expiry=CASE WHEN queued.notice_kind='expired' THEN queued.expiry_snapshot ELSE c.expired_expiry END
  FROM verification_private.pass_entitlements e WHERE c.entitlement_id=queued.entitlement_id AND e.id=c.entitlement_id
   AND c.version=queued.contact_version AND e.expires_at=queued.expiry_snapshot AND e.status IN(0,6);
  IF FOUND AND queued.notice_kind='expired' AND queued.expiry_snapshot<=stamp THEN
   UPDATE verification_private.pass_entitlements SET status=1 WHERE id=queued.entitlement_id AND expires_at=queued.expiry_snapshot AND status IN(0,6);
  END IF;
  DELETE FROM verification_private.pass_mail_outbox WHERE request_id=queued.request_id;
 ELSE
  UPDATE verification_private.pass_mail_outbox SET lease_hash=NULL,lease_until=NULL,retry_after=stamp+interval '60 seconds' WHERE request_id=queued.request_id;
 END IF;
 RETURN true;
END $$;
REVOKE ALL ON FUNCTION content.pass_finish_mail(text,bytea,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_finish_mail(text,bytea,boolean) TO board_mailer;
CREATE FUNCTION content.pass_mail_ready() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT content.public_pass_ready()
$$;
REVOKE ALL ON FUNCTION content.pass_mail_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_mail_ready() TO board_mailer;
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
