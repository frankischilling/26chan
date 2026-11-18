-- Blind application review and explicit administrator selection.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before application review'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before application review'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before application review'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before application review'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before application review'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before application review'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;

SELECT singleton FROM submission_private.catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Submission authority drifted before application review'; END IF; END $$;
RESET ROLE;

CREATE TABLE submission_private.application_review_log (
 application_id bigint PRIMARY KEY REFERENCES submission_private.applications(id),
 account_id bigint NOT NULL CHECK(account_id>0),
 revision integer NOT NULL CHECK(revision BETWEEN 1 AND 1000000),
 action text NOT NULL CHECK(action IN('accept','decline')),
 after_state smallint NOT NULL CHECK(after_state IN(1,2)),
 created_at bigint NOT NULL CHECK(created_at BETWEEN 0 AND 253402300799),
 CHECK((action='accept' AND after_state=2) OR (action='decline' AND after_state=1))
);
REVOKE ALL ON submission_private.application_review_log FROM PUBLIC,board_public,board_staff,board_auth,board_verifier,board_media,board_media_read,board_media_intake,board_monitor;
GRANT SELECT ON submission_private.application_campaigns,submission_private.applications TO board_submission_review_owner;
GRANT UPDATE(state,updated_at) ON submission_private.applications TO board_submission_review_owner;
GRANT INSERT ON submission_private.application_review_log TO board_submission_review_owner;
GRANT EXECUTE ON FUNCTION submission_private.valid_text_array(text[],integer,integer,boolean) TO board_submission_review_owner;

GRANT CREATE ON SCHEMA submission_api TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;
CREATE FUNCTION submission_api.application_review_authority(p_session bytea,p_csrf bytea,p_idle integer,p_select boolean)
RETURNS TABLE(account_id bigint,is_admin boolean)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE identity record;
BEGIN
 IF p_session IS NULL OR octet_length(p_session)<>32 OR p_csrf IS NULL OR octet_length(p_csrf)<>32
  OR p_idle IS NULL OR p_idle NOT BETWEEN 60 AND 3600 OR p_select IS NULL THEN
  RAISE EXCEPTION 'Application review authentication is required' USING ERRCODE='P225A'; END IF;
 SELECT * INTO identity FROM staff_identity.lock_session(p_session,p_idle);
 IF NOT FOUND OR identity.csrf_hash<>p_csrf
  OR NOT coalesce(identity.role='admin' OR (NOT p_select AND 'application_review'=ANY(identity.flags)),false) THEN
  RAISE EXCEPTION 'Application review authentication is required' USING ERRCODE='P225A'; END IF;
 IF p_select AND identity.recent IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Recent application review authentication is required' USING ERRCODE='P225Q'; END IF;
 RETURN QUERY SELECT identity.account_id,identity.role='admin';
END $$;
REVOKE ALL ON FUNCTION submission_api.application_review_authority(bytea,bytea,integer,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.application_review_authority(bytea,bytea,integer,boolean) TO board_submission_review_owner,board_application_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_api FROM board_staff_post_owner;

GRANT CREATE ON SCHEMA submission_api TO board_submission_review_owner;
SET LOCAL ROLE board_submission_review_owner;
CREATE FUNCTION submission_api.application_review_page(p_session bytea,p_csrf bytea,p_idle integer,p_campaign bigint,p_state integer,p_before bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE authority record; campaign bigint; rows jsonb; next_id bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true OR current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Application review authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_campaign IS NULL OR p_campaign<0 OR p_state IS NULL OR p_state NOT BETWEEN 0 AND 4 OR p_before IS NULL OR p_before<0 THEN
  RAISE EXCEPTION 'Invalid application review page' USING ERRCODE='23514'; END IF;
 SELECT * INTO authority FROM submission_api.application_review_authority(p_session,p_csrf,p_idle,false);
 IF (SELECT count(*) FROM (SELECT 1 FROM submission_private.application_campaigns LIMIT 10001) bounded)>10000 THEN
  RAISE EXCEPTION 'Application campaign inventory exceeds its bound' USING ERRCODE='P225C'; END IF;
 IF p_campaign=0 THEN SELECT coalesce(max(id) FILTER(WHERE enabled),max(id)) INTO campaign FROM submission_private.application_campaigns;
 ELSE SELECT id INTO campaign FROM submission_private.application_campaigns WHERE id=p_campaign; END IF;
 IF campaign IS NULL THEN RETURN jsonb_build_object('campaign',NULL,'rows','[]'::jsonb,'next',0,'is_admin',authority.is_admin); END IF;
 IF (SELECT count(*) FROM (SELECT 1 FROM submission_private.applications WHERE campaign_id=campaign LIMIT 100001) bounded)>100000 THEN
  RAISE EXCEPTION 'Application inventory exceeds its bound' USING ERRCODE='P225C'; END IF;
 WITH chosen AS (
  SELECT a.id,a.state,a.revision,a.age,a.timezone,a.hours,a.board1,a.board2,a.created_at
  FROM submission_private.applications a WHERE a.campaign_id=campaign AND a.state=p_state AND (p_before=0 OR a.id<p_before)
  ORDER BY a.id DESC LIMIT 26
 ), numbered AS (SELECT *,row_number() OVER(ORDER BY id DESC) position FROM chosen)
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'state',state,'revision',revision,'age',age,'timezone',timezone,'hours',hours,
  'board1',board1,'board2',board2,'created_at',created_at) ORDER BY id DESC) FILTER(WHERE position<=25),'[]'::jsonb),
  CASE WHEN count(*)=26 THEN min(id) FILTER(WHERE position<=25) ELSE 0 END INTO rows,next_id FROM numbered;
 PERFORM submission_api.application_review_authority(p_session,p_csrf,p_idle,false);
 RETURN jsonb_build_object('campaign',campaign,'rows',rows,'next',next_id,'is_admin',authority.is_admin);
END $$;

CREATE FUNCTION submission_api.application_review_detail(p_session bytea,p_csrf bytea,p_idle integer,p_id bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE result jsonb;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true OR current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Application review authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_id IS NULL OR p_id<=0 THEN RAISE EXCEPTION 'Invalid application ID' USING ERRCODE='23514'; END IF;
 PERFORM submission_api.application_review_authority(p_session,p_csrf,p_idle,false);
 SELECT jsonb_build_object('id',a.id,'campaign',a.campaign_id,'state',a.state,'revision',a.revision,
  'age',a.age,'timezone',a.timezone,'hours',a.hours,'times',a.times,'board1',a.board1,'board2',a.board2,
  'answers',a.answers,'questions',c.questions,'created_at',a.created_at,'updated_at',a.updated_at)
 INTO result FROM submission_private.applications a JOIN submission_private.application_campaigns c ON c.id=a.campaign_id WHERE a.id=p_id;
 PERFORM submission_api.application_review_authority(p_session,p_csrf,p_idle,false);
 RETURN result;
END $$;

-- Selection is an explicit native review step. It neither creates an account
-- nor signs an agreement. State 2 matches the source agreement eligibility gate.
CREATE FUNCTION submission_api.application_review_action(p_session bytea,p_csrf bytea,p_idle integer,p_id bigint,p_revision integer,p_action text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE authority record; row_data record; next_state smallint; stamp bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true OR current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Application review authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_id IS NULL OR p_id<=0 OR p_revision IS NULL OR p_revision NOT BETWEEN 1 AND 1000000
  OR p_action IS NULL OR p_action NOT IN('accept','decline') THEN
  RAISE EXCEPTION 'Invalid application review action' USING ERRCODE='23514'; END IF;
 SELECT * INTO authority FROM submission_api.application_review_authority(p_session,p_csrf,p_idle,true);
 SELECT state,revision,updated_at INTO row_data FROM submission_private.applications WHERE id=p_id FOR UPDATE;
 IF NOT FOUND THEN RETURN false; END IF;
 PERFORM submission_api.application_review_authority(p_session,p_csrf,p_idle,true);
 IF row_data.state<>0 OR row_data.revision<>p_revision THEN RETURN false; END IF;
 next_state:=CASE WHEN p_action='accept' THEN 2 ELSE 1 END;
 stamp:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 UPDATE submission_private.applications SET state=next_state,updated_at=greatest(stamp,updated_at) WHERE id=p_id;
 INSERT INTO submission_private.application_review_log(application_id,account_id,revision,action,after_state,created_at)
 VALUES(p_id,authority.account_id,p_revision,p_action,next_state,stamp);
 RETURN true;
END $$;
REVOKE ALL ON FUNCTION submission_api.application_review_page(bytea,bytea,integer,bigint,integer,bigint),
 submission_api.application_review_detail(bytea,bytea,integer,bigint),
 submission_api.application_review_action(bytea,bytea,integer,bigint,integer,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.application_review_page(bytea,bytea,integer,bigint,integer,bigint),
 submission_api.application_review_detail(bytea,bytea,integer,bigint),
 submission_api.application_review_action(bytea,bytea,integer,bigint,integer,text) TO board_auth;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_api FROM board_submission_review_owner;

-- The blind reviewer never gains identity-table access. The existing applicant
-- owner releases contact fields only to a recently authenticated administrator,
-- after selection. Ownership hashes and actor hashes are never returned.
GRANT CREATE ON SCHEMA submission_api TO board_application_owner;
SET LOCAL ROLE board_application_owner;
CREATE FUNCTION submission_api.application_selected_identity(p_session bytea,p_csrf bytea,p_idle integer,p_id bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE result jsonb;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true OR current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Application review authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_id IS NULL OR p_id<=0 THEN RAISE EXCEPTION 'Invalid application ID' USING ERRCODE='23514'; END IF;
 PERFORM submission_api.application_review_authority(p_session,p_csrf,p_idle,true);
 SELECT jsonb_build_object('id',a.id,'firstname',i.firstname,'handle',i.handle,'email',i.email)
 INTO result FROM submission_private.applications a JOIN submission_private.application_identity i ON i.application_id=a.id
 WHERE a.id=p_id AND a.state IN(2,4);
 PERFORM submission_api.application_review_authority(p_session,p_csrf,p_idle,true);
 RETURN result;
END $$;
REVOKE ALL ON FUNCTION submission_api.application_selected_identity(bytea,bytea,integer,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.application_selected_identity(bytea,bytea,integer,bigint) TO board_auth;
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
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application review changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application review changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application review changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application review changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application review changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application review changed challenge authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Application review authority changed'; END IF; END $$;
RESET ROLE;
