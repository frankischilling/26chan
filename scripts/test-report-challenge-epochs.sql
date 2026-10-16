\set ON_ERROR_STOP on
BEGIN;
SET LOCAL ROLE board_migrator;
DO $epochs$
DECLARE a bytea:=decode(repeat('a9',32),'hex'); b bytea:=decode(repeat('b8',32),'hex');
 stamp timestamptz:=clock_timestamp(); expected_horizon timestamptz; first_horizon timestamptz; next_horizon timestamptz; current_epoch bytea;
BEGIN
 UPDATE verification_private.policy SET report_history_epoch=NULL,report_history_complete_after=stamp-interval '1 second' WHERE singleton;
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 SELECT greatest(stamp-interval '1 second',
  (SELECT max(expires_at) FROM verification_private.report_history),
  content.report_restriction_history_horizon()) INTO expected_horizon;
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 PERFORM verification_private.observe_report_epoch(a);
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
 SELECT report_history_epoch,report_history_complete_after INTO current_epoch,first_horizon FROM verification_private.policy WHERE singleton;
 IF current_epoch IS DISTINCT FROM a OR first_horizon IS DISTINCT FROM expected_horizon THEN
  RAISE EXCEPTION 'Initial epoch changed the recorded history horizon'; END IF;
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 PERFORM verification_private.observe_report_epoch(a);
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
 IF (SELECT report_history_complete_after FROM verification_private.policy WHERE singleton) IS DISTINCT FROM first_horizon THEN
  RAISE EXCEPTION 'Stable epoch extended the warm-up'; END IF;
 -- Rotation must honor events retained under a previous identity key.
 INSERT INTO verification_private.report_history(actor_hash,last_reports,last_cleared_at,expires_at)
 VALUES(decode(repeat('c7',32),'hex'),ARRAY[stamp],stamp+interval '100 days',stamp+interval '103 days');
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 PERFORM verification_private.observe_report_epoch(b);
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
 SELECT report_history_epoch,report_history_complete_after INTO current_epoch,next_horizon FROM verification_private.policy WHERE singleton;
 IF current_epoch IS DISTINCT FROM b OR (next_horizon<first_horizon OR next_horizon<stamp+interval '103 days') THEN
  RAISE EXCEPTION 'Changed epoch lost its conservative history horizon'; END IF;
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 PERFORM verification_private.observe_report_epoch(NULL);
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
 SELECT report_history_epoch,report_history_complete_after INTO current_epoch,first_horizon FROM verification_private.policy WHERE singleton;
 IF current_epoch IS DISTINCT FROM b OR (first_horizon<next_horizon OR first_horizon<stamp+interval '30 days') THEN
  RAISE EXCEPTION 'Unknown epoch erased identity or shortened the horizon'; END IF;
 UPDATE verification_private.policy SET report_history_complete_after=stamp+interval '200 days' WHERE singleton;
 PERFORM set_config('board.report_history_epoch',encode(a,'hex'),true);
 EXECUTE 'SET LOCAL ROLE board_report_admission_owner';
 PERFORM content.observe_report_challenge_epoch();
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
 IF (SELECT report_history_epoch FROM verification_private.policy WHERE singleton) IS DISTINCT FROM a
  OR (SELECT report_history_complete_after FROM verification_private.policy WHERE singleton) IS DISTINCT FROM stamp+interval '200 days' THEN
  RAISE EXCEPTION 'Typed transport epoch was not recorded'; END IF;
 PERFORM set_config('board.report_history_epoch','malformed',true);
 EXECUTE 'SET LOCAL ROLE board_report_admission_owner';
 PERFORM content.observe_report_challenge_epoch();
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_migrator';
 SELECT report_history_epoch,report_history_complete_after INTO current_epoch,next_horizon FROM verification_private.policy WHERE singleton;
 IF current_epoch IS DISTINCT FROM a OR next_horizon<first_horizon THEN
  RAISE EXCEPTION 'Malformed transport metadata changed identity or shortened history'; END IF;
 EXECUTE 'SET LOCAL ROLE board_public';
 BEGIN PERFORM content.observe_report_challenge_epoch();
  RAISE EXCEPTION 'Public caller could change the epoch outside report admission'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM content.report_restriction_history_horizon();
  RAISE EXCEPTION 'Public caller could inspect private restriction history'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 EXECUTE 'RESET ROLE'; EXECUTE 'SET LOCAL ROLE board_verifier';
 BEGIN PERFORM verification_private.observe_report_epoch(a);
  RAISE EXCEPTION 'Issuer could mutate report epochs directly'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $epochs$;
RESET ROLE;
ROLLBACK;
