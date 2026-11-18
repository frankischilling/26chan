-- One-use agreement invitations for selected applicants.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before application agreements'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before application agreements'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before application agreements'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before application agreements'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before application agreements'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before application agreements'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;

SELECT singleton FROM submission_private.catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Submission authority drifted before application agreements'; END IF; END $$;
RESET ROLE;

CREATE TABLE submission_private.application_agreements (
 application_id bigint PRIMARY KEY REFERENCES submission_private.applications(id),
 revision integer NOT NULL CHECK(revision BETWEEN 1 AND 1000000),
 invite_hash bytea NOT NULL UNIQUE CHECK(octet_length(invite_hash)=32),
 terms text NOT NULL CHECK(submission_private.valid_text(terms,16000,true,true)),
 issued_at timestamptz NOT NULL,
 expires_at timestamptz NOT NULL CHECK(expires_at>issued_at AND expires_at<=issued_at+interval '168 hours'),
 accepted_at timestamptz CHECK(accepted_at>=issued_at AND accepted_at<expires_at)
);
REVOKE ALL ON submission_private.application_agreements FROM PUBLIC;
GRANT SELECT ON submission_private.application_agreements TO board_application_owner;
GRANT UPDATE(accepted_at) ON submission_private.application_agreements TO board_application_owner;
GRANT UPDATE(state) ON submission_private.applications TO board_application_owner;
GRANT CREATE ON SCHEMA submission_api TO board_application_owner;
SET LOCAL ROLE board_application_owner;

CREATE FUNCTION submission_api.application_agreement(p_hash bytea) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE result text;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Application agreement authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_hash IS NULL OR octet_length(p_hash)<>32 THEN RETURN NULL; END IF;
 SELECT i.terms INTO result FROM submission_private.application_agreements i
 JOIN submission_private.applications a ON a.id=i.application_id
 WHERE i.invite_hash=p_hash AND i.accepted_at IS NULL AND i.expires_at>clock_timestamp()
 AND a.state=2 AND a.revision=i.revision;
 RETURN result;
END $$;

CREATE FUNCTION submission_api.accept_application_agreement(p_hash bytea) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE applicant record; invite record; stamp timestamptz; target bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true OR current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Application agreement authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_hash IS NULL OR octet_length(p_hash)<>32 THEN RETURN false; END IF;
 SELECT application_id INTO target FROM submission_private.application_agreements WHERE invite_hash=p_hash;
 IF NOT FOUND THEN RETURN false; END IF;

 -- Issuance, revocation and acceptance serialize on the application row.
 SELECT state,revision INTO applicant FROM submission_private.applications WHERE id=target FOR UPDATE;
 IF NOT FOUND OR applicant.state<>2 THEN RETURN false; END IF;
 SELECT revision,accepted_at,expires_at INTO invite FROM submission_private.application_agreements
 WHERE application_id=target AND invite_hash=p_hash FOR UPDATE;
 IF NOT FOUND THEN RETURN false; END IF;
 stamp:=clock_timestamp();
 IF invite.accepted_at IS NOT NULL OR invite.expires_at<=stamp OR invite.revision<>applicant.revision THEN
  RETURN false;
 END IF;

 UPDATE submission_private.application_agreements SET accepted_at=stamp WHERE application_id=target;
 UPDATE submission_private.applications SET state=4,
 updated_at=greatest(updated_at,floor(extract(epoch FROM stamp))::bigint) WHERE id=target;
 RETURN true;
END $$;

REVOKE ALL ON FUNCTION submission_api.application_agreement(bytea),submission_api.accept_application_agreement(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.application_agreement(bytea),submission_api.accept_application_agreement(bytea) TO board_public;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_api FROM board_application_owner;


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
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application agreements changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application agreements changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application agreements changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application agreements changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application agreements changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application agreements changed challenge authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application agreement authority changed'; END IF; END $$;
RESET ROLE;
