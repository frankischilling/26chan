DO $$ DECLARE role_name text; BEGIN
 FOREACH role_name IN ARRAY ARRAY['board_public','board_staff','board_auth','board_migrator'] LOOP
  IF has_function_privilege(role_name,'post_secrets.janitor_request_unlocked(text,bigint)','EXECUTE') THEN
   RAISE EXCEPTION 'Runtime role gained private request-weight access: %',role_name; END IF;
 END LOOP;
 IF EXISTS(SELECT 1 FROM restrictions.templates WHERE request_can_warn OR request_save_policy IS NOT NULL) THEN
  RAISE EXCEPTION 'Request configuration was inferred'; END IF;
END $$;
-- Synthetic retained reports exercise the same private persisted evidence the
-- eventual request writer will consume. No runtime evidence setter is added.
SET ROLE board_migrator;
INSERT INTO content.reports(board,post_id,reason)
 SELECT 'demo',1000001,'Owned OP boundary' FROM generate_series(1,2398);
SET ROLE board_migrator;
INSERT INTO content.reports(board,post_id,reason)
 SELECT 'demo',1000002,'Owned reply boundary' FROM generate_series(1,2998);
SET ROLE board_report_admission_owner;
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,effective_weight,numeric_proof,evaluated_at)
 SELECT id,1,0.5,'BaseEqualsFallback',clock_timestamp() FROM content.reports WHERE board='demo';
RESET ROLE;
SET ROLE board_restriction_owner;
DO $$ BEGIN
 IF post_secrets.janitor_request_unlocked('demo',1000001)
  OR post_secrets.janitor_request_unlocked('demo',1000002) THEN
  RAISE EXCEPTION 'Subthreshold evidence unlocked'; END IF;
END $$;
RESET ROLE;
SET ROLE board_migrator;
INSERT INTO content.reports(board,post_id,reason) VALUES('demo',1000001,'Owned OP ceiling'),('demo',1000002,'Owned reply ceiling');
SET ROLE board_report_admission_owner;
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,effective_weight,numeric_proof,evaluated_at)
 SELECT r.id,1,0.5,'BaseEqualsFallback',clock_timestamp() FROM content.reports r
 WHERE r.board='demo' AND NOT EXISTS(SELECT 1 FROM post_secrets.report_weight_evidence e WHERE e.report_id=r.id);
RESET ROLE;
SET ROLE board_restriction_owner;
DO $$ BEGIN
 IF NOT post_secrets.janitor_request_unlocked('demo',1000001)
  OR NOT post_secrets.janitor_request_unlocked('demo',1000002) THEN
  RAISE EXCEPTION 'Ceiling threshold differs from source'; END IF;
END $$;
RESET ROLE;
-- Group clearing does not remove the source's retained report weight.
UPDATE content.reports SET group_cleared_at=clock_timestamp(),group_cleared_by=1,group_clear_inherited=false WHERE board='demo';
SET ROLE board_restriction_owner;
DO $$ BEGIN IF NOT post_secrets.janitor_request_unlocked('demo',1000001) THEN
 RAISE EXCEPTION 'Group clearing incorrectly removed request evidence'; END IF; END $$;
RESET ROLE;
-- Reporter retirement removes weight, unlike ordinary group clearing.
BEGIN;
UPDATE content.reports SET reporter_cleared_at=clock_timestamp()
 WHERE id=(SELECT min(id) FROM content.reports WHERE board='demo' AND post_id=1000002);
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF post_secrets.janitor_request_unlocked('demo',1000002) THEN
 RAISE EXCEPTION 'Retired reporter weight unlocked a request'; END IF; END $$;
ROLLBACK;
SET ROLE board_restriction_owner;
DO $$ BEGIN
 IF post_secrets.janitor_request_unlocked('missing',1)
  OR post_secrets.janitor_request_unlocked('demo',9999999) THEN
  RAISE EXCEPTION 'Missing target unlocked'; END IF;
 BEGIN
  PERFORM post_secrets.janitor_request_unlocked(NULL,1);
  RAISE EXCEPTION 'NULL target accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
RESET ROLE;
BEGIN ISOLATION LEVEL REPEATABLE READ;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 BEGIN
  PERFORM post_secrets.janitor_request_unlocked('demo',1000001);
  RAISE EXCEPTION 'Stale-snapshot request evidence accepted';
 EXCEPTION WHEN SQLSTATE 'P0206' THEN NULL; END;
END $$;
ROLLBACK;
SET ROLE board_staff;
DO $$ BEGIN
 BEGIN
  PERFORM post_secrets.janitor_request_unlocked('demo',1000001);
  RAISE EXCEPTION 'Staff runtime called private request predicate';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
RESET ROLE;
-- A single unknown report must fail closed, even if the known subtotal qualifies.
SET ROLE board_migrator;
INSERT INTO content.reports(board,post_id,reason) VALUES('demo',1000001,'Owned unknown weight');
SET ROLE board_restriction_owner;
DO $$ BEGIN IF post_secrets.janitor_request_unlocked('demo',1000001) THEN
 RAISE EXCEPTION 'Unknown weight unlocked a request'; END IF; END $$;
RESET ROLE;
BEGIN;
GRANT EXECUTE ON FUNCTION post_secrets.janitor_request_unlocked(text,bigint) TO board_staff;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Private helper grant drift was accepted'; END IF; END $$;
ROLLBACK;
BEGIN;
ALTER FUNCTION post_secrets.janitor_request_unlocked(text,bigint) SECURITY INVOKER;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Private helper definition drift was accepted'; END IF; END $$;
ROLLBACK;
