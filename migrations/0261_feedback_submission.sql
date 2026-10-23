-- Source feedback fields with native capacity, cooldown and one-use proof gates.
SELECT singleton FROM submission_private.catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Submission authority drifted before feedback writes'; END IF; END $$;
RESET ROLE;
ALTER TABLE submission_private.feedback_profiles ADD COLUMN cooldown_seconds integer NOT NULL DEFAULT 60
 CHECK(cooldown_seconds BETWEEN 0 AND 86400);
ALTER TABLE submission_private.feedback ADD CONSTRAINT feedback_host_address
 CHECK(masklen(peer)=CASE family(peer) WHEN 4 THEN 32 WHEN 6 THEN 128 END);
GRANT UPDATE(received) ON submission_private.feedback_profiles TO board_submission_owner;
GRANT SELECT(id,profile_id,actor_hash,created_at) ON submission_private.feedback TO board_submission_owner;
GRANT INSERT(profile_id,category,subject,message,actor_hash,peer,user_agent,language,created_at,updated_at)
 ON submission_private.feedback TO board_submission_owner;
GRANT USAGE ON SEQUENCE submission_private.feedback_id_seq TO board_submission_owner;
GRANT EXECUTE ON FUNCTION submission_private.valid_text(text,integer,boolean,boolean) TO board_submission_owner;
GRANT CREATE ON SCHEMA submission_api TO board_submission_owner;
SET LOCAL ROLE board_submission_owner;
CREATE FUNCTION submission_api.submit_feedback(p_profile bigint,p_category integer,p_subject text,p_message text,
 p_address inet,p_agent text,p_language text,p_session bytea,p_receipt bytea,p_peer bytea,p_context bytea)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE profile submission_private.feedback_profiles%ROWTYPE; stamp bigint; previous bigint; result bigint;
BEGIN
 IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Submission authority is unavailable' USING ERRCODE='55000'; END IF;
 IF p_profile IS NULL OR p_profile<=0 OR p_category IS NULL OR p_category NOT BETWEEN 0 AND 65535
  OR p_subject IS NULL OR p_message IS NULL OR p_address IS NULL OR p_agent IS NULL OR p_language IS NULL
  OR p_session IS NULL OR octet_length(p_session)<>32 OR p_receipt IS NULL OR octet_length(p_receipt)<>32
  OR p_peer IS NULL OR octet_length(p_peer)<>32 OR p_context IS NULL OR octet_length(p_context)<>32
  OR masklen(p_address)<>(CASE family(p_address) WHEN 4 THEN 32 WHEN 6 THEN 128 END) THEN
  RAISE EXCEPTION 'Invalid feedback submission.' USING ERRCODE='P0225'; END IF;
 SELECT * INTO profile FROM submission_private.feedback_profiles WHERE id=p_profile FOR UPDATE;
 IF NOT FOUND OR NOT profile.enabled THEN RAISE EXCEPTION 'Feedback is closed.' USING ERRCODE='P0225'; END IF;
 IF NOT submission_private.valid_text(p_subject,profile.subject_limit,true,false)
  OR NOT submission_private.valid_text(p_message,profile.message_limit,true,true)
  OR NOT submission_private.valid_text(p_agent,512,false,false)
  OR NOT submission_private.valid_text(p_language,128,false,false)
  OR NOT EXISTS(SELECT 1 FROM submission_private.feedback_categories WHERE profile_id=p_profile AND id=p_category) THEN
  RAISE EXCEPTION 'Invalid feedback submission.' USING ERRCODE='P0225'; END IF;
 IF profile.received>=profile.capacity THEN RAISE EXCEPTION 'Feedback capacity has been reached.' USING ERRCODE='P0225'; END IF;
 stamp:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 SELECT created_at INTO previous FROM submission_private.feedback WHERE profile_id=p_profile AND actor_hash=p_peer
  ORDER BY created_at DESC LIMIT 1;
 IF previous IS NOT NULL AND stamp<previous+profile.cooldown_seconds THEN
  RAISE EXCEPTION 'Please wait before submitting more feedback.' USING ERRCODE='P0225'; END IF;
 PERFORM submission_api.consume_captcha(p_receipt,p_peer,p_session,'site_feedback',p_profile,p_context);
 stamp:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 INSERT INTO submission_private.feedback(profile_id,category,subject,message,actor_hash,peer,user_agent,language,created_at,updated_at)
  VALUES(p_profile,p_category,p_subject,p_message,p_peer,p_address,p_agent,p_language,stamp,stamp) RETURNING id INTO result;
 UPDATE submission_private.feedback_profiles SET received=received+1 WHERE id=p_profile;
 RETURN result;
END $$;
REVOKE ALL ON FUNCTION submission_api.submit_feedback(bigint,integer,text,text,inet,text,text,bytea,bytea,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submission_api.submit_feedback(bigint,integer,text,text,inet,text,text,bytea,bytea,bytea,bytea) TO board_public;
RESET ROLE;
REVOKE CREATE ON SCHEMA submission_api FROM board_submission_owner;
SET LOCAL ROLE board_submission_owner;
SELECT set_config('board.submission_catalog_hash',encode(submission_private.catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE submission_private.catalog SET definition_hash=decode(current_setting('board.submission_catalog_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Feedback write authority changed'; END IF; END $$;
RESET ROLE;
