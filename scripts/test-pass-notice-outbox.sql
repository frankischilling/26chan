-- Owned encrypted-envelope placeholders exercise SQL authority, not SMTP.
BEGIN;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
 VALUES(sha256('notice-active'),repeat('owned-placeholder',3),0,statement_timestamp()-interval '1 year',statement_timestamp()+interval '6 days'),
 (sha256('notice-expired'),repeat('owned-placeholder',3),6,statement_timestamp()-interval '1 year',statement_timestamp()-interval '1 second'),
 (sha256('notice-distant'),repeat('owned-placeholder',3),0,statement_timestamp()-interval '1 year',statement_timestamp()+interval '8 days');
INSERT INTO verification_private.pass_contacts(entitlement_id,recipient_hash,encrypted_contact)
 SELECT id,sha256('owned-recipient'),decode(repeat('02',40),'hex') FROM verification_private.pass_entitlements WHERE token_hash IN(sha256('notice-active'),sha256('notice-expired'),sha256('notice-distant'));
SELECT set_config('board.notice_expired',(SELECT id::text FROM verification_private.pass_entitlements WHERE token_hash=sha256('notice-expired')),true);
SELECT set_config('board.notice_expiry',(SELECT expires_at::text FROM verification_private.pass_entitlements WHERE token_hash=sha256('notice-expired')),true);
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT count(*) FROM content.pass_due_notices(100))<>2 THEN RAISE EXCEPTION 'Wrong source notice eligibility'; END IF; END $$;
SELECT content.pass_queue_notice(current_setting('board.notice_expired')::bigint,1,current_setting('board.notice_expiry')::timestamptz,'expired','00000000-0000-4000-8000-000000000001',sha256('notice-envelope'),decode(repeat('02',40),'hex'));
DO $$ BEGIN IF (SELECT count(*) FROM content.pass_due_notices(100))<>1 THEN RAISE EXCEPTION 'Queued notice was selected again'; END IF;
 IF content.pass_queue_notice(current_setting('board.notice_expired')::bigint,1,current_setting('board.notice_expiry')::timestamptz,'reminder','00000000-0000-4000-8000-000000000002',sha256('notice-envelope'),decode(repeat('02',40),'hex'))<>'stale' THEN RAISE EXCEPTION 'Wrong notice kind accepted'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_policy SET mail_outbox_limit=1 WHERE singleton;
SELECT set_config('board.notice_active',(SELECT id::text FROM verification_private.pass_entitlements WHERE token_hash=sha256('notice-active')),true);
SELECT set_config('board.notice_active_expiry',(SELECT expires_at::text FROM verification_private.pass_entitlements WHERE token_hash=sha256('notice-active')),true);
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF content.pass_queue_notice(current_setting('board.notice_active')::bigint,1,current_setting('board.notice_active_expiry')::timestamptz,'reminder','00000000-0000-4000-8000-000000000098',sha256('capacity-context'),decode(repeat('02',40),'hex'))<>'capacity' THEN RAISE EXCEPTION 'Notice capacity did not fail closed'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_policy SET mail_outbox_limit=100000 WHERE singleton;
SET LOCAL ROLE board_mailer;
DO $$ BEGIN
 IF content.pass_mail_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Mailer readiness unavailable'; END IF;
 IF (SELECT count(*) FROM content.pass_claim_mail(sha256('lease-one'),1))<>1 THEN RAISE EXCEPTION 'Notice claim missing'; END IF;
 IF (SELECT count(*) FROM content.pass_claim_mail(sha256('lease-two'),1))<>0 THEN RAISE EXCEPTION 'Active lease stolen'; END IF;
 IF content.pass_finish_mail('00000000-0000-4000-8000-000000000001',sha256('wrong-lease'),true) THEN RAISE EXCEPTION 'Wrong lease acknowledged'; END IF;
 IF NOT content.pass_finish_mail('00000000-0000-4000-8000-000000000001',sha256('lease-one'),false) THEN RAISE EXCEPTION 'Failure acknowledgement missing'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF (SELECT status FROM verification_private.pass_entitlements WHERE id=current_setting('board.notice_expired')::bigint)<>6
 OR (SELECT expired_expiry FROM verification_private.pass_contacts WHERE entitlement_id=current_setting('board.notice_expired')::bigint) IS NOT NULL
 OR EXISTS(SELECT 1 FROM verification_private.pass_mail_outbox WHERE lease_hash IS NOT NULL OR retry_after<=clock_timestamp()) THEN RAISE EXCEPTION 'Failed delivery changed entitlement or lost retry'; END IF;
END $$;
UPDATE verification_private.pass_mail_outbox SET retry_after=statement_timestamp()-interval '1 second';
SET LOCAL ROLE board_mailer;
SELECT * FROM content.pass_claim_mail(sha256('lease-two'),1);
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_entitlements SET status=0,expires_at=statement_timestamp()+interval '1 year' WHERE id=current_setting('board.notice_expired')::bigint;
SET LOCAL ROLE board_mailer;
SELECT content.pass_finish_mail('00000000-0000-4000-8000-000000000001',sha256('lease-two'),true);
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT status FROM verification_private.pass_entitlements WHERE id=current_setting('board.notice_expired')::bigint)<>0
 OR (SELECT expired_expiry FROM verification_private.pass_contacts WHERE entitlement_id=current_setting('board.notice_expired')::bigint) IS NOT NULL THEN RAISE EXCEPTION 'Late mail expired a renewal'; END IF; END $$;
UPDATE verification_private.pass_entitlements SET expires_at=current_setting('board.notice_expiry')::timestamptz WHERE id=current_setting('board.notice_expired')::bigint;
SET LOCAL ROLE board_verifier;
SELECT content.pass_queue_notice(current_setting('board.notice_expired')::bigint,1,current_setting('board.notice_expiry')::timestamptz,'expired','00000000-0000-4000-8000-000000000003',sha256('notice-envelope'),decode(repeat('02',40),'hex'));
SET LOCAL ROLE board_mailer;
SELECT * FROM content.pass_claim_mail(sha256('lease-three'),1);
SELECT content.pass_finish_mail('00000000-0000-4000-8000-000000000003',sha256('lease-three'),true);
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT status FROM verification_private.pass_entitlements WHERE id=current_setting('board.notice_expired')::bigint)<>1
 OR (SELECT expired_expiry FROM verification_private.pass_contacts WHERE entitlement_id=current_setting('board.notice_expired')::bigint) IS DISTINCT FROM current_setting('board.notice_expiry')::timestamptz
 OR EXISTS(SELECT 1 FROM verification_private.pass_mail_outbox) THEN RAISE EXCEPTION 'Successful expiry acknowledgement differs'; END IF; END $$;
SAVEPOINT contact_change;
UPDATE verification_private.pass_contacts SET encrypted_contact=decode(repeat('03',40),'hex') WHERE entitlement_id=current_setting('board.notice_expired')::bigint;
DO $$ BEGIN IF (SELECT version FROM verification_private.pass_contacts WHERE entitlement_id=current_setting('board.notice_expired')::bigint)<>2 THEN RAISE EXCEPTION 'Contact change did not advance version'; END IF; END $$;
ROLLBACK TO contact_change;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN PERFORM * FROM content.pass_due_notices(1); RAISE EXCEPTION 'Public notice enumeration succeeded'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM * FROM content.pass_claim_mail(sha256('public'),1); RAISE EXCEPTION 'Public delivery claim succeeded'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SET LOCAL ROLE board_mailer;
DO $$ BEGIN
 BEGIN PERFORM * FROM content.pass_due_notices(1); RAISE EXCEPTION 'Mailer read contacts'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM * FROM verification_private.pass_contacts; RAISE EXCEPTION 'Mailer read private table'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
