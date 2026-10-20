-- Owned ciphertext-shaped bytes; issuer HTTP tests verify real encryption.
BEGIN;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
VALUES(sha256('expired-renewal-owned'),repeat('owned-phc-placeholder',3),1,now()-interval '2 years',now()-interval '1 day');
INSERT INTO verification_private.pass_contacts(entitlement_id,recipient_hash,encrypted_contact)
SELECT id,sha256('expired-renewal-recipient'),decode(repeat('ab',40),'hex') FROM verification_private.pass_entitlements WHERE token_hash=sha256('expired-renewal-owned');
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF content.pass_expired_contact(sha256('expired-renewal-owned')) IS DISTINCT FROM decode(repeat('ab',40),'hex') THEN RAISE EXCEPTION 'Expired contact lookup differs'; END IF;
 IF content.pass_expired_contact(sha256('unknown-owned')) IS NOT NULL OR content.pass_expired_contact(NULL) IS NOT NULL THEN RAISE EXCEPTION 'Unknown contact returned'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_entitlements SET status=4 WHERE token_hash=sha256('expired-renewal-owned');
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF content.pass_expired_contact(sha256('expired-renewal-owned')) IS NOT NULL THEN RAISE EXCEPTION 'Restricted account exposes contact'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_entitlements SET status=0,expires_at=now()+interval '1 year' WHERE token_hash=sha256('expired-renewal-owned');
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF content.pass_expired_contact(sha256('expired-renewal-owned')) IS NOT NULL THEN RAISE EXCEPTION 'Active account exposes contact'; END IF; END $$;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN PERFORM content.pass_expired_contact(sha256('expired-renewal-owned')); RAISE EXCEPTION 'Public role can read private contacts'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SET LOCAL ROLE board_mailer;
DO $$ BEGIN
 BEGIN PERFORM content.pass_expired_contact(sha256('expired-renewal-owned')); RAISE EXCEPTION 'Mailer can read arbitrary private contacts'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
