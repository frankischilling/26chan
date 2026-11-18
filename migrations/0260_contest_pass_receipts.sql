-- Contest voting uses live Pass membership, never a public badge or CAPTCHA.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before site submission proofs'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before site submission proofs'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before site submission proofs'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before site submission proofs'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before site submission proofs'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before site submission proofs'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;

SELECT singleton FROM submission_private.catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Submission authority drifted before contest vote proofs'; END IF; END $$;
RESET ROLE;
GRANT SELECT(board) ON submission_private.contest_entries TO board_submission_owner;
GRANT CREATE ON SCHEMA submission_api TO board_submission_owner,board_anonymous_owner;
SET LOCAL ROLE board_submission_owner;
-- Authorization gates must observe the clock after waiting for proof locks.
DO $$ DECLARE definition text; BEGIN
 definition:=pg_get_functiondef('submission_api.challenge_open(text,bigint)'::regprocedure);
 IF strpos(definition,'statement_timestamp()')=0 THEN RAISE EXCEPTION 'Site proof clock boundary drifted'; END IF;
 EXECUTE replace(definition,'statement_timestamp()','clock_timestamp()');
END $$;
ALTER FUNCTION submission_api.challenge_open(text,bigint) VOLATILE;
CREATE FUNCTION submission_api.vote_campaign(p_entry bigint) RETURNS bigint
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE result bigint; stamp bigint:=floor(extract(epoch FROM clock_timestamp()))::bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_entry IS NULL OR p_entry<=0 THEN RETURN NULL; END IF;
 SELECT c.id INTO result FROM submission_private.contest_entries e
  JOIN submission_private.contest_campaigns c ON c.id=e.campaign_id
  WHERE e.id=p_entry AND e.state=1 AND c.enabled AND e.board=ANY(c.boards)
   AND c.voting_starts<=stamp AND c.voting_ends>stamp AND c.votes<c.vote_capacity;
 RETURN result;
END $$;
REVOKE ALL ON FUNCTION submission_api.vote_campaign(bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.vote_campaign(bigint) TO board_verifier,board_anonymous_owner;
RESET ROLE;
GRANT CREATE ON SCHEMA verification_private,content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
DO $$ DECLARE definition text; anchor text; BEGIN
 definition:=pg_get_functiondef('submission_api.consume_captcha(bytea,bytea,bytea,text,bigint,bytea)'::regprocedure);
 anchor:=' DELETE FROM verification_private.receipts WHERE token_hash=p_receipt;';
 IF strpos(definition,anchor)=0 THEN RAISE EXCEPTION 'Site proof consumption boundary drifted'; END IF;
 EXECUTE replace(definition,anchor,E' IF submission_api.challenge_open(p_purpose,p_target) IS DISTINCT FROM true THEN RAISE EXCEPTION ''Submission workflow is closed'' USING ERRCODE=''P0002''; END IF;\n'||anchor);
END $$;
CREATE TABLE verification_private.pass_site_receipts (
 token_hash bytea PRIMARY KEY CHECK(octet_length(token_hash)=32),
 pass_session bytea NOT NULL REFERENCES verification_private.pass_sessions(token_hash) ON DELETE CASCADE,
 anonymous_session bytea NOT NULL CHECK(octet_length(anonymous_session)=32),
 peer bytea NOT NULL CHECK(octet_length(peer)=32),
 context_hash bytea NOT NULL CHECK(octet_length(context_hash)=32),
 entry_id bigint NOT NULL CHECK(entry_id>0),
 campaign_id bigint NOT NULL CHECK(campaign_id>0),
 created_at timestamptz NOT NULL,
 expires_at timestamptz NOT NULL CHECK(expires_at>created_at AND expires_at<=created_at+interval '30 seconds')
);
CREATE INDEX pass_site_receipt_expiry ON verification_private.pass_site_receipts(expires_at);
CREATE INDEX pass_site_receipt_session ON verification_private.pass_site_receipts(pass_session);
REVOKE ALL ON verification_private.pass_site_receipts FROM PUBLIC;

-- No posting board/network hold is taken or extended by contest voting.
CREATE FUNCTION verification_private.pass_lock_site_scope(p_pass bytea,p_anonymous bytea) RETURNS void
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE expiry bigint; account bigint;
BEGIN
 PERFORM verification_private.pass_gate();
 SELECT expires_at INTO expiry FROM post_secrets.anonymous_sessions WHERE token_hash=p_anonymous FOR UPDATE;
 IF NOT FOUND OR expiry<=floor(extract(epoch FROM clock_timestamp()))::bigint THEN
  RAISE EXCEPTION 'Anonymous session unavailable' USING ERRCODE='P0166'; END IF;
 PERFORM singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Membership policy unavailable' USING ERRCODE='55000'; END IF;
 SELECT entitlement_id INTO account FROM verification_private.pass_sessions WHERE token_hash=p_pass FOR UPDATE;
 IF FOUND THEN PERFORM id FROM verification_private.pass_entitlements WHERE id=account FOR UPDATE; END IF;
 IF expiry<=floor(extract(epoch FROM clock_timestamp()))::bigint THEN
  RAISE EXCEPTION 'Anonymous session expired while waiting' USING ERRCODE='P0166'; END IF;
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_lock_site_scope(bytea,bytea) FROM PUBLIC;

CREATE FUNCTION content.pass_site_vote_receipt(p_pass bytea,p_receipt bytea,p_peer bytea,p_anonymous bytea,p_context bytea,p_entry bigint)
RETURNS TABLE(status text,purchase_year integer) LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE saved verification_private.pass_sessions%ROWTYPE; member verification_private.pass_entitlements%ROWTYPE;
 stamp timestamptz; campaign bigint; cap integer;
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_verifier' THEN
  RAISE EXCEPTION 'Membership issuer unavailable' USING ERRCODE='42501'; END IF;
 IF p_pass IS NULL OR octet_length(p_pass)<>32 OR p_receipt IS NULL OR octet_length(p_receipt)<>32
  OR p_peer IS NULL OR octet_length(p_peer)<>32 OR p_anonymous IS NULL OR octet_length(p_anonymous)<>32
  OR p_context IS NULL OR octet_length(p_context)<>32 OR p_entry IS NULL OR p_entry<=0 THEN
  RAISE EXCEPTION 'Invalid contest vote context' USING ERRCODE='23514'; END IF;
 campaign:=submission_api.vote_campaign(p_entry);
 IF campaign IS NULL THEN RETURN QUERY SELECT 'none'::text,NULL::integer; RETURN; END IF;
 PERFORM verification_private.pass_lock_site_scope(p_pass,p_anonymous);
 stamp:=clock_timestamp();
 SELECT * INTO saved FROM verification_private.pass_sessions WHERE token_hash=p_pass;
 IF NOT FOUND OR saved.expires_at<=stamp THEN RETURN QUERY SELECT 'none'::text,NULL::integer; RETURN; END IF;
 SELECT * INTO member FROM verification_private.pass_entitlements WHERE id=saved.entitlement_id;
 IF NOT FOUND OR member.credential_version<>saved.credential_version THEN RETURN QUERY SELECT 'none'::text,NULL::integer; RETURN; END IF;
 IF member.expires_at IS NULL OR member.expires_at<=stamp THEN RETURN QUERY SELECT 'expired'::text,NULL::integer; RETURN; END IF;
 IF member.status<>0 THEN RETURN QUERY SELECT 'disabled'::text,NULL::integer; RETURN; END IF;
 DELETE FROM verification_private.pass_site_receipts WHERE token_hash IN(
  SELECT token_hash FROM verification_private.pass_site_receipts WHERE expires_at<=stamp ORDER BY expires_at LIMIT 64 FOR UPDATE SKIP LOCKED);
 SELECT receipt_limit INTO cap FROM verification_private.pass_policy WHERE singleton;
 IF (SELECT count(*) FROM (SELECT 1 FROM verification_private.pass_site_receipts WHERE pass_session=p_pass LIMIT 8) bounded)>=8
  OR (SELECT count(*) FROM (SELECT 1 FROM verification_private.pass_site_receipts LIMIT cap) bounded)>=cap THEN
  RETURN QUERY SELECT 'capacity'::text,NULL::integer; RETURN; END IF;
 IF submission_api.vote_campaign(p_entry) IS DISTINCT FROM campaign THEN RETURN QUERY SELECT 'none'::text,NULL::integer; RETURN; END IF;
 INSERT INTO verification_private.pass_site_receipts VALUES(p_receipt,p_pass,p_anonymous,p_peer,p_context,p_entry,campaign,stamp,stamp+interval '30 seconds');
 RETURN QUERY SELECT 'ready'::text,extract(year FROM member.purchased_at AT TIME ZONE 'America/New_York')::integer;
END $$;
REVOKE ALL ON FUNCTION content.pass_site_vote_receipt(bytea,bytea,bytea,bytea,bytea,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.pass_site_vote_receipt(bytea,bytea,bytea,bytea,bytea,bigint) TO board_verifier;

CREATE FUNCTION submission_api.consume_pass_vote(p_receipt bytea,p_peer bytea,p_anonymous bytea,p_context bytea,p_entry bigint,p_campaign bigint)
RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE receipt verification_private.pass_site_receipts%ROWTYPE; account bigint; stamp timestamptz;
BEGIN
 IF p_receipt IS NULL OR octet_length(p_receipt)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
  OR p_anonymous IS NULL OR octet_length(p_anonymous)<>32 OR p_context IS NULL OR octet_length(p_context)<>32
  OR p_entry IS NULL OR p_entry<=0 OR p_campaign IS NULL OR p_campaign<=0 THEN
  RAISE EXCEPTION 'Invalid contest vote proof' USING ERRCODE='23514'; END IF;
 -- The protected vote holds the campaign and entry locks through this call.
 IF submission_api.vote_campaign(p_entry) IS DISTINCT FROM p_campaign THEN
  RAISE EXCEPTION 'Contest voting is closed' USING ERRCODE='P0002'; END IF;
 SELECT * INTO receipt FROM verification_private.pass_site_receipts WHERE token_hash=p_receipt;
 IF NOT FOUND THEN RAISE EXCEPTION 'Contest membership proof unavailable' USING ERRCODE='P0166'; END IF;
 PERFORM verification_private.pass_lock_site_scope(receipt.pass_session,p_anonymous);
 SELECT * INTO receipt FROM verification_private.pass_site_receipts WHERE token_hash=p_receipt FOR UPDATE;
 stamp:=clock_timestamp();
 IF NOT FOUND OR receipt.peer<>p_peer OR receipt.anonymous_session<>p_anonymous OR receipt.context_hash<>p_context
  OR receipt.entry_id<>p_entry OR receipt.campaign_id<>p_campaign OR receipt.expires_at<=stamp THEN
  RAISE EXCEPTION 'Contest membership proof unavailable' USING ERRCODE='P0166'; END IF;
 SELECT e.id INTO account FROM verification_private.pass_sessions s JOIN verification_private.pass_entitlements e ON e.id=s.entitlement_id
  WHERE s.token_hash=receipt.pass_session AND s.expires_at>stamp AND s.credential_version=e.credential_version
   AND e.expires_at IS NOT NULL AND e.expires_at>stamp AND e.status=0;
 IF NOT FOUND THEN RAISE EXCEPTION 'Contest membership authorization changed' USING ERRCODE='P0166'; END IF;
 IF submission_api.vote_campaign(p_entry) IS DISTINCT FROM p_campaign THEN
  RAISE EXCEPTION 'Contest voting is closed' USING ERRCODE='P0002'; END IF;
 DELETE FROM verification_private.pass_site_receipts WHERE token_hash=p_receipt;
 -- This opaque key stays inside the private vote ledger. Bind the stable
 -- entitlement row, not a device, session, PIN, badge or credential version.
 RETURN sha256(convert_to('26chan:contest-voter:v1','UTF8')||int8send(p_campaign)||int8send(account));
END $$;
REVOKE ALL ON FUNCTION submission_api.consume_pass_vote(bytea,bytea,bytea,bytea,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.consume_pass_vote(bytea,bytea,bytea,bytea,bigint,bigint) TO board_submission_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_api FROM board_submission_owner,board_anonymous_owner;
REVOKE CREATE ON SCHEMA verification_private,content FROM board_anonymous_owner;
SET LOCAL ROLE board_submission_owner;
SELECT set_config('board.submission_catalog_hash',encode(submission_private.catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE submission_private.catalog SET definition_hash=decode(current_setting('board.submission_catalog_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_image_classifier_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog SET definition_hash=decode(current_setting('board.migration_image_classifier_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_image_report_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_image_report_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest Pass proof scopes changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest Pass proof scopes changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest Pass proof scopes changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest Pass proof scopes changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest Pass proof scopes changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Contest Pass proof scopes changed challenge authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission proof authority changed'; END IF; END $$;
RESET ROLE;
