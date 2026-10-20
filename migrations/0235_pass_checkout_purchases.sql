-- Sandbox purchase preparation and atomic account/setup-mail fulfillment.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass purchases'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass purchases'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass purchases'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass purchases'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass purchases'; END IF; END $$;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
ALTER TABLE verification_private.pass_checkout_policy ADD COLUMN purchase_enabled boolean NOT NULL DEFAULT false,
 ADD COLUMN purchase_amount_minor bigint NOT NULL DEFAULT 0 CHECK(purchase_amount_minor BETWEEN 0 AND 100000000),
 ADD CHECK(NOT purchase_enabled OR purchase_amount_minor>0);
ALTER TABLE verification_private.pass_checkout_orders RENAME COLUMN renewal_hash TO target_hash;
ALTER TABLE verification_private.pass_checkout_orders DROP CONSTRAINT pass_checkout_orders_product_check;
ALTER TABLE verification_private.pass_checkout_orders ADD CHECK(product IN('pass_renewal','pass_purchase')),
 ADD COLUMN prepared boolean NOT NULL DEFAULT true,
 ADD COLUMN delayed boolean NOT NULL DEFAULT false,
 ADD COLUMN preparation_hash bytea CHECK(preparation_hash IS NULL OR octet_length(preparation_hash)=32),
 ADD COLUMN preparation_until timestamptz CHECK(preparation_until IS NULL OR isfinite(preparation_until)),
 ADD COLUMN preparation_attempts integer NOT NULL DEFAULT 0 CHECK(preparation_attempts BETWEEN 0 AND 5),
 ADD COLUMN new_identity_hash bytea UNIQUE CHECK(new_identity_hash IS NULL OR octet_length(new_identity_hash)=32),
 ADD COLUMN new_pin_hash text CHECK(new_pin_hash IS NULL OR octet_length(new_pin_hash) BETWEEN 40 AND 256),
 ADD COLUMN new_contact bytea CHECK(new_contact IS NULL OR octet_length(new_contact) BETWEEN 29 AND 512),
 ADD COLUMN new_renewal_hash bytea UNIQUE CHECK(new_renewal_hash IS NULL OR octet_length(new_renewal_hash)=32),
 ADD COLUMN new_setup_hash bytea CHECK(new_setup_hash IS NULL OR octet_length(new_setup_hash)=32),
 ADD COLUMN new_setup_mail bytea CHECK(new_setup_mail IS NULL OR octet_length(new_setup_mail) BETWEEN 29 AND 512),
 ADD CHECK((preparation_hash IS NULL)=(preparation_until IS NULL)),
 ADD CHECK(product<>'pass_purchase' OR NOT prepared OR fulfilled OR state IN('cancelled','declined','refunded','disputed') OR
  (new_identity_hash IS NOT NULL AND new_pin_hash IS NOT NULL AND new_contact IS NOT NULL AND new_renewal_hash IS NOT NULL AND new_setup_hash IS NOT NULL AND new_setup_mail IS NOT NULL));
CREATE OR REPLACE FUNCTION content.pass_create_renewal_order(p_id uuid,p_view bytea,p_peer bytea,p_reference bytea,p_amount bigint,p_currency text,p_provider_object text)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' AS $$
DECLARE policy verification_private.pass_checkout_policy%ROWTYPE; stamp timestamptz; account bigint; existing verification_private.pass_checkout_orders%ROWTYPE;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Checkout issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_id IS NULL OR p_view IS NULL OR octet_length(p_view)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32 OR p_reference IS NULL OR octet_length(p_reference)<>32 OR p_amount IS NULL OR p_currency IS NULL OR p_provider_object IS NULL OR p_provider_object !~ '^[A-Za-z0-9_-]{1,255}$' THEN RAISE EXCEPTION 'Invalid checkout request' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate(); PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 SELECT * INTO existing FROM verification_private.pass_checkout_orders WHERE id=p_id;
 IF FOUND THEN
  IF existing.product='pass_renewal' AND existing.view_hash=p_view AND existing.target_hash=p_reference AND existing.amount_minor=p_amount AND existing.currency=p_currency AND existing.provider_object=p_provider_object THEN RETURN 'created'; ELSE RETURN 'refused'; END IF;
 END IF;
 SELECT * INTO STRICT policy FROM verification_private.pass_checkout_policy WHERE singleton FOR UPDATE;
 IF NOT policy.enabled THEN RETURN 'unavailable'; END IF;
 IF p_amount<>policy.amount_minor OR p_currency<>policy.currency THEN RETURN 'price_changed'; END IF;
 stamp:=clock_timestamp();
 IF (SELECT count(*) FROM verification_private.pass_checkout_orders)>=policy.order_limit THEN RETURN 'capacity'; END IF;
 IF (SELECT count(*) FROM verification_private.pass_checkout_orders WHERE peer_hash=p_peer AND created_at>stamp-interval '1 hour')>=5 THEN RETURN 'flood'; END IF;
 SELECT e.id INTO account FROM verification_private.pass_contacts c JOIN verification_private.pass_entitlements e ON e.id=c.entitlement_id WHERE c.renewal_hash=p_reference AND e.status IN(0,1,6) AND e.expires_at IS NOT NULL FOR UPDATE OF e,c;
 IF NOT FOUND THEN RETURN 'unavailable'; END IF;
 INSERT INTO verification_private.pass_checkout_orders(id,view_hash,peer_hash,target_hash,entitlement_id,provider,provider_object,product,amount_minor,currency,created_at,expires_at)
 VALUES(p_id,p_view,p_peer,p_reference,account,policy.provider,p_provider_object,'pass_renewal',policy.amount_minor,policy.currency,stamp,stamp+interval '30 minutes');
 RETURN 'created';
END $$;
CREATE FUNCTION content.pass_purchase_offer() RETURNS TABLE(enabled boolean,provider text,amount_minor bigint,currency text)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Checkout issuer unavailable' USING ERRCODE='42501'; END IF;
 PERFORM verification_private.pass_gate();
 RETURN QUERY SELECT p.purchase_enabled,p.provider,p.purchase_amount_minor,p.currency FROM verification_private.pass_checkout_policy p WHERE singleton;
END $$;
REVOKE ALL ON FUNCTION content.pass_purchase_offer() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_purchase_offer() TO board_verifier;
CREATE FUNCTION content.pass_begin_purchase(p_id uuid,p_view bytea,p_peer bytea,p_recipient bytea,p_amount bigint,p_currency text,p_object text,p_delayed boolean,p_ticket bytea)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE policy verification_private.pass_checkout_policy%ROWTYPE; existing verification_private.pass_checkout_orders%ROWTYPE; stamp timestamptz;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Checkout issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_id IS NULL OR p_view IS NULL OR octet_length(p_view)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32 OR p_recipient IS NULL OR octet_length(p_recipient)<>32 OR p_amount IS NULL OR p_currency IS NULL OR p_object IS NULL OR p_object !~ '^[A-Za-z0-9_-]{1,255}$' OR p_delayed IS NULL OR p_ticket IS NULL OR octet_length(p_ticket)<>32 THEN RAISE EXCEPTION 'Invalid purchase request' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate(); PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 stamp:=clock_timestamp();
 SELECT * INTO existing FROM verification_private.pass_checkout_orders WHERE id=p_id FOR UPDATE;
 IF FOUND THEN
  IF existing.product<>'pass_purchase' OR existing.view_hash<>p_view OR existing.target_hash<>p_recipient OR existing.amount_minor<>p_amount OR existing.currency<>p_currency OR existing.provider_object<>p_object OR existing.delayed<>p_delayed THEN RETURN 'refused'; END IF;
  IF existing.prepared OR existing.state<>'pending' OR existing.expires_at<=stamp THEN RETURN 'created'; END IF;
  IF existing.preparation_until>stamp OR existing.preparation_attempts>=5 THEN RETURN 'capacity'; END IF;
  UPDATE verification_private.pass_checkout_orders SET preparation_hash=p_ticket,preparation_until=stamp+interval '30 seconds',preparation_attempts=preparation_attempts+1 WHERE id=p_id;
  RETURN 'prepare';
 END IF;
 SELECT * INTO STRICT policy FROM verification_private.pass_checkout_policy WHERE singleton FOR UPDATE;
 IF NOT policy.purchase_enabled THEN RETURN 'unavailable'; END IF;
 IF p_amount<>policy.purchase_amount_minor OR p_currency<>policy.currency THEN RETURN 'price_changed'; END IF;
 IF (SELECT count(*) FROM verification_private.pass_checkout_orders)>=policy.order_limit THEN RETURN 'capacity'; END IF;
 IF (SELECT count(*) FROM verification_private.pass_checkout_orders WHERE peer_hash=p_peer AND created_at>stamp-interval '1 hour')>=5 THEN RETURN 'flood'; END IF;
 INSERT INTO verification_private.pass_checkout_orders(id,view_hash,peer_hash,target_hash,provider,provider_object,product,amount_minor,currency,created_at,expires_at,prepared,delayed,preparation_hash,preparation_until,preparation_attempts)
 VALUES(p_id,p_view,p_peer,p_recipient,policy.provider,p_object,'pass_purchase',policy.purchase_amount_minor,policy.currency,stamp,stamp+interval '30 minutes',false,p_delayed,p_ticket,stamp+interval '30 seconds',1);
 RETURN 'prepare';
END $$;
REVOKE ALL ON FUNCTION content.pass_begin_purchase(uuid,bytea,bytea,bytea,bigint,text,text,boolean,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_begin_purchase(uuid,bytea,bytea,bytea,bigint,text,text,boolean,bytea) TO board_verifier;
CREATE FUNCTION content.pass_prepare_purchase(p_id uuid,p_ticket bytea,p_identity bytea,p_pin text,p_contact bytea,p_reference bytea,p_setup bytea,p_mail bytea)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE ordered verification_private.pass_checkout_orders%ROWTYPE; stamp timestamptz;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Checkout issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_id IS NULL OR p_ticket IS NULL OR octet_length(p_ticket)<>32 OR p_identity IS NULL OR octet_length(p_identity)<>32 OR p_pin IS NULL OR octet_length(p_pin) NOT BETWEEN 40 AND 256 OR p_pin NOT LIKE '$argon2id$v=19$m=19456,t=2,p=1$%' OR p_contact IS NULL OR octet_length(p_contact) NOT BETWEEN 29 AND 512 OR p_reference IS NULL OR octet_length(p_reference)<>32 OR p_setup IS NULL OR octet_length(p_setup)<>32 OR p_mail IS NULL OR octet_length(p_mail) NOT BETWEEN 29 AND 512 THEN RAISE EXCEPTION 'Invalid purchase preparation' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate(); PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 SELECT * INTO ordered FROM verification_private.pass_checkout_orders WHERE id=p_id FOR UPDATE;
 stamp:=clock_timestamp();
 IF NOT FOUND OR ordered.product<>'pass_purchase' OR ordered.state<>'pending' OR ordered.prepared OR ordered.expires_at<=stamp OR ordered.preparation_hash IS DISTINCT FROM p_ticket OR ordered.preparation_until<=stamp THEN RETURN 'stale'; END IF;
 IF EXISTS(SELECT 1 FROM verification_private.pass_entitlements WHERE token_hash=p_identity) OR EXISTS(SELECT 1 FROM verification_private.pass_checkout_orders WHERE new_identity_hash=p_identity OR new_renewal_hash=p_reference) OR EXISTS(SELECT 1 FROM verification_private.pass_contacts WHERE renewal_hash=p_reference) THEN RETURN 'collision'; END IF;
 UPDATE verification_private.pass_checkout_orders SET prepared=true,preparation_hash=NULL,preparation_until=NULL,new_identity_hash=p_identity,new_pin_hash=p_pin,new_contact=p_contact,new_renewal_hash=p_reference,new_setup_hash=p_setup,new_setup_mail=p_mail WHERE id=p_id;
 RETURN 'ready';
END $$;
REVOKE ALL ON FUNCTION content.pass_prepare_purchase(uuid,bytea,bytea,text,bytea,bytea,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_prepare_purchase(uuid,bytea,bytea,text,bytea,bytea,bytea,bytea) TO board_verifier;

CREATE OR REPLACE FUNCTION content.pass_apply_checkout_event(p_provider text,p_event text,p_order uuid,p_object text,p_product text,p_amount bigint,p_currency text,p_kind text,p_occurred timestamptz,p_fingerprint bytea)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' AS $$
DECLARE ordered verification_private.pass_checkout_orders%ROWTYPE; previous verification_private.pass_checkout_events%ROWTYPE; account verification_private.pass_entitlements%ROWTYPE; stamp timestamptz; did_fulfill boolean:=false; review boolean:=false;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_checkout' THEN RAISE EXCEPTION 'Payment adapter unavailable' USING ERRCODE='42501'; END IF;
 IF p_provider IS NULL OR p_provider NOT IN('sandbox','stripe') OR p_event IS NULL OR p_event !~ '^[A-Za-z0-9_-]{1,128}$' OR p_order IS NULL OR p_object IS NULL OR p_object !~ '^[A-Za-z0-9_-]{1,255}$' OR p_product IS NULL OR p_amount IS NULL OR p_currency IS NULL OR p_kind IS NULL OR p_kind NOT IN('paid','declined','cancelled','refunded','disputed') OR p_occurred IS NULL OR NOT isfinite(p_occurred) OR p_fingerprint IS NULL OR octet_length(p_fingerprint)<>32 THEN RAISE EXCEPTION 'Invalid payment event' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate(); PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 SELECT * INTO ordered FROM verification_private.pass_checkout_orders WHERE id=p_order FOR UPDATE;
 IF NOT FOUND OR ordered.provider<>p_provider OR ordered.provider_object<>p_object OR ordered.product<>p_product OR ordered.amount_minor<>p_amount OR ordered.currency<>p_currency THEN RETURN 'refused'; END IF;
 stamp:=clock_timestamp();
 IF p_occurred<date_trunc('second',ordered.created_at) OR p_occurred>stamp+interval '30 seconds' OR (p_kind='paid' AND p_occurred>ordered.expires_at) THEN RETURN 'refused'; END IF;
 SELECT * INTO previous FROM verification_private.pass_checkout_events WHERE provider=p_provider AND event_id=p_event;
 IF FOUND THEN
  IF previous.order_id=p_order AND previous.event_kind=p_kind AND previous.fingerprint=p_fingerprint AND previous.occurred_at=p_occurred THEN RETURN 'duplicate'; ELSE RETURN 'refused'; END IF;
 END IF;
 IF ordered.state=p_kind THEN RETURN 'duplicate'; END IF;
 IF ordered.state<>'pending' AND NOT (ordered.state='paid' AND p_kind IN('refunded','disputed')) THEN RETURN 'refused'; END IF;
 IF p_kind='paid' AND ordered.product='pass_purchase' THEN
  IF NOT ordered.prepared THEN RETURN 'refused'; END IF;
  -- Do not acknowledge an event that could not atomically fulfill. The adapter
  -- returns a retryable failure; the same signed event can be replayed safely.
  IF (SELECT count(*) FROM verification_private.pass_entitlements)>=(SELECT entitlement_limit FROM verification_private.pass_policy WHERE singleton)
   OR (SELECT count(*) FROM verification_private.pass_recoveries)>=(SELECT recovery_limit FROM verification_private.pass_policy WHERE singleton) THEN RETURN 'capacity'; END IF;
  INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
   VALUES(ordered.new_identity_hash,ordered.new_pin_hash,CASE WHEN ordered.delayed THEN 7 ELSE 6 END,p_occurred,CASE WHEN ordered.delayed THEN NULL ELSE p_occurred+interval '12 months' END) RETURNING * INTO account;
  INSERT INTO verification_private.pass_contacts(entitlement_id,recipient_hash,encrypted_contact,renewal_hash)
   VALUES(account.id,ordered.target_hash,ordered.new_contact,ordered.new_renewal_hash);
  INSERT INTO verification_private.pass_recoveries(token_hash,request_id,entitlement_id,credential_version,contact_version,encrypted_payload,created_at,expires_at,retry_after)
   VALUES(ordered.new_setup_hash,ordered.id,account.id,account.credential_version,1,ordered.new_setup_mail,stamp,stamp+interval '30 minutes',stamp);
  UPDATE verification_private.pass_checkout_orders SET entitlement_id=account.id WHERE id=p_order;
  did_fulfill:=true;
 ELSIF p_kind='paid' THEN
  SELECT * INTO account FROM verification_private.pass_entitlements WHERE id=ordered.entitlement_id FOR UPDATE;
  IF NOT FOUND OR account.status NOT IN(0,1,6) OR account.expires_at IS NULL OR greatest(account.expires_at,p_occurred)>='9999-01-01T00:00:00Z'::timestamptz THEN review:=true;
  ELSE
   UPDATE verification_private.pass_entitlements SET status=0,expires_at=greatest(account.expires_at,p_occurred)+interval '12 months' WHERE id=account.id;
   did_fulfill:=true;
  END IF;
 ELSIF p_kind IN('refunded','disputed') AND ordered.fulfilled THEN
  -- Conservative native payment hold. Never downgrade a content/spam restriction.
  UPDATE verification_private.pass_entitlements SET status=CASE WHEN p_kind='refunded' THEN 2 ELSE 3 END,expires_at=coalesce(expires_at,p_occurred)
   WHERE id=ordered.entitlement_id AND status IN(0,1,6,7);
  -- A delayed Pass needs a finite expiry when it enters a payment hold.

  did_fulfill:=ordered.fulfilled;
 ELSE did_fulfill:=ordered.fulfilled;
 END IF;
 INSERT INTO verification_private.pass_checkout_events(provider,event_id,order_id,fingerprint,event_kind,occurred_at)
 VALUES(p_provider,p_event,p_order,p_fingerprint,p_kind,p_occurred);
 UPDATE verification_private.pass_checkout_orders SET state=p_kind,fulfilled=did_fulfill OR ordered.fulfilled,review_required=review OR ordered.review_required,
 paid_at=CASE WHEN p_kind='paid' THEN p_occurred ELSE paid_at END,
 new_identity_hash=NULL,new_pin_hash=NULL,new_contact=NULL,new_renewal_hash=NULL,new_setup_hash=NULL,new_setup_mail=NULL,preparation_hash=NULL,preparation_until=NULL WHERE id=p_order;
 RETURN CASE WHEN review THEN 'review' ELSE 'applied' END;
END $$;
CREATE FUNCTION content.pass_purchase_contact(p_id uuid,p_view bytea) RETURNS bytea
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Checkout issuer unavailable' USING ERRCODE='42501'; END IF;
 PERFORM verification_private.pass_gate();
 RETURN (SELECT e.token_hash||c.encrypted_contact FROM verification_private.pass_checkout_orders o JOIN verification_private.pass_contacts c ON c.entitlement_id=o.entitlement_id JOIN verification_private.pass_entitlements e ON e.id=c.entitlement_id WHERE o.id=p_id AND o.view_hash=p_view AND o.product='pass_purchase' AND o.fulfilled);
END $$;
REVOKE ALL ON FUNCTION content.pass_purchase_contact(uuid,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_purchase_contact(uuid,bytea) TO board_verifier;
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
