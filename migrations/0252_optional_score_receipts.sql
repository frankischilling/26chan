-- Distinct, default-closed proof scope for optional event likes.
-- Like receipts never consume posting credits or authorize another action.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before optional event authority'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before optional event authority'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;

SELECT singleton FROM event_private.catalog WHERE singleton FOR UPDATE;
SET LOCAL ROLE board_event_owner;
DO $$ BEGIN
 IF content.events_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Event authority drifted before score scopes'; END IF;
END $$;
RESET ROLE;
ALTER TABLE event_private.runs DROP CONSTRAINT runs_kind_check;
ALTER TABLE event_private.runs ADD CONSTRAINT runs_kind_check CHECK(kind IN(1,2,3));
GRANT CREATE ON SCHEMA content TO board_event_owner;
SET LOCAL ROLE board_event_owner;
CREATE FUNCTION content.event_score_open() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT content.events_ready() AND EXISTS(SELECT 1 FROM event_private.runs WHERE kind=3 AND enabled
  AND starts_at<=extract(epoch FROM statement_timestamp()) AND ends_at>extract(epoch FROM statement_timestamp()))
$$;
REVOKE ALL ON FUNCTION content.event_score_open() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.event_score_open() TO board_public,board_verifier,board_anonymous_owner;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_event_owner;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
-- All six affected checks use a finite ARRAY. Preserve every unrelated
-- condition, including target binding and existing credit restrictions.
DO $$
DECLARE c record; definition text; changed integer:=0;
BEGIN
 FOR c IN SELECT conrelid,conname,pg_get_constraintdef(oid) AS definition,
  (SELECT pg_get_userbyid(relowner) FROM pg_class WHERE oid=conrelid) AS owner_name FROM pg_constraint
  WHERE conrelid IN('verification_private.challenges'::regclass,'verification_private.receipts'::regclass,'verification_private.pass_receipts'::regclass)
   AND contype='c' AND pg_get_constraintdef(oid) LIKE '%''report''::text%'
 LOOP
  IF c.definition NOT LIKE '%ARRAY[%''report''::text%' THEN RAISE EXCEPTION 'Unexpected score proof constraint'; END IF;
  IF c.owner_name NOT IN('board_migrator','board_anonymous_owner') THEN RAISE EXCEPTION 'Unexpected proof table owner'; END IF;
  EXECUTE format('SET LOCAL ROLE %I',c.owner_name);
  definition:=replace(c.definition,'''report''::text','''report''::text, ''like''::text');
  EXECUTE format('ALTER TABLE %s DROP CONSTRAINT %I',c.conrelid::regclass,c.conname);
  EXECUTE format('ALTER TABLE %s ADD CONSTRAINT %I %s',c.conrelid::regclass,c.conname,definition);
  RESET ROLE;
  changed:=changed+1;
 END LOOP;
 IF changed<>6 THEN RAISE EXCEPTION 'Unexpected score proof constraint count: %',changed; END IF;
END $$;
ALTER TABLE verification_private.receipts ADD CONSTRAINT event_like_no_credits
 CHECK(purpose<>'like' OR (NOT from_credit AND NOT may_grant_credit));
SET LOCAL ROLE board_anonymous_owner;
-- Predecessor fingerprints above cover these exact definitions. Replace only
-- the finite purpose sets and insert a closed-run check before any writes.
DO $$
DECLARE target regprocedure; definition text; updated text;
BEGIN
 FOREACH target IN ARRAY ARRAY[
  'content.verification_issue_challenge(bytea,bytea,bytea,bytea,text,text,bigint,bytea,boolean)'::regprocedure,
  'content.pass_write_receipt(bytea,bytea,bytea,bytea,bytea,bytea,bytea,text,text,bigint)'::regprocedure]
 LOOP
  definition:=pg_get_functiondef(target);
  updated:=replace(definition,'''reply'',''report''','''reply'',''report'',''like''');
  IF updated=definition THEN RAISE EXCEPTION 'Missing score receipt purpose set'; END IF;
  updated:=replace(updated,'p_purpose=''report'' AND','p_purpose IN(''report'',''like'') AND');
  updated:=regexp_replace(updated,E'BEGIN\n',E'BEGIN\n IF p_purpose=''like'' AND content.event_score_open() IS DISTINCT FROM true THEN RAISE EXCEPTION ''Score event is closed'' USING ERRCODE=''P0002''; END IF;\n');
  IF updated NOT LIKE '%Score event is closed%' THEN RAISE EXCEPTION 'Missing score receipt gate'; END IF;
  EXECUTE updated;
 END LOOP;
 definition:=pg_get_functiondef('verification_private.take_write_receipt_data(bytea,bytea,bytea,text,text,bigint,bytea)'::regprocedure);
 updated:=replace(definition,'(p_purpose IN(''reply'',''report'') AND p_target>0)','(p_purpose IN(''reply'',''report'',''like'') AND p_target>0)');
 IF updated=definition THEN RAISE EXCEPTION 'Missing protected score scope'; END IF;
 -- Its separate from_credit check intentionally still accepts reply/report only.
 updated:=regexp_replace(updated,E'BEGIN\n',E'BEGIN\n IF p_purpose=''like'' AND content.event_score_open() IS DISTINCT FROM true THEN RAISE EXCEPTION ''Score event is closed'' USING ERRCODE=''P0002''; END IF;\n');
 EXECUTE updated;
 definition:=pg_get_functiondef('content.verification_check_challenge(bytea,bytea,bytea,bytea,text,text,bigint,bytea,bytea)'::regprocedure);
 updated:=regexp_replace(definition,E'BEGIN\n',E'BEGIN\n IF p_purpose=''like'' AND content.event_score_open() IS DISTINCT FROM true THEN RETURN false; END IF;\n');
 IF updated=definition THEN RAISE EXCEPTION 'Missing score answer gate'; END IF;
 EXECUTE updated;
 definition:=pg_get_functiondef('verification_private.pass_checked_receipt(bytea,bytea,bytea,bytea,text,text,bigint)'::regprocedure);
 updated:=regexp_replace(definition,E'BEGIN\n',E'BEGIN\n IF p_purpose=''like'' AND content.event_score_open() IS DISTINCT FROM true THEN RAISE EXCEPTION ''Membership receipt unavailable'' USING ERRCODE=''P0166''; END IF;\n');
 IF updated=definition THEN RAISE EXCEPTION 'Missing score membership gate'; END IF;
 EXECUTE updated;

END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
SET LOCAL ROLE board_event_owner;
SELECT set_config('board.migration_score_event_hash',encode(event_private.catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE event_private.catalog SET definition_hash=decode(current_setting('board.migration_score_event_hash'),'hex') WHERE singleton;
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
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Optional score receipt scopes changed challenge authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_event_owner;
DO $$ BEGIN IF content.events_ready() IS DISTINCT FROM true OR content.event_score_open() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Score proof migration did not preserve closed events'; END IF; END $$;
RESET ROLE;
