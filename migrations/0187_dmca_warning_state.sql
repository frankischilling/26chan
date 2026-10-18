-- Private repeat-warning state for the future exact-file effect recorder.
-- No runtime caller receives authority to manufacture warnings or bans.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before file-warning state'; END IF;
END $$;
RESET ROLE;

CREATE TABLE admission.file_effect_capacity (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 warning_limit integer NOT NULL DEFAULT 100000 CHECK(warning_limit BETWEEN 1 AND 1000000)
);
INSERT INTO admission.file_effect_capacity DEFAULT VALUES;
CREATE TABLE admission.dmca_warnings (
 actor_hash bytea PRIMARY KEY CHECK(octet_length(actor_hash)=32),
 last_warning_at timestamptz NOT NULL CHECK(isfinite(last_warning_at))
);
CREATE INDEX dmca_warnings_expiry ON admission.dmca_warnings(last_warning_at);
REVOKE ALL ON admission.file_effect_capacity,admission.dmca_warnings FROM PUBLIC;
GRANT SELECT,UPDATE(singleton) ON admission.file_effect_capacity TO board_admission_owner;
GRANT SELECT,INSERT,DELETE ON admission.dmca_warnings TO board_admission_owner;
GRANT UPDATE(last_warning_at) ON admission.dmca_warnings TO board_admission_owner;
GRANT CREATE ON SCHEMA admission TO board_admission_owner;
SET LOCAL ROLE board_admission_owner;
CREATE FUNCTION admission.record_dmca_warning(p_actor bytea,p_now timestamptz)
RETURNS boolean LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' AS $$
DECLARE v_last timestamptz; v_found boolean; v_limit integer;
BEGIN
 IF p_actor IS NULL OR octet_length(p_actor)<>32 OR p_now IS NULL OR NOT isfinite(p_now) THEN
  RAISE EXCEPTION 'Invalid file-warning context.' USING ERRCODE='23514'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'File warnings require read committed isolation.' USING ERRCODE='55000'; END IF;
 -- The issuer supplies a server-derived exact-peer hash and its decision clock.
 -- Serialize absent rows too, so concurrent first attempts cannot both warn.
 PERFORM pg_advisory_xact_lock(hashtextextended('26chan:dmca-warning:'||encode(p_actor,'hex'),0));
 SELECT w.last_warning_at INTO v_last FROM admission.dmca_warnings w
  WHERE w.actor_hash=p_actor FOR UPDATE;
 v_found:=FOUND;
 IF p_now NOT BETWEEN clock_timestamp()-interval '30 seconds' AND clock_timestamp()+interval '30 seconds' THEN
  RAISE EXCEPTION 'File-warning context expired.' USING ERRCODE='23514'; END IF;
 IF v_found THEN
  IF v_last>=p_now-interval '1 day' THEN
   -- Repeated refusals do not extend the original warning's one-day window.
   RETURN true;
  END IF;
  UPDATE admission.dmca_warnings SET last_warning_at=p_now WHERE actor_hash=p_actor;
  RETURN false;
 END IF;
 SELECT warning_limit INTO v_limit FROM admission.file_effect_capacity WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN
  RAISE EXCEPTION 'File-warning capacity is unavailable.' USING ERRCODE='55000'; END IF;
 IF p_now NOT BETWEEN clock_timestamp()-interval '30 seconds' AND clock_timestamp()+interval '30 seconds' THEN
  RAISE EXCEPTION 'File-warning context expired.' USING ERRCODE='23514'; END IF;
 DELETE FROM admission.dmca_warnings w WHERE w.actor_hash IN (
  SELECT old.actor_hash FROM admission.dmca_warnings old
  WHERE old.last_warning_at<p_now-interval '7 days'
  ORDER BY old.last_warning_at,old.actor_hash LIMIT 256 FOR UPDATE SKIP LOCKED
 );
 IF (SELECT count(*) FROM (SELECT 1 FROM admission.dmca_warnings LIMIT v_limit) bounded)>=v_limit THEN
  RAISE EXCEPTION 'File-warning capacity is exhausted.' USING ERRCODE='53300'; END IF;
 INSERT INTO admission.dmca_warnings(actor_hash,last_warning_at) VALUES(p_actor,p_now);
 RETURN false;
END $$;
REVOKE ALL ON FUNCTION admission.record_dmca_warning(bytea,timestamptz) FROM PUBLIC;
RESET ROLE;
REVOKE CREATE ON SCHEMA admission FROM board_admission_owner;

SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'File-warning state changed transfer authority'; END IF; END $$;
RESET ROLE;
