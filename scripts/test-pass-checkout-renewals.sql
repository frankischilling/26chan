-- Native, owned sandbox transaction fixtures. No external payment is attempted.
BEGIN;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
 VALUES(sha256('checkout-active'),repeat('owned-placeholder',3),0,clock_timestamp()-interval '1 year',clock_timestamp()+interval '20 days'),
 (sha256('checkout-expired'),repeat('owned-placeholder',3),1,clock_timestamp()-interval '2 years',clock_timestamp()-interval '10 days');
INSERT INTO verification_private.pass_contacts(entitlement_id,recipient_hash,encrypted_contact,renewal_hash)
 SELECT id,sha256('checkout-recipient'),decode(repeat('02',40),'hex'),CASE WHEN token_hash=sha256('checkout-active') THEN sha256('active-renewal') ELSE sha256('expired-renewal') END FROM verification_private.pass_entitlements WHERE token_hash IN(sha256('checkout-active'),sha256('checkout-expired'));
SELECT set_config('board.checkout_original',(SELECT expires_at::text FROM verification_private.pass_entitlements WHERE token_hash=sha256('checkout-active')),true);
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF content.pass_create_renewal_order('00000000-0000-4000-8000-000000000071',sha256('active-view'),sha256('active-peer'),sha256('active-renewal'),2000,'usd','owned_active')<>'unavailable' THEN RAISE EXCEPTION 'Disabled checkout accepted order'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_checkout_policy SET enabled=true,amount_minor=2000 WHERE singleton;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF content.pass_create_renewal_order('00000000-0000-4000-8000-000000000071',sha256('active-view'),sha256('active-peer'),sha256('active-renewal'),1999,'usd','owned_active')<>'price_changed' THEN RAISE EXCEPTION 'Underpayment created order'; END IF;
 IF content.pass_create_renewal_order('00000000-0000-4000-8000-000000000071',sha256('active-view'),sha256('active-peer'),sha256('active-renewal'),2000,'eur','owned_active')<>'price_changed' THEN RAISE EXCEPTION 'Wrong currency created order'; END IF;
 IF content.pass_create_renewal_order('00000000-0000-4000-8000-000000000071',sha256('active-view'),sha256('active-peer'),sha256('active-renewal'),2000,'usd','owned_active')<>'created' THEN RAISE EXCEPTION 'Owned renewal not created'; END IF;
 IF content.pass_create_renewal_order('00000000-0000-4000-8000-000000000071',sha256('active-view'),sha256('active-peer'),sha256('active-renewal'),2000,'usd','owned_active')<>'created' THEN RAISE EXCEPTION 'Order retry was not idempotent'; END IF;
 IF EXISTS(SELECT 1 FROM content.pass_checkout_state('00000000-0000-4000-8000-000000000071',sha256('wrong-view'))) THEN RAISE EXCEPTION 'Order view capability ignored'; END IF;
END $$;
SELECT set_config('board.checkout_capture',clock_timestamp()::text,true);
SET LOCAL ROLE board_checkout;
DO $$ BEGIN
 IF content.pass_apply_checkout_event('sandbox','owned_wrong_amount','00000000-0000-4000-8000-000000000071','owned_active','pass_renewal',1999,'usd','paid',current_setting('board.checkout_capture')::timestamptz,sha256('wrong'))<>'refused' THEN RAISE EXCEPTION 'Wrong amount fulfilled'; END IF;
 IF content.pass_apply_checkout_event('sandbox','owned_wrong_product','00000000-0000-4000-8000-000000000071','owned_active','donation',2000,'usd','paid',current_setting('board.checkout_capture')::timestamptz,sha256('wrong'))<>'refused' THEN RAISE EXCEPTION 'Wrong product fulfilled'; END IF;
 IF content.pass_apply_checkout_event('stripe','owned_wrong_provider','00000000-0000-4000-8000-000000000071','owned_active','pass_renewal',2000,'usd','paid',current_setting('board.checkout_capture')::timestamptz,sha256('wrong'))<>'refused' THEN RAISE EXCEPTION 'Wrong provider fulfilled'; END IF;
 IF content.pass_apply_checkout_event('sandbox','owned_paid','00000000-0000-4000-8000-000000000071','owned_active','pass_renewal',2000,'usd','paid',current_setting('board.checkout_capture')::timestamptz,sha256('paid'))<>'applied' THEN RAISE EXCEPTION 'Owned payment failed'; END IF;
 IF content.pass_apply_checkout_event('sandbox','owned_paid','00000000-0000-4000-8000-000000000071','owned_active','pass_renewal',2000,'usd','paid',current_setting('board.checkout_capture')::timestamptz,sha256('paid'))<>'duplicate' THEN RAISE EXCEPTION 'Payment replay not idempotent'; END IF;
 IF content.pass_apply_checkout_event('sandbox','owned_paid','00000000-0000-4000-8000-000000000071','owned_active','pass_renewal',2000,'usd','paid',current_setting('board.checkout_capture')::timestamptz,sha256('changed'))<>'refused' THEN RAISE EXCEPTION 'Changed replay accepted'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT expires_at FROM verification_private.pass_entitlements WHERE token_hash=sha256('checkout-active')) IS DISTINCT FROM current_setting('board.checkout_original')::timestamptz+interval '12 months' THEN RAISE EXCEPTION 'Active renewal did not extend exactly once'; END IF; END $$;
SET LOCAL ROLE board_checkout;
SELECT content.pass_apply_checkout_event('sandbox','owned_refund','00000000-0000-4000-8000-000000000071','owned_active','pass_renewal',2000,'usd','refunded',clock_timestamp(),sha256('refund'));
DO $$ BEGIN IF content.pass_apply_checkout_event('sandbox','late_paid','00000000-0000-4000-8000-000000000071','owned_active','pass_renewal',2000,'usd','paid',current_setting('board.checkout_capture')::timestamptz,sha256('late'))<>'refused' THEN RAISE EXCEPTION 'Late success revived refund'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT status FROM verification_private.pass_entitlements WHERE token_hash=sha256('checkout-active'))<>2 THEN RAISE EXCEPTION 'Refund did not hold entitlement'; END IF; END $$;
SET LOCAL ROLE board_verifier;
SELECT content.pass_create_renewal_order('00000000-0000-4000-8000-000000000072',sha256('expired-view'),sha256('expired-peer'),sha256('expired-renewal'),2000,'usd','owned_expired');
SELECT set_config('board.checkout_expired_capture',clock_timestamp()::text,true);
SET LOCAL ROLE board_checkout;
SELECT content.pass_apply_checkout_event('sandbox','expired_paid','00000000-0000-4000-8000-000000000072','owned_expired','pass_renewal',2000,'usd','paid',current_setting('board.checkout_expired_capture')::timestamptz,sha256('expired-paid'));
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM verification_private.pass_entitlements WHERE token_hash=sha256('checkout-expired') AND status=0 AND expires_at=current_setting('board.checkout_expired_capture')::timestamptz+interval '12 months') THEN RAISE EXCEPTION 'Expired renewal did not begin at capture'; END IF; END $$;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN PERFORM content.pass_checkout_offer(); RAISE EXCEPTION 'Public direct checkout authority leaked'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM * FROM verification_private.pass_checkout_orders; RAISE EXCEPTION 'Public order inventory leaked'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 BEGIN PERFORM content.pass_apply_checkout_event('sandbox','public_paid','00000000-0000-4000-8000-000000000072','owned_expired','pass_renewal',2000,'usd','paid',clock_timestamp(),sha256('unauthorized')); RAISE EXCEPTION 'Issuer granted payment'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SET LOCAL ROLE board_checkout;
DO $$ BEGIN
 BEGIN PERFORM * FROM verification_private.pass_contacts; RAISE EXCEPTION 'Payment adapter read contact'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
