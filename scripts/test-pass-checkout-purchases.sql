-- Owned sandbox values; no real provider or recipient is contacted.
BEGIN;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_checkout_policy SET purchase_enabled=true,purchase_amount_minor=2300,currency='usd';
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF content.pass_begin_purchase('00000000-0000-0000-0000-000000002350',sha256('view'),sha256('peer'),sha256('recipient'),2300,'usd','sandbox_purchase',false,sha256('ticket'))<>'prepare' THEN RAISE EXCEPTION 'Purchase reservation failed'; END IF;
 IF content.pass_begin_purchase('00000000-0000-0000-0000-000000002350',sha256('view'),sha256('peer'),sha256('recipient'),2300,'usd','sandbox_purchase',false,sha256('other'))<>'capacity' THEN RAISE EXCEPTION 'Concurrent preparation was not bounded'; END IF;
 IF content.pass_prepare_purchase('00000000-0000-0000-0000-000000002350',sha256('wrong'),sha256('identity'),'$argon2id$v=19$m=19456,t=2,p=1$owned-salt$owned-hash',decode(repeat('ab',40),'hex'),sha256('renewal'),sha256('setup'),decode(repeat('cd',50),'hex'))<>'stale' THEN RAISE EXCEPTION 'Wrong preparation lease accepted'; END IF;
 IF content.pass_prepare_purchase('00000000-0000-0000-0000-000000002350',sha256('ticket'),sha256('identity'),'$argon2id$v=19$m=19456,t=2,p=1$owned-salt$owned-hash',decode(repeat('ab',40),'hex'),sha256('renewal'),sha256('setup'),decode(repeat('cd',50),'hex'))<>'ready' THEN RAISE EXCEPTION 'Purchase preparation failed'; END IF;
 IF content.pass_purchase_contact('00000000-0000-0000-0000-000000002350',sha256('view')) IS NOT NULL THEN RAISE EXCEPTION 'Unpaid order has account contact'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF EXISTS(SELECT 1 FROM verification_private.pass_entitlements WHERE token_hash=sha256('identity')) THEN RAISE EXCEPTION 'Preparation granted unpaid membership'; END IF; END $$;
SET LOCAL ROLE board_checkout;
DO $$ DECLARE stamp timestamptz:=date_trunc('second',clock_timestamp()); BEGIN
 PERFORM set_config('board.purchase_test_time',stamp::text,true);
 IF content.pass_apply_checkout_event('sandbox','wrong_price','00000000-0000-0000-0000-000000002350','sandbox_purchase','pass_purchase',2301,'usd','paid',stamp,sha256('wrong_price'))<>'refused' THEN RAISE EXCEPTION 'Wrong amount accepted'; END IF;
 IF content.pass_apply_checkout_event('sandbox','purchase_paid','00000000-0000-0000-0000-000000002350','sandbox_purchase','pass_purchase',2300,'usd','paid',stamp,sha256('paid'))<>'applied' THEN RAISE EXCEPTION 'Purchase fulfillment failed'; END IF;
 IF content.pass_apply_checkout_event('sandbox','purchase_paid','00000000-0000-0000-0000-000000002350','sandbox_purchase','pass_purchase',2300,'usd','paid',stamp,sha256('paid'))<>'duplicate' THEN RAISE EXCEPTION 'Purchase paid replay was not idempotent'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF (SELECT count(*) FROM verification_private.pass_entitlements WHERE token_hash=sha256('identity') AND status=6 AND expires_at=current_setting('board.purchase_test_time')::timestamptz+interval '12 months')<>1 THEN RAISE EXCEPTION 'Paid purchase entitlement differs'; END IF;
 IF (SELECT count(*) FROM verification_private.pass_recoveries r JOIN verification_private.pass_contacts c USING(entitlement_id) WHERE r.token_hash=sha256('setup') AND c.recipient_hash=sha256('recipient') AND r.delivered_at IS NULL AND c.renewal_hash=sha256('renewal'))<>1 THEN RAISE EXCEPTION 'Setup outbox/contact was not atomic'; END IF;
 IF EXISTS(SELECT 1 FROM verification_private.pass_checkout_orders WHERE id='00000000-0000-0000-0000-000000002350' AND (new_contact IS NOT NULL OR new_pin_hash IS NOT NULL OR new_setup_mail IS NOT NULL OR NOT fulfilled)) THEN RAISE EXCEPTION 'Fulfillment retained duplicate secrets'; END IF;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF content.pass_purchase_contact('00000000-0000-0000-0000-000000002350',sha256('wrong')) IS NOT NULL OR content.pass_purchase_contact('00000000-0000-0000-0000-000000002350',sha256('view')) IS NULL THEN RAISE EXCEPTION 'Purchase contact view binding differs'; END IF;
END $$;
SET LOCAL ROLE board_checkout;
SELECT content.pass_apply_checkout_event('sandbox','purchase_refund','00000000-0000-0000-0000-000000002350','sandbox_purchase','pass_purchase',2300,'usd','refunded',date_trunc('second',clock_timestamp()),sha256('refund'));
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT status FROM verification_private.pass_entitlements WHERE token_hash=sha256('identity'))<>2 THEN RAISE EXCEPTION 'Refund did not hold purchased membership'; END IF; END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF content.pass_begin_purchase('00000000-0000-0000-0000-000000002351',sha256('view2'),sha256('peer2'),sha256('gift'),2300,'usd','sandbox_delayed',true,sha256('ticket2'))<>'prepare' THEN RAISE EXCEPTION 'Delayed reservation failed'; END IF;
 IF content.pass_prepare_purchase('00000000-0000-0000-0000-000000002351',sha256('ticket2'),sha256('identity2'),'$argon2id$v=19$m=19456,t=2,p=1$owned-salt$owned-hash',decode(repeat('aa',40),'hex'),sha256('renewal2'),sha256('setup2'),decode(repeat('bb',50),'hex'))<>'ready' THEN RAISE EXCEPTION 'Delayed preparation failed'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_policy SET entitlement_limit=(SELECT count(*) FROM verification_private.pass_entitlements) WHERE singleton;
SET LOCAL ROLE board_checkout;
DO $$ BEGIN
 PERFORM set_config('board.purchase_test_time',date_trunc('second',clock_timestamp())::text,true);
 IF content.pass_apply_checkout_event('sandbox','delayed_paid','00000000-0000-0000-0000-000000002351','sandbox_delayed','pass_purchase',2300,'usd','paid',current_setting('board.purchase_test_time')::timestamptz,sha256('delayed'))<>'capacity' THEN RAISE EXCEPTION 'Full account inventory acknowledged a purchase'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM verification_private.pass_checkout_events WHERE event_id='delayed_paid') OR EXISTS(SELECT 1 FROM verification_private.pass_entitlements WHERE token_hash=sha256('identity2')) THEN RAISE EXCEPTION 'Capacity refusal partially fulfilled'; END IF;
END $$;
UPDATE verification_private.pass_policy SET entitlement_limit=50000 WHERE singleton;
SET LOCAL ROLE board_checkout;
DO $$ BEGIN
 IF content.pass_apply_checkout_event('sandbox','delayed_paid','00000000-0000-0000-0000-000000002351','sandbox_delayed','pass_purchase',2300,'usd','paid',current_setting('board.purchase_test_time')::timestamptz,sha256('delayed'))<>'applied' THEN RAISE EXCEPTION 'Retry after capacity restoration failed'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM verification_private.pass_entitlements WHERE token_hash=sha256('identity2') AND status=7 AND expires_at IS NULL) THEN RAISE EXCEPTION 'Delayed purchase started too early'; END IF; END $$;
SET LOCAL ROLE board_checkout;
DO $$ BEGIN
 IF content.pass_apply_checkout_event('sandbox','delayed_refund','00000000-0000-0000-0000-000000002351','sandbox_delayed','pass_purchase',2300,'usd','refunded',date_trunc('second',clock_timestamp()),sha256('delayed_refund'))<>'applied' THEN RAISE EXCEPTION 'Delayed refund failed'; END IF;
 IF content.pass_apply_checkout_event('sandbox','late_paid','00000000-0000-0000-0000-000000002351','sandbox_delayed','pass_purchase',2300,'usd','paid',current_setting('board.purchase_test_time')::timestamptz,sha256('late_paid'))<>'refused' THEN RAISE EXCEPTION 'Late paid event revived refunded gift'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM verification_private.pass_entitlements WHERE token_hash=sha256('identity2') AND status=2 AND expires_at IS NOT NULL) THEN RAISE EXCEPTION 'Delayed refund did not hold membership'; END IF; END $$;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN PERFORM content.pass_purchase_offer(); RAISE EXCEPTION 'Public runtime queried private purchase issuer'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM content.pass_purchase_contact('00000000-0000-0000-0000-000000002350',sha256('view')); RAISE EXCEPTION 'Public runtime decrypted purchase contacts'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SET LOCAL ROLE board_checkout;
DO $$ BEGIN
 BEGIN PERFORM content.pass_purchase_contact('00000000-0000-0000-0000-000000002350',sha256('view')); RAISE EXCEPTION 'Payment adapter read purchase contacts'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
