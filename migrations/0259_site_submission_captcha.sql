-- Purpose-separated site CAPTCHA receipts; no posting credits or Pass bypass.
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
 RAISE EXCEPTION 'Submission authority drifted before proof scopes'; END IF; END $$;
RESET ROLE;
GRANT SELECT(id,campaign_id,state) ON submission_private.applications,submission_private.contest_entries TO board_submission_owner;
GRANT CREATE ON SCHEMA submission_api TO board_submission_owner,board_anonymous_owner;
GRANT USAGE ON SCHEMA submission_api TO board_anonymous_owner;
SET LOCAL ROLE board_submission_owner;
CREATE FUNCTION submission_api.challenge_open(p_purpose text,p_target bigint) RETURNS boolean
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE stamp bigint:=floor(extract(epoch FROM statement_timestamp()))::bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_purpose IS NULL OR p_target IS NULL OR p_target<=0 THEN RETURN false; END IF;
 CASE p_purpose
 WHEN 'site_feedback' THEN
  RETURN EXISTS(SELECT 1 FROM submission_private.feedback_profiles p WHERE p.id=p_target AND p.enabled AND p.received<p.capacity)
   AND EXISTS(SELECT 1 FROM submission_private.feedback_categories c WHERE c.profile_id=p_target);
 WHEN 'site_application' THEN
  RETURN EXISTS(SELECT 1 FROM submission_private.application_campaigns c WHERE c.id=p_target AND c.enabled
   AND c.starts_at<=stamp AND c.ends_at>stamp AND c.received<c.capacity);
 WHEN 'site_application_auth','site_application_edit' THEN
  RETURN EXISTS(SELECT 1 FROM submission_private.applications a JOIN submission_private.application_campaigns c ON c.id=a.campaign_id
   WHERE a.id=p_target AND c.enabled AND c.starts_at<=stamp AND c.ends_at>stamp);
 WHEN 'site_contest_submit' THEN
  RETURN EXISTS(SELECT 1 FROM submission_private.contest_campaigns c WHERE c.id=p_target AND c.enabled
   AND c.submission_starts<=stamp AND c.submission_ends>stamp AND c.received<c.capacity);
 WHEN 'site_contest_delete' THEN
  RETURN EXISTS(SELECT 1 FROM submission_private.contest_entries e JOIN submission_private.contest_campaigns c ON c.id=e.campaign_id
   WHERE e.id=p_target AND c.enabled AND c.submission_starts<=stamp AND c.submission_ends>stamp AND e.state<>4);
 ELSE RETURN false;
 END CASE;
END $$;
REVOKE ALL ON FUNCTION submission_api.challenge_open(text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.challenge_open(text,bigint) TO board_verifier,board_anonymous_owner;
RESET ROLE;

DO $$
DECLARE c record; definition text; changed integer:=0;
 names text:='''site_feedback''::text,''site_application''::text,''site_application_auth''::text,''site_application_edit''::text,''site_contest_submit''::text,''site_contest_delete''::text';
BEGIN
 FOR c IN SELECT conrelid,conname,pg_get_constraintdef(oid) AS definition,
  (SELECT pg_get_userbyid(relowner) FROM pg_class WHERE oid=conrelid) AS owner_name FROM pg_constraint
  WHERE conrelid IN('verification_private.challenges'::regclass,'verification_private.receipts'::regclass)
   AND contype='c' AND pg_get_constraintdef(oid) LIKE '%''report''::text%'
 LOOP
  IF c.owner_name NOT IN('board_migrator','board_anonymous_owner') THEN RAISE EXCEPTION 'Unexpected site proof table owner'; END IF;
  EXECUTE format('SET LOCAL ROLE %I',c.owner_name);
  IF c.definition LIKE '%board%' THEN
   IF c.definition NOT LIKE 'CHECK (%' THEN RAISE EXCEPTION 'Unexpected site context constraint'; END IF;
   definition:='CHECK ('||substr(c.definition,7)||' OR (purpose=ANY(ARRAY['||names||']) AND board IS NULL AND target>0'||
    CASE WHEN c.conrelid='verification_private.receipts'::regclass THEN ' AND NOT from_credit AND NOT may_grant_credit' ELSE '' END||'))';
  ELSE
   definition:=replace(c.definition,'''report''::text','''report''::text,'||names);
   IF definition=c.definition THEN RAISE EXCEPTION 'Missing site purpose constraint'; END IF;
  END IF;
  EXECUTE format('ALTER TABLE %s DROP CONSTRAINT %I',c.conrelid::regclass,c.conname);
  EXECUTE format('ALTER TABLE %s ADD CONSTRAINT %I %s',c.conrelid::regclass,c.conname,definition);
  RESET ROLE;
  changed:=changed+1;
 END LOOP;
 IF changed<>4 THEN RAISE EXCEPTION 'Unexpected site proof constraint count: %',changed; END IF;
END $$;
ALTER TABLE verification_private.receipts ADD CONSTRAINT site_submission_no_credits
 CHECK(purpose NOT IN('site_feedback','site_application','site_application_auth','site_application_edit','site_contest_submit','site_contest_delete')
 OR (NOT from_credit AND NOT may_grant_credit AND credit_identity IS NULL));

GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
DO $$
DECLARE definition text; updated text;
 names text:='''site_feedback'',''site_application'',''site_application_auth'',''site_application_edit'',''site_contest_submit'',''site_contest_delete''';
 anchor text;
BEGIN
 definition:=pg_get_functiondef('content.verification_issue_challenge(bytea,bytea,bytea,bytea,text,text,bigint,bytea,boolean)'::regprocedure);
 anchor:='p_purpose NOT IN (''thread'',''reply'',''report'',''like'',''email_request'',''email_verify'',''ticket'')';
 IF strpos(definition,anchor)=0 THEN RAISE EXCEPTION 'Missing issuer purpose boundary'; END IF;
 updated:=replace(definition,anchor,'p_purpose NOT IN (''thread'',''reply'',''report'',''like'',''email_request'',''email_verify'',''ticket'','||names||')');
 anchor:='OR (p_purpose=''ticket'' AND p_board IS NOT NULL)';
 IF strpos(updated,anchor)=0 THEN RAISE EXCEPTION 'Missing issuer target boundary'; END IF;
 updated:=replace(updated,anchor,anchor||' OR (p_purpose IN('||names||') AND p_board IS NULL AND p_target>0)');
 updated:=regexp_replace(updated,E'BEGIN\n',E'BEGIN\n IF p_purpose IN('||names||E') AND submission_api.challenge_open(p_purpose,p_target) IS DISTINCT FROM true THEN RAISE EXCEPTION ''Submission workflow is closed'' USING ERRCODE=''P0002''; END IF;\n');
 EXECUTE updated;
 definition:=pg_get_functiondef('content.verification_check_challenge(bytea,bytea,bytea,bytea,text,text,bigint,bytea,bytea)'::regprocedure);
 updated:=regexp_replace(definition,E'BEGIN\n',E'BEGIN\n IF p_purpose IN('||names||E') AND submission_api.challenge_open(p_purpose,p_target) IS DISTINCT FROM true THEN RETURN false; END IF;\n');
 IF updated=definition THEN RAISE EXCEPTION 'Missing site answer boundary'; END IF;
 EXECUTE updated;
END $$;

CREATE FUNCTION submission_api.consume_captcha(p_receipt bytea,p_peer bytea,p_session bytea,p_purpose text,p_target bigint,p_context bytea)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE receipt verification_private.receipts%ROWTYPE; expiry bigint; stamp timestamptz;
BEGIN
 IF p_receipt IS NULL OR octet_length(p_receipt)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
  OR p_session IS NULL OR octet_length(p_session)<>32 OR p_context IS NULL OR octet_length(p_context)<>32
  OR p_purpose IS NULL OR p_target IS NULL OR p_target<=0
  OR p_purpose NOT IN('site_feedback','site_application','site_application_auth','site_application_edit','site_contest_submit','site_contest_delete') THEN
  RAISE EXCEPTION 'Invalid submission verification context' USING ERRCODE='23514'; END IF;
 IF submission_api.challenge_open(p_purpose,p_target) IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Submission workflow is closed' USING ERRCODE='P0002'; END IF;
 -- The protected mutation locks its profile/campaign before entering here.
 -- Session, verification-policy and receipt order matches the shared issuer.
 SELECT s.expires_at INTO expiry FROM post_secrets.anonymous_sessions s WHERE s.token_hash=p_session FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Submission verification is required' USING ERRCODE='P0166'; END IF;
 PERFORM singleton FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable' USING ERRCODE='55000'; END IF;
 SELECT r.* INTO receipt FROM verification_private.receipts r WHERE r.token_hash=p_receipt FOR UPDATE;
 IF NOT FOUND OR receipt.peer_hash<>p_peer OR receipt.session_hash<>p_session OR receipt.context_hash<>p_context
  OR receipt.purpose<>p_purpose OR receipt.target<>p_target OR receipt.board IS NOT NULL
  OR receipt.from_credit OR receipt.may_grant_credit OR receipt.credit_identity IS NOT NULL THEN
  RAISE EXCEPTION 'Submission verification is required' USING ERRCODE='P0166'; END IF;
 stamp:=clock_timestamp();
 IF receipt.expires_at<=stamp OR expiry<=extract(epoch FROM stamp)::bigint THEN
  RAISE EXCEPTION 'Submission verification is required' USING ERRCODE='P0166'; END IF;
 DELETE FROM verification_private.receipts WHERE token_hash=p_receipt;
END $$;
REVOKE ALL ON FUNCTION submission_api.consume_captcha(bytea,bytea,bytea,text,bigint,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.consume_captcha(bytea,bytea,bytea,text,bigint,bytea) TO board_submission_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_api FROM board_submission_owner,board_anonymous_owner;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
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
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Site submission proof scopes changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Site submission proof scopes changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Site submission proof scopes changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Site submission proof scopes changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Site submission proof scopes changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Site submission proof scopes changed challenge authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission proof authority changed'; END IF; END $$;
RESET ROLE;
