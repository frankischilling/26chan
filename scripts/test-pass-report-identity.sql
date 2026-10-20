-- Owned report identity, public receipt admission and private authority checks.
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_enabled)
 VALUES('qpassrpt','Owned report membership','',2000,100,100,100,10,false);
DO $$ DECLARE post bigint:=nextval('content.post_number'); BEGIN
 INSERT INTO content.threads(id,board) VALUES(post,'qpassrpt');
 INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(post,'qpassrpt',post,'Anonymous','Owned report target','Owned report identity target');
 PERFORM set_config('board.fixture_report_post',post::text,true);
END $$;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at,automatic_identity)
 SELECT sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),n-100,n-100,n-100,n-100,n+3600,gen_random_uuid()
 FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) clock;
SET LOCAL ROLE board_anonymous_owner;
WITH account AS (INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
 VALUES(sha256('rpt-account'),repeat('owned-placeholder',3),0,'2020-01-01',statement_timestamp()+interval '1 year') RETURNING id)
INSERT INTO verification_private.pass_sessions SELECT sha256('rpt-member'),id,1,statement_timestamp(),statement_timestamp()+interval '1 day' FROM account;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_write_receipt(sha256('rpt-member'),sha256('rpt-proof'),sha256('peer'),sha256('login'),sha256('posting'),sha256('rpt-anon'),sha256('form'),'report','qpassrpt',current_setting('board.fixture_report_post')::bigint))<>'ready' THEN
 RAISE EXCEPTION 'Owned report receipt unavailable'; END IF; END $$;
SAVEPOINT public_boundary;
SET LOCAL ROLE board_migrator;
UPDATE content.boards SET captcha_enabled=true WHERE slug='qpassrpt';
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('rpt-proof'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('rpt-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
DO $$ BEGIN
 PERFORM content.admit_report('qpassrpt',current_setting('board.fixture_report_post')::bigint,'Owned public membership proof',sha256('rpt-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint);
 BEGIN
  PERFORM verification_private.pass_consume_report_receipt(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-anon'),sha256('rpt-proof'),sha256('peer'),sha256('form'));
  RAISE EXCEPTION 'Public runtime staged private report authority';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT count(*) FROM verification_private.pass_reports WHERE identity_hash=sha256('rpt-account'))<>1 OR EXISTS(SELECT 1 FROM verification_private.pass_report_handoffs) THEN RAISE EXCEPTION 'Public report did not consume membership'; END IF; END $$;
ROLLBACK TO public_boundary;
SAVEPOINT revoked_public;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_entitlements SET status=1 WHERE token_hash=sha256('rpt-account');
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('rpt-proof'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('rpt-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
DO $$ BEGIN
 BEGIN PERFORM content.admit_report('qpassrpt',current_setting('board.fixture_report_post')::bigint,'Owned public report state',sha256('rpt-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint); RAISE EXCEPTION 'Revoked report membership was accepted';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
ROLLBACK TO revoked_public;
SAVEPOINT revoked_public;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_entitlements SET status=4 WHERE token_hash=sha256('rpt-account');
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('rpt-proof'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('rpt-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
DO $$ BEGIN
 BEGIN PERFORM content.admit_report('qpassrpt',current_setting('board.fixture_report_post')::bigint,'Owned public report state',sha256('rpt-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint); RAISE EXCEPTION 'Revoked report membership was accepted';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
ROLLBACK TO revoked_public;
SAVEPOINT disabled_board_member;
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('rpt-proof'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('rpt-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
SELECT content.admit_report('qpassrpt',current_setting('board.fixture_report_post')::bigint,'Owned public report state',sha256('rpt-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint);
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT count(*) FROM verification_private.pass_reports WHERE identity_hash=sha256('rpt-account'))<>1 THEN RAISE EXCEPTION 'CAPTCHA-off report discarded valid membership'; END IF; END $$;
ROLLBACK TO disabled_board_member;

SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 BEGIN
  PERFORM verification_private.pass_consume_report_receipt(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_report_post')::bigint+1,sha256('rpt-anon'),sha256('rpt-proof'),sha256('peer'),sha256('form'));
  RAISE EXCEPTION 'Report proof accepted another target';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
SAVEPOINT actor_mismatch;
SELECT verification_private.pass_consume_report_receipt(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-anon'),sha256('rpt-proof'),sha256('peer'),sha256('form'));
SET LOCAL ROLE board_public;
SELECT content.admit_report('qpassrpt',current_setting('board.fixture_report_post')::bigint,'Owned other reporter',sha256('different-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint);
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF EXISTS(SELECT 1 FROM verification_private.pass_reports) OR (SELECT count(*) FROM verification_private.pass_report_handoffs)<>1 THEN RAISE EXCEPTION 'Mismatched reporter inherited membership'; END IF; END $$;
SET CONSTRAINTS verification_private.pass_report_handoff_cleanup IMMEDIATE;
DO $$ BEGIN IF EXISTS(SELECT 1 FROM verification_private.pass_report_handoffs) THEN RAISE EXCEPTION 'Unused handoff survived cleanup'; END IF; END $$;
ROLLBACK TO actor_mismatch;
SET LOCAL ROLE board_anonymous_owner;
SELECT verification_private.pass_consume_report_receipt(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-anon'),sha256('rpt-proof'),sha256('peer'),sha256('form'));
DO $$ BEGIN
 BEGIN
  PERFORM verification_private.pass_consume_report_receipt(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-anon'),sha256('rpt-proof'),sha256('peer'),sha256('form'));
  RAISE EXCEPTION 'Report receipt replay succeeded';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
SET LOCAL ROLE board_public;
SELECT set_config('board.fixture_report_id',content.admit_report('qpassrpt',current_setting('board.fixture_report_post')::bigint,'Owned matched reporter',sha256('rpt-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint)::text,true);
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF (SELECT identity_hash FROM verification_private.pass_reports WHERE report_id=current_setting('board.fixture_report_id')::bigint) IS DISTINCT FROM sha256('rpt-account') OR EXISTS(SELECT 1 FROM verification_private.pass_report_handoffs) THEN RAISE EXCEPTION 'Accepted report did not capture membership'; END IF;
 IF has_table_privilege('board_public','verification_private.pass_reports','SELECT,INSERT,UPDATE,DELETE')
 OR has_table_privilege('board_staff','verification_private.pass_reports','SELECT,INSERT,UPDATE,DELETE')
 OR has_table_privilege('board_verifier','verification_private.pass_reports','SELECT,INSERT,UPDATE,DELETE')
 OR has_function_privilege('board_report_admission_owner','verification_private.pass_consume_report_receipt(bytea,text,bigint,bytea,bytea,bytea,bytea)','EXECUTE') THEN RAISE EXCEPTION 'Report membership authority leaked'; END IF;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_write_receipt(sha256('rpt-member'),sha256('rpt-second-proof'),sha256('peer'),sha256('login'),sha256('posting'),sha256('rpt-anon'),sha256('form'),'report','qpassrpt',current_setting('board.fixture_report_post')::bigint))<>'ready' THEN RAISE EXCEPTION 'Second report receipt unavailable'; END IF; END $$;
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('rpt-second-proof'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('rpt-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
DO $$ BEGIN
 BEGIN PERFORM content.admit_report('qpassrpt',current_setting('board.fixture_report_post')::bigint,'Owned public report state',sha256('rpt-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint); RAISE EXCEPTION 'Duplicate Pass report admitted';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN IF SQLERRM<>'You have already reported this post.' THEN RAISE; END IF; END;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=sha256('rpt-second-proof')) OR EXISTS(SELECT 1 FROM verification_private.pass_report_handoffs) THEN RAISE EXCEPTION 'Quota failure consumed or retained partial membership'; END IF; END $$;
SAVEPOINT retirement;
SET LOCAL ROLE board_report_admission_owner;
DELETE FROM post_secrets.report_membership WHERE report_id=current_setting('board.fixture_report_id')::bigint;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF EXISTS(SELECT 1 FROM verification_private.pass_reports WHERE report_id=current_setting('board.fixture_report_id')::bigint) THEN RAISE EXCEPTION 'Retired report retained membership'; END IF; END $$;
ROLLBACK TO retirement;
SAVEPOINT capture_drift;
SET LOCAL ROLE board_report_admission_owner;
ALTER TABLE post_secrets.report_membership DISABLE TRIGGER pass_report_identity_capture;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Capture drift accepted'; END IF; END $$;
ROLLBACK TO capture_drift;
SAVEPOINT fk_drift;
RESET ROLE;
ALTER TABLE verification_private.pass_reports DISABLE TRIGGER ALL;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Cascade drift accepted'; END IF; END $$;
ROLLBACK TO fk_drift;
ROLLBACK;
