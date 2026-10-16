\set ON_ERROR_STOP on
BEGIN;
SET LOCAL ROLE board_migrator;
DO $history$
DECLARE a bytea:=sha256(convert_to('owned-report-history-a','UTF8'));
 b bytea:=sha256(convert_to('owned-report-history-b','UTF8'));
 c bytea:=sha256(convert_to('owned-report-history-c','UTF8'));
 stamp timestamptz:=clock_timestamp(); values_at timestamptz[]; cleared timestamptz; result boolean;
 expected integer; remaining integer; attempt integer;
BEGIN
 EXECUTE 'SET LOCAL ROLE board_report_admission_owner';
 PERFORM content.record_report_challenge_history(ARRAY[a,a,a,a],ARRAY[stamp-interval '3 minutes',stamp-interval '2 minutes',stamp-interval '1 minute',stamp],false);
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
 SELECT last_reports INTO values_at FROM verification_private.report_history WHERE actor_hash=a;
 IF values_at IS DISTINCT FROM ARRAY[stamp,stamp-interval '1 minute',stamp-interval '2 minutes'] THEN
  RAISE EXCEPTION 'Report history did not retain the latest three actions'; END IF;
 EXECUTE 'SET LOCAL ROLE board_report_admission_owner';
 PERFORM content.record_report_challenge_history(ARRAY[a],ARRAY[stamp-interval '1 minute'],true);
 PERFORM content.record_report_challenge_history(ARRAY[a],ARRAY[stamp-interval '2 minutes'],true);
 PERFORM content.record_report_challenge_history(ARRAY[b],ARRAY[stamp-interval '72 hours'],true);
 PERFORM content.record_report_challenge_history(ARRAY[c],ARRAY[stamp-interval '1 hour'],false);
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
 SELECT last_cleared_at INTO cleared FROM verification_private.report_history WHERE actor_hash=a;
 IF cleared IS DISTINCT FROM stamp-interval '1 minute' THEN RAISE EXCEPTION 'Older clear replaced latest clear'; END IF;
 IF EXISTS(SELECT 1 FROM verification_private.report_history WHERE actor_hash IN(b,c)) THEN
  RAISE EXCEPTION 'Expired actions created active history'; END IF;
 UPDATE verification_private.policy SET report_history_limit=(SELECT count(*) FROM verification_private.report_history),report_history_complete_after=stamp-interval '1 second' WHERE singleton;
 EXECUTE 'SET LOCAL ROLE board_report_admission_owner';
 SELECT content.record_report_challenge_history(ARRAY[a],ARRAY[stamp],true) INTO result;
 IF NOT result THEN RAISE EXCEPTION 'Full capacity prevented existing actor update'; END IF;
 SELECT content.record_report_challenge_history(ARRAY[b],ARRAY[stamp],false) INTO result;
 IF result THEN RAISE EXCEPTION 'Full history capacity was ignored'; END IF;
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
 IF (SELECT report_history_complete_after FROM verification_private.policy WHERE singleton)<stamp+interval '72 hours' THEN
  RAISE EXCEPTION 'Omitted history did not suppress the optional exemption'; END IF;
 IF EXISTS(SELECT 1 FROM verification_private.report_history WHERE actor_hash=b) THEN
  RAISE EXCEPTION 'Unavailable history silently inserted an over-cap actor'; END IF;
 -- Cleanup commits progress even after lowering the cap below occupancy.
 UPDATE verification_private.policy SET report_history_limit=(SELECT count(*)+1 FROM verification_private.report_history) WHERE singleton;
 INSERT INTO verification_private.report_history(actor_hash,last_reports,last_cleared_at,expires_at)
 SELECT sha256(convert_to('owned-report-history-expired-'||n,'UTF8')),'{}'::timestamptz[],stamp-interval '73 hours',stamp-interval '1 hour'
 FROM generate_series(1,130) n;
 FOR attempt IN 1..3 LOOP
  EXECUTE 'SET LOCAL ROLE board_report_admission_owner';
  SELECT content.record_report_challenge_history(ARRAY[b],ARRAY[stamp],false) INTO result;
  IF result IS DISTINCT FROM (attempt=3) THEN RAISE EXCEPTION 'History cleanup returned the wrong capacity result'; END IF;
  EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
  SELECT count(*) INTO remaining FROM verification_private.report_history WHERE expires_at<=stamp;
  expected:=greatest(130-attempt*64,0);
  IF remaining<>expected THEN RAISE EXCEPTION 'History cleanup lost bounded progress'; END IF;
 END LOOP;
 EXECUTE 'SET LOCAL ROLE board_report_admission_owner';
 PERFORM content.invalidate_report_challenge_history(stamp+interval '1 day');
 PERFORM content.invalidate_report_challenge_history(stamp);
 PERFORM content.record_report_challenge_history(ARRAY[a],ARRAY[stamp+interval '1 day'],false);
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
 IF (SELECT report_history_complete_after FROM verification_private.policy WHERE singleton)<stamp+interval '4 days' THEN
  RAISE EXCEPTION 'Future omitted events outlived the incomplete-history watermark'; END IF;
 IF NOT EXISTS(SELECT 1 FROM verification_private.report_history WHERE actor_hash=a
  AND last_reports[1]=stamp+interval '1 day' AND expires_at>=stamp+interval '25 hours') THEN
  RAISE EXCEPTION 'Finite future actions lost their source window'; END IF;
 EXECUTE 'SET LOCAL ROLE board_public';
 BEGIN PERFORM content.record_report_challenge_history(ARRAY[a],ARRAY[stamp],false);
  RAISE EXCEPTION 'Public caller could forge report history'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM content.invalidate_report_challenge_history(stamp);
  RAISE EXCEPTION 'Public caller could invalidate report history'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM actor_hash FROM verification_private.report_history;
  RAISE EXCEPTION 'Public caller could read report identities'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $history$;
RESET ROLE;
ROLLBACK;

-- Missing inherited-group stamping must make the dependent history gate unready.
BEGIN;
SET LOCAL ROLE board_report_admission_owner;
ALTER TABLE post_secrets.report_membership DISABLE TRIGGER report_membership_group_insert;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.report_challenge_history_ready() IS DISTINCT FROM false THEN
  RAISE EXCEPTION 'History readiness ignored its disabled predecessor'; END IF;
END $$;
ROLLBACK;
SET ROLE board_verifier;
DO $$ BEGIN
 IF content.report_challenge_history_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'History readiness did not recover after rollback'; END IF;
END $$;
RESET ROLE;
