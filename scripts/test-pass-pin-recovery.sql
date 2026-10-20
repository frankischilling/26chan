-- Owned placeholders test private authority, without sending mail.
BEGIN;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
 VALUES(sha256('recovery-owned'),repeat('owned-placeholder',3),4,clock_timestamp()-interval '1 year',clock_timestamp()+interval '1 year');
INSERT INTO verification_private.pass_contacts(entitlement_id,recipient_hash,encrypted_contact)
 SELECT id,sha256('recovery-owner'),decode(repeat('02',40),'hex') FROM verification_private.pass_entitlements WHERE token_hash=sha256('recovery-owned');
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF (SELECT status FROM content.pass_begin_recovery(sha256('missing'),sha256('missing-owner'),sha256('missing-peer'),sha256('missing-ticket')))<>'accepted' THEN RAISE EXCEPTION 'Unknown recovery differs'; END IF;
 IF (SELECT status FROM content.pass_begin_recovery(sha256('recovery-owned'),sha256('wrong-owner'),sha256('wrong-peer'),sha256('wrong-ticket')))<>'accepted' THEN RAISE EXCEPTION 'Wrong owner recovery differs'; END IF;
 IF (SELECT status FROM content.pass_begin_recovery(sha256('recovery-owned'),sha256('recovery-owner'),sha256('owned-peer'),sha256('owned-ticket')))<>'prepare' THEN RAISE EXCEPTION 'Owned recovery did not prepare'; END IF;
 IF (SELECT status FROM content.pass_begin_recovery(sha256('recovery-owned'),sha256('recovery-owner'),sha256('other-peer'),sha256('duplicate-ticket')))<>'accepted' THEN RAISE EXCEPTION 'Recovery throttle differs'; END IF;
 IF NOT content.pass_queue_recovery(sha256('owned-ticket'),sha256('owned-reset'),'00000000-0000-4000-8000-000000000088',decode(repeat('03',40),'hex')) THEN RAISE EXCEPTION 'Owned recovery queue failed'; END IF;
 IF content.pass_queue_recovery(sha256('owned-ticket'),sha256('replayed-reset'),'00000000-0000-4000-8000-000000000087',decode(repeat('03',40),'hex')) THEN RAISE EXCEPTION 'Recovery preparation replayed'; END IF;
 IF content.pass_recovery_state(sha256('owned-reset')) THEN RAISE EXCEPTION 'Undelivered recovery became authority'; END IF;
END $$;
SET LOCAL ROLE board_mailer;
DO $$ BEGIN
 IF (SELECT count(*) FROM content.pass_claim_recovery_mail(sha256('first-lease'),1))<>1 THEN RAISE EXCEPTION 'Recovery lease failed'; END IF;
 IF content.pass_finish_recovery_mail('00000000-0000-4000-8000-000000000088',sha256('wrong-lease'),true) THEN RAISE EXCEPTION 'Wrong recovery lease acknowledged'; END IF;
 IF NOT content.pass_finish_recovery_mail('00000000-0000-4000-8000-000000000088',sha256('first-lease'),false) THEN RAISE EXCEPTION 'Recovery failure not recorded'; END IF;
 IF EXISTS(SELECT 1 FROM content.pass_claim_recovery_mail(sha256('early-retry'),1)) THEN RAISE EXCEPTION 'Recovery retry delay bypassed'; END IF;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF content.pass_recovery_state(sha256('owned-reset')) THEN RAISE EXCEPTION 'Failed delivery granted recovery'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_recoveries SET retry_after=clock_timestamp()-interval '1 second';
SET LOCAL ROLE board_mailer;
SELECT * FROM content.pass_claim_recovery_mail(sha256('second-lease'),1);
SELECT content.pass_finish_recovery_mail('00000000-0000-4000-8000-000000000088',sha256('second-lease'),true);
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF NOT content.pass_recovery_state(sha256('owned-reset')) THEN RAISE EXCEPTION 'Delivered recovery missing'; END IF;
 IF content.pass_complete_recovery(sha256('owned-reset'),'invalid') THEN RAISE EXCEPTION 'Invalid PIN hash accepted'; END IF;
 IF NOT content.pass_complete_recovery(sha256('owned-reset'),'$argon2id$v=19$m=19456,t=2,p=1$'||repeat('a',20)||'$'||repeat('b',43)) THEN RAISE EXCEPTION 'Recovery completion failed'; END IF;
 IF content.pass_complete_recovery(sha256('owned-reset'),'$argon2id$v=19$m=19456,t=2,p=1$'||repeat('a',20)||'$'||repeat('c',43)) OR content.pass_recovery_state(sha256('owned-reset')) THEN RAISE EXCEPTION 'Recovery token replayed'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM verification_private.pass_entitlements WHERE token_hash=sha256('recovery-owned') AND status=4 AND credential_version=2) THEN RAISE EXCEPTION 'Recovery changed restriction or failed credential rotation'; END IF; END $$;
UPDATE verification_private.pass_recovery_attempts SET created_at=created_at-interval '2 minutes';
SET LOCAL ROLE board_verifier;
SELECT * FROM content.pass_begin_recovery(sha256('recovery-owned'),sha256('recovery-owner'),sha256('owned-peer'),sha256('contact-ticket'));
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_contacts SET encrypted_contact=decode(repeat('04',40),'hex');
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF content.pass_queue_recovery(sha256('contact-ticket'),sha256('stale-contact'),'00000000-0000-4000-8000-000000000086',decode(repeat('03',40),'hex')) THEN RAISE EXCEPTION 'Changed contact accepted pending recovery'; END IF; END $$;
SET LOCAL ROLE board_verifier;
SELECT * FROM content.pass_begin_recovery(sha256('rate-one'),sha256('rate-owner-one'),sha256('rate-peer'),sha256('rate-ticket-one'));
SELECT * FROM content.pass_begin_recovery(sha256('rate-two'),sha256('rate-owner-two'),sha256('rate-peer'),sha256('rate-ticket-two'));
SELECT * FROM content.pass_begin_recovery(sha256('rate-three'),sha256('rate-owner-three'),sha256('rate-peer'),sha256('rate-ticket-three'));
SELECT * FROM content.pass_begin_recovery(sha256('rate-four'),sha256('rate-owner-four'),sha256('rate-peer'),sha256('rate-ticket-four'));
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT count(*) FROM verification_private.pass_recovery_attempts WHERE peer_hash=sha256('rate-peer'))<>3 THEN RAISE EXCEPTION 'Recovery peer budget differs'; END IF; END $$;
UPDATE verification_private.pass_policy SET recovery_limit=1 WHERE singleton;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_begin_recovery(sha256('capacity-id'),sha256('capacity-owner'),sha256('capacity-peer'),sha256('capacity-ticket')))<>'capacity' THEN RAISE EXCEPTION 'Recovery capacity did not fail closed'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_policy SET recovery_limit=100000 WHERE singleton;
SAVEPOINT expired_token;
UPDATE verification_private.pass_recoveries r SET used_at=NULL,credential_version=e.credential_version,contact_version=c.version,expires_at=clock_timestamp(),created_at=clock_timestamp()-interval '1 hour' FROM verification_private.pass_entitlements e JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id WHERE e.id=r.entitlement_id;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF content.pass_recovery_state(sha256('owned-reset')) THEN RAISE EXCEPTION 'Expired token remained usable'; END IF; END $$;
ROLLBACK TO expired_token;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN PERFORM content.pass_recovery_state(sha256('owned-reset')); RAISE EXCEPTION 'Public recovery authority leaked'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM * FROM verification_private.pass_recoveries; RAISE EXCEPTION 'Public recovery material leaked'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SET LOCAL ROLE board_mailer;
DO $$ BEGIN
 BEGIN PERFORM content.pass_complete_recovery(sha256('owned-reset'),'invalid'); RAISE EXCEPTION 'Mailer changed credential'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
