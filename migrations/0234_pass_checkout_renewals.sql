-- Bounded encrypted Pass contacts and delivery-only notice leases.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass checkout'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass checkout'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass checkout'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass checkout'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass checkout'; END IF; END $$;
GRANT USAGE ON SCHEMA content TO board_checkout;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
ALTER TABLE verification_private.pass_contacts ADD COLUMN renewal_hash bytea UNIQUE CHECK(renewal_hash IS NULL OR octet_length(renewal_hash)=32);
CREATE TABLE verification_private.pass_checkout_policy (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 enabled boolean NOT NULL DEFAULT false,
 provider text NOT NULL DEFAULT 'sandbox' CHECK(provider IN('sandbox','stripe')),
 amount_minor bigint NOT NULL DEFAULT 0 CHECK(amount_minor BETWEEN 0 AND 100000000),
 currency text NOT NULL DEFAULT 'usd' CHECK(currency ~ '^[a-z]{3}$'),
 order_limit integer NOT NULL DEFAULT 100000 CHECK(order_limit BETWEEN 1 AND 1000000),
 CHECK(NOT enabled OR amount_minor>0)
);
INSERT INTO verification_private.pass_checkout_policy(singleton) VALUES(true);
CREATE TABLE verification_private.pass_checkout_orders (
 id uuid PRIMARY KEY,
 view_hash bytea NOT NULL UNIQUE CHECK(octet_length(view_hash)=32),
 peer_hash bytea NOT NULL CHECK(octet_length(peer_hash)=32),
 renewal_hash bytea NOT NULL CHECK(octet_length(renewal_hash)=32),
 entitlement_id bigint REFERENCES verification_private.pass_entitlements(id) ON DELETE SET NULL,
 provider text NOT NULL CHECK(provider IN('sandbox','stripe')),
 provider_object text NOT NULL CHECK(provider_object ~ '^[A-Za-z0-9_-]{1,255}$'),
 product text NOT NULL CHECK(product='pass_renewal'),
 amount_minor bigint NOT NULL CHECK(amount_minor BETWEEN 1 AND 100000000),
 currency text NOT NULL CHECK(currency ~ '^[a-z]{3}$'),
 state text NOT NULL DEFAULT 'pending' CHECK(state IN('pending','paid','declined','cancelled','refunded','disputed')),
 fulfilled boolean NOT NULL DEFAULT false,
 review_required boolean NOT NULL DEFAULT false,
 created_at timestamptz NOT NULL CHECK(isfinite(created_at)),
 expires_at timestamptz NOT NULL CHECK(isfinite(expires_at) AND expires_at>created_at),
 paid_at timestamptz CHECK(paid_at IS NULL OR isfinite(paid_at)),
 UNIQUE(provider,provider_object)
);
CREATE INDEX pass_checkout_peer ON verification_private.pass_checkout_orders(peer_hash,created_at);
CREATE INDEX pass_checkout_expiry ON verification_private.pass_checkout_orders(expires_at);
CREATE TABLE verification_private.pass_checkout_events (
 provider text NOT NULL CHECK(provider IN('sandbox','stripe')),
 event_id text NOT NULL CHECK(event_id ~ '^[A-Za-z0-9_-]{1,128}$'),
 order_id uuid NOT NULL REFERENCES verification_private.pass_checkout_orders(id) ON DELETE CASCADE,
 fingerprint bytea NOT NULL CHECK(octet_length(fingerprint)=32),
 event_kind text NOT NULL CHECK(event_kind IN('paid','declined','cancelled','refunded','disputed')),
 occurred_at timestamptz NOT NULL CHECK(isfinite(occurred_at)),
 recorded_at timestamptz NOT NULL DEFAULT clock_timestamp() CHECK(isfinite(recorded_at)),
 PRIMARY KEY(provider,event_id),
 UNIQUE(order_id,event_kind)
);
REVOKE ALL ON verification_private.pass_checkout_policy,verification_private.pass_checkout_orders,verification_private.pass_checkout_events FROM PUBLIC;
CREATE FUNCTION content.pass_checkout_ready() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$ SELECT content.public_pass_ready() $$;
REVOKE ALL ON FUNCTION content.pass_checkout_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_checkout_ready() TO board_checkout;
CREATE FUNCTION content.pass_checkout_offer() RETURNS TABLE(enabled boolean,provider text,amount_minor bigint,currency text)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Checkout issuer unavailable' USING ERRCODE='42501'; END IF;
 PERFORM verification_private.pass_gate();
 RETURN QUERY SELECT p.enabled,p.provider,p.amount_minor,p.currency FROM verification_private.pass_checkout_policy p WHERE singleton;
END $$;
REVOKE ALL ON FUNCTION content.pass_checkout_offer() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_checkout_offer() TO board_verifier;
CREATE FUNCTION content.pass_bind_renewal(p_identifier bytea,p_recipient bytea,p_reference bytea) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Checkout issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_identifier IS NULL OR octet_length(p_identifier)<>32 OR p_recipient IS NULL OR octet_length(p_recipient)<>32 OR p_reference IS NULL OR octet_length(p_reference)<>32 THEN RETURN false; END IF;
 PERFORM verification_private.pass_gate(); PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 UPDATE verification_private.pass_contacts c SET renewal_hash=p_reference FROM verification_private.pass_entitlements e WHERE e.id=c.entitlement_id AND e.token_hash=p_identifier AND c.recipient_hash=p_recipient AND (c.renewal_hash IS NULL OR c.renewal_hash=p_reference);
 RETURN FOUND;
END $$;
REVOKE ALL ON FUNCTION content.pass_bind_renewal(bytea,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_bind_renewal(bytea,bytea,bytea) TO board_verifier;
CREATE FUNCTION content.pass_create_renewal_order(p_id uuid,p_view bytea,p_peer bytea,p_reference bytea,p_amount bigint,p_currency text,p_provider_object text)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' AS $$
DECLARE policy verification_private.pass_checkout_policy%ROWTYPE; stamp timestamptz; account bigint; existing verification_private.pass_checkout_orders%ROWTYPE;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Checkout issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_id IS NULL OR p_view IS NULL OR octet_length(p_view)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32 OR p_reference IS NULL OR octet_length(p_reference)<>32 OR p_amount IS NULL OR p_currency IS NULL OR p_provider_object IS NULL OR p_provider_object !~ '^[A-Za-z0-9_-]{1,255}$' THEN RAISE EXCEPTION 'Invalid checkout request' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_gate(); PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 SELECT * INTO existing FROM verification_private.pass_checkout_orders WHERE id=p_id;
 IF FOUND THEN
  IF existing.view_hash=p_view AND existing.renewal_hash=p_reference AND existing.amount_minor=p_amount AND existing.currency=p_currency AND existing.provider_object=p_provider_object THEN RETURN 'created'; ELSE RETURN 'refused'; END IF;
 END IF;
 SELECT * INTO STRICT policy FROM verification_private.pass_checkout_policy WHERE singleton FOR UPDATE;
 IF NOT policy.enabled THEN RETURN 'unavailable'; END IF;
 IF p_amount<>policy.amount_minor OR p_currency<>policy.currency THEN RETURN 'price_changed'; END IF;
 stamp:=clock_timestamp();
 IF (SELECT count(*) FROM verification_private.pass_checkout_orders)>=policy.order_limit THEN RETURN 'capacity'; END IF;
 IF (SELECT count(*) FROM verification_private.pass_checkout_orders WHERE peer_hash=p_peer AND created_at>stamp-interval '1 hour')>=5 THEN RETURN 'flood'; END IF;
 SELECT e.id INTO account FROM verification_private.pass_contacts c JOIN verification_private.pass_entitlements e ON e.id=c.entitlement_id WHERE c.renewal_hash=p_reference AND e.status IN(0,1,6) AND e.expires_at IS NOT NULL FOR UPDATE OF e,c;
 IF NOT FOUND THEN RETURN 'unavailable'; END IF;
 INSERT INTO verification_private.pass_checkout_orders(id,view_hash,peer_hash,renewal_hash,entitlement_id,provider,provider_object,product,amount_minor,currency,created_at,expires_at)
 VALUES(p_id,p_view,p_peer,p_reference,account,policy.provider,p_provider_object,'pass_renewal',policy.amount_minor,policy.currency,stamp,stamp+interval '30 minutes');
 RETURN 'created';
END $$;
REVOKE ALL ON FUNCTION content.pass_create_renewal_order(uuid,bytea,bytea,bytea,bigint,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_create_renewal_order(uuid,bytea,bytea,bytea,bigint,text,text) TO board_verifier;
CREATE FUNCTION content.pass_checkout_state(p_id uuid,p_view bytea)
RETURNS TABLE(state text,provider text,product text,amount_minor bigint,currency text,fulfilled boolean,review_required boolean)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN RAISE EXCEPTION 'Checkout issuer unavailable' USING ERRCODE='42501'; END IF;
 PERFORM verification_private.pass_gate();
 RETURN QUERY SELECT CASE WHEN o.state='pending' AND o.expires_at<=clock_timestamp() THEN 'expired' ELSE o.state END,o.provider,o.product,o.amount_minor,o.currency,o.fulfilled,o.review_required FROM verification_private.pass_checkout_orders o WHERE o.id=p_id AND o.view_hash=p_view;
END $$;
REVOKE ALL ON FUNCTION content.pass_checkout_state(uuid,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_checkout_state(uuid,bytea) TO board_verifier;
CREATE FUNCTION content.pass_apply_checkout_event(p_provider text,p_event text,p_order uuid,p_object text,p_product text,p_amount bigint,p_currency text,p_kind text,p_occurred timestamptz,p_fingerprint bytea)
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
 IF p_kind='paid' THEN
  SELECT * INTO account FROM verification_private.pass_entitlements WHERE id=ordered.entitlement_id FOR UPDATE;
  IF NOT FOUND OR account.status NOT IN(0,1,6) OR account.expires_at IS NULL OR greatest(account.expires_at,p_occurred)>='9999-01-01T00:00:00Z'::timestamptz THEN review:=true;
  ELSE
   UPDATE verification_private.pass_entitlements SET status=0,expires_at=greatest(account.expires_at,p_occurred)+interval '12 months' WHERE id=account.id;
   did_fulfill:=true;
  END IF;
 ELSIF p_kind IN('refunded','disputed') AND ordered.fulfilled THEN
  -- Conservative native payment hold. Never downgrade a content/spam restriction.
  UPDATE verification_private.pass_entitlements SET status=CASE WHEN p_kind='refunded' THEN 2 ELSE 3 END
   WHERE id=ordered.entitlement_id AND status IN(0,1,6);
  did_fulfill:=ordered.fulfilled;
 ELSE did_fulfill:=ordered.fulfilled;
 END IF;
 INSERT INTO verification_private.pass_checkout_events(provider,event_id,order_id,fingerprint,event_kind,occurred_at)
 VALUES(p_provider,p_event,p_order,p_fingerprint,p_kind,p_occurred);
 UPDATE verification_private.pass_checkout_orders SET state=p_kind,fulfilled=did_fulfill OR ordered.fulfilled,review_required=review OR ordered.review_required,
 paid_at=CASE WHEN p_kind='paid' THEN p_occurred ELSE paid_at END WHERE id=p_order;
 RETURN CASE WHEN review THEN 'review' ELSE 'applied' END;
END $$;
REVOKE ALL ON FUNCTION content.pass_apply_checkout_event(text,text,uuid,text,text,bigint,text,text,timestamptz,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_apply_checkout_event(text,text,uuid,text,text,bigint,text,text,timestamptz,bytea) TO board_checkout;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_checkout_classifier_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog SET definition_hash=decode(current_setting('board.migration_checkout_classifier_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_checkout_report_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_checkout_report_hash'),'hex') WHERE singleton;

SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Checkout changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Checkout changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Checkout changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Checkout changed posting authority'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Checkout changed report profile'; END IF; END $$;
