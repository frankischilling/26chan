-- Purpose-bound Pass access to the reconstructed warning/status controller.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass warning receipts'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass warning receipts'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass warning receipts'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass warning receipts'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass warning receipts'; END IF; END $$;
GRANT CREATE ON SCHEMA verification_private,content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
ALTER TABLE verification_private.pass_receipts DROP CONSTRAINT pass_receipts_purpose_check;
ALTER TABLE verification_private.pass_receipts DROP CONSTRAINT pass_receipts_check;
ALTER TABLE verification_private.pass_receipts ADD CONSTRAINT pass_receipts_purpose_check
 CHECK(purpose IN('thread','reply','report','restriction_status','warning_view'));
ALTER TABLE verification_private.pass_receipts ADD CONSTRAINT pass_receipts_check
 CHECK((purpose IN('thread','restriction_status') AND target=0) OR (purpose IN('reply','report','warning_view') AND target>0));
CREATE OR REPLACE FUNCTION content.pass_write_receipt(
 p_pass bytea,p_receipt bytea,p_peer bytea,p_login bytea,p_posting bytea,p_anonymous bytea,
 p_context bytea,p_purpose text,p_board text,p_target bigint
) RETURNS TABLE(status text,purchase_year integer) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE advice record; stamp timestamptz; bound integer; cap integer;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN
  RAISE EXCEPTION 'Membership issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_pass IS NULL OR octet_length(p_pass)<>32 OR p_receipt IS NULL OR octet_length(p_receipt)<>32
  OR p_peer IS NULL OR octet_length(p_peer)<>32 OR p_anonymous IS NULL OR octet_length(p_anonymous)<>32
  OR p_context IS NULL OR octet_length(p_context)<>32 OR (p_login IS NOT NULL AND octet_length(p_login)<>32)
  OR (p_posting IS NOT NULL AND octet_length(p_posting)<>32) OR p_purpose IS NULL OR p_board IS NULL
  OR p_target IS NULL OR NOT ((p_purpose IN('thread','restriction_status') AND p_target=0) OR (p_purpose IN('reply','report','warning_view') AND p_target>0)) THEN
  RAISE EXCEPTION 'Invalid membership receipt context' USING ERRCODE='23514'; END IF;
 PERFORM verification_private.pass_lock_write_scope(p_pass,p_anonymous,p_board);
 SELECT * INTO advice FROM content.pass_posting_state(p_pass,p_posting);
 IF advice.status<>'ready' THEN RETURN QUERY SELECT advice.status::text,NULL::integer; RETURN; END IF;
 stamp:=clock_timestamp();
 DELETE FROM verification_private.pass_receipts WHERE token_hash IN (
  SELECT token_hash FROM verification_private.pass_receipts WHERE expires_at<=stamp ORDER BY expires_at LIMIT 64 FOR UPDATE SKIP LOCKED);
 -- At most eight outstanding requests per authenticated device. Existing
 -- policy bounds the device inventory; receipts expire after thirty seconds.
 SELECT count(*) INTO bound FROM (SELECT 1 FROM verification_private.pass_receipts WHERE pass_session=p_pass LIMIT 8) inventory;
 SELECT receipt_limit INTO cap FROM verification_private.pass_policy WHERE singleton;
 IF bound>=8 OR (SELECT count(*) FROM (SELECT 1 FROM verification_private.pass_receipts LIMIT cap) inventory)>=cap THEN
  RETURN QUERY SELECT 'capacity'::text,NULL::integer; RETURN; END IF;
 INSERT INTO verification_private.pass_receipts VALUES(p_receipt,p_pass,p_anonymous,p_peer,p_login,p_posting,p_context,p_purpose,p_board,p_target,stamp,stamp+interval '30 seconds');
 RETURN QUERY SELECT 'ready'::text,advice.purchase_year::integer;
END $$;
REVOKE ALL ON FUNCTION content.pass_write_receipt(bytea,bytea,bytea,bytea,bytea,bytea,bytea,text,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_write_receipt(bytea,bytea,bytea,bytea,bytea,bytea,bytea,text,text,bigint) TO board_verifier;

CREATE OR REPLACE FUNCTION verification_private.pass_take_receipt(
 p_receipt bytea,p_peer bytea,p_anonymous bytea,p_context bytea,p_purpose text,p_board text,p_target bigint
) RETURNS TABLE(entitlement_id bigint,purchase_year integer) LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE receipt verification_private.pass_receipts%ROWTYPE; stamp timestamptz; account bigint;
BEGIN
 receipt:=verification_private.pass_checked_receipt(p_receipt,p_peer,p_anonymous,p_context,p_purpose,p_board,p_target);
 stamp:=clock_timestamp();
 SELECT s.entitlement_id INTO STRICT account FROM verification_private.pass_sessions s WHERE s.token_hash=receipt.pass_session;
 DELETE FROM verification_private.pass_receipts WHERE token_hash=p_receipt;
 -- Reading or viewing a warning must not extend the posting-network hold.
 IF p_purpose IN('thread','reply','report') THEN
 UPDATE verification_private.pass_entitlements SET last_peer=receipt.peer,login_scope=receipt.login_scope,
  posting_scope=receipt.posting_scope,last_used=date_trunc('second',stamp) WHERE id=account;
 END IF;
 RETURN QUERY SELECT e.id,extract(year FROM e.purchased_at AT TIME ZONE 'America/New_York')::integer
  FROM verification_private.pass_entitlements e WHERE e.id=account;
END $$;
-- This consumes a single, purpose-bound receipt. The stable identity never
-- leaves the private owner boundary; ordinary posting receipts cannot be used.
CREATE FUNCTION verification_private.pass_restriction_identity(p_token bytea,p_board text,p_target bigint,p_purpose text) RETURNS bytea
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE raw text; proof jsonb; key text; account bigint;
BEGIN
 IF p_purpose IS NULL OR NOT ((p_purpose='restriction_status' AND p_target=0)
 OR (p_purpose='warning_view' AND p_target>0)) THEN
 RAISE EXCEPTION 'Invalid membership status purpose' USING ERRCODE='P0166'; END IF;
 raw:=current_setting('board.challenge_context',true);
 IF raw IS NULL OR octet_length(raw) NOT BETWEEN 1 AND 1024 THEN
 RAISE EXCEPTION 'Membership status proof unavailable' USING ERRCODE='P0166'; END IF;
 BEGIN proof:=raw::jsonb;
 EXCEPTION WHEN invalid_text_representation THEN RAISE EXCEPTION 'Invalid membership status proof' USING ERRCODE='P0166'; END;
 IF jsonb_typeof(proof) IS DISTINCT FROM 'object' THEN RAISE EXCEPTION 'Invalid membership status proof' USING ERRCODE='P0166'; END IF;
 IF (SELECT count(*) FROM jsonb_object_keys(proof))<>4 THEN RAISE EXCEPTION 'Invalid membership status proof' USING ERRCODE='P0166'; END IF;
 FOREACH key IN ARRAY ARRAY['receipt','peer','session','context'] LOOP
  IF jsonb_typeof(proof->key) IS DISTINCT FROM 'string' OR octet_length(proof->>key)<>64 OR (proof->>key) !~ '^[0-9a-f]{64}$' THEN
   RAISE EXCEPTION 'Invalid membership status proof' USING ERRCODE='P0166'; END IF;
 END LOOP;
 IF p_token IS NULL OR decode(proof->>'session','hex')<>p_token THEN
 RAISE EXCEPTION 'Membership status session differs' USING ERRCODE='P0166'; END IF;
 SELECT entitlement_id INTO STRICT account FROM verification_private.pass_take_receipt(
 decode(proof->>'receipt','hex'),decode(proof->>'peer','hex'),p_token,decode(proof->>'context','hex'),p_purpose,p_board,p_target);
 RETURN (SELECT token_hash FROM verification_private.pass_entitlements WHERE id=account);
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_restriction_identity(bytea,text,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION verification_private.pass_restriction_identity(bytea,text,bigint,text) TO board_restriction_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private,content FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA content TO board_restriction_owner;
SET LOCAL ROLE board_restriction_owner;
CREATE FUNCTION content.pass_restriction_status(p_actor bytea,p_token bytea,p_board text,p_request bigint)
RETURNS TABLE(id bigint,board text,global boolean,warning boolean,starts_at timestamptz,
 ends_at timestamptz,public_reason text) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_automatic uuid; membership bytea;
BEGIN
 IF restrictions.invoker() NOT IN('board_public','board_migrator') THEN
  RAISE EXCEPTION 'Restriction status is unavailable' USING ERRCODE='42501';
 END IF;
 PERFORM restrictions.gate(false);
 membership:=verification_private.pass_restriction_identity(p_token,p_board,0,'restriction_status');
 PERFORM restrictions.evaluate_qualified(p_actor,p_token,p_board,p_request,membership);
 -- Status explains report restrictions too: valid retained equality applies
 -- during creation-second/idle reset, even though posting ignores it then.
 SELECT a.automatic_identity INTO v_automatic
  FROM post_secrets.lookup_automatic_identity(p_token,p_request) a;
 IF (SELECT count(*) FROM (SELECT 1 FROM restrictions.bans b WHERE b.active
    AND (b.actor_hash=p_actor OR (v_automatic IS NOT NULL AND b.automatic_identity=v_automatic) OR b.pass_identity_hash=membership)
    AND (b.warning OR b.global OR b.board=p_board)
    LIMIT 65) bounded)>64 THEN
  RAISE EXCEPTION 'Restriction status exceeds its bound' USING ERRCODE='P134C';
 END IF;
 RETURN QUERY SELECT b.id,b.board,b.global,b.warning,b.starts_at,b.ends_at,b.public_reason
  FROM restrictions.bans b WHERE b.active
    AND (b.actor_hash=p_actor OR (v_automatic IS NOT NULL AND b.automatic_identity=v_automatic) OR b.pass_identity_hash=membership)
    AND (b.warning OR b.global OR b.board=p_board)
  ORDER BY b.id LIMIT 64;
END $$;

CREATE FUNCTION content.pass_acknowledge_warning(p_actor bytea,p_token bytea,p_board text,p_request bigint,p_id bigint)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_automatic uuid; membership bytea;
BEGIN
 IF restrictions.invoker() NOT IN('board_public','board_migrator') THEN
  RAISE EXCEPTION 'Warning viewing is unavailable' USING ERRCODE='42501';
 END IF;
 IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$'
    OR p_id IS NULL OR p_id<=0 THEN
  RAISE EXCEPTION 'Invalid warning context' USING ERRCODE='23514';
 END IF;
 PERFORM restrictions.gate();
 membership:=verification_private.pass_restriction_identity(p_token,p_board,p_id,'warning_view');
 SELECT a.automatic_identity INTO v_automatic
  FROM post_secrets.lookup_automatic_identity(p_token,p_request) a;
 UPDATE restrictions.bans b SET active=false,retired_at=clock_timestamp(),retirement='warning-viewed'
  WHERE b.id=p_id AND b.active AND b.warning
    AND (b.actor_hash=p_actor OR (v_automatic IS NOT NULL AND b.automatic_identity=v_automatic) OR b.pass_identity_hash=membership);
 IF NOT FOUND THEN RETURN false; END IF;
 INSERT INTO restrictions.audit(ban_id,action) VALUES(p_id,'warning-viewed');
 RETURN true;
END $$;

REVOKE ALL ON FUNCTION content.pass_restriction_status(bytea,bytea,text,bigint),content.pass_acknowledge_warning(bytea,bytea,text,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_restriction_status(bytea,bytea,text,bigint),content.pass_acknowledge_warning(bytea,bytea,text,bigint,bigint) TO board_public;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_restriction_owner;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass warning receipts changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass warning receipts changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass warning receipts changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass warning receipts changed posting authority'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass warning receipts changed report profile'; END IF; END $$;
