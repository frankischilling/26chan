-- Owned report identities; public receipt admission remains disconnected.
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
SET LOCAL ROLE board_migrator;
SELECT set_config('board.fixture_report_revision',content.import_report_catalog('{
 "version":1,"categories":[{"id":1,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Owned half","weight":0.5,"filtered":0}]}')::text,true);
SELECT content.set_report_catalog_active(current_setting('board.fixture_report_revision')::bigint);
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at,automatic_identity)
 SELECT sha256('rpt-anon-two'),sha256('network'),sha256('address'),sha256('environment'),n-100,n-100,n-100,n-100,n+3600,gen_random_uuid()
 FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) clock;
SET LOCAL ROLE board_anonymous_owner;
SELECT verification_private.pass_consume_report_receipt(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-anon'),sha256('rpt-proof'),sha256('peer'),sha256('form'));
SET LOCAL ROLE board_public;
SELECT set_config('board.fixture_clear_report',content.admit_categorical_report('qpassrpt',current_setting('board.fixture_report_post')::bigint,1,current_setting('board.fixture_report_revision')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint)::text,true);
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF EXISTS(SELECT 1 FROM verification_private.pass_report_clear_events) THEN RAISE EXCEPTION 'Uncleared report created clear history'; END IF; END $$;
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 IF content.clear_report_group('qpassrpt',current_setting('board.fixture_report_post')::bigint,7) IS DISTINCT FROM 1::bigint THEN RAISE EXCEPTION 'Wrong clear count'; END IF;
 IF content.clear_report_group('qpassrpt',current_setting('board.fixture_report_post')::bigint,7) IS DISTINCT FROM 0::bigint THEN RAISE EXCEPTION 'Clear retry was not idempotent'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF (SELECT count(*) FROM verification_private.pass_report_clear_events)<>1
 OR NOT EXISTS(SELECT 1 FROM verification_private.pass_report_clear_events WHERE report_id=current_setting('board.fixture_clear_report')::bigint
 AND actor_hash=sha256('rpt-actor') AND identity_hash=sha256('rpt-account') AND automatic_identity IS NOT NULL) THEN
 RAISE EXCEPTION 'Manual clear omitted final reporter identity'; END IF;
END $$;
SET LOCAL ROLE board_public;
SELECT set_config('board.fixture_inherited_report',content.admit_categorical_report('qpassrpt',current_setting('board.fixture_report_post')::bigint,1,current_setting('board.fixture_report_revision')::bigint,sha256('rpt-actor-two'),sha256('rpt-anon-two'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint)::text,true);
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF (SELECT count(*) FROM verification_private.pass_report_clear_events)<>2
 OR NOT EXISTS(SELECT 1 FROM verification_private.pass_report_clear_events WHERE report_id=current_setting('board.fixture_inherited_report')::bigint
 AND actor_hash=sha256('rpt-actor-two') AND identity_hash IS NULL AND automatic_identity IS NOT NULL) THEN
 RAISE EXCEPTION 'Inherited clear omitted identity or invented membership'; END IF;
END $$;
SAVEPOINT capacity;
UPDATE verification_private.pass_report_history_policy SET event_limit=2,complete_after=statement_timestamp()-interval '1 second';
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 IF content.pass_record_report_clears(ARRAY[99999999::bigint],ARRAY[sha256('owned-capacity')],ARRAY[NULL::uuid],clock_timestamp()) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Full history did not report omission'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT count(*) FROM verification_private.pass_report_clear_events)<>2
 OR (SELECT complete_after FROM verification_private.pass_report_history_policy WHERE singleton)<clock_timestamp()+interval '4 days' THEN RAISE EXCEPTION 'Capacity omission lost its uncertainty'; END IF; END $$;
ROLLBACK TO capacity;
SAVEPOINT expiry;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_report_clear_events SET created_at=statement_timestamp()-interval '6 days' WHERE report_id=current_setting('board.fixture_clear_report')::bigint;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_record_report_clears(ARRAY[99999998::bigint],ARRAY[sha256('owned-expiry')],ARRAY[NULL::uuid],clock_timestamp()) IS DISTINCT FROM true THEN RAISE EXCEPTION 'Expired history was not reclaimed'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF EXISTS(SELECT 1 FROM verification_private.pass_report_clear_events WHERE report_id=current_setting('board.fixture_clear_report')::bigint)
 OR (SELECT count(*) FROM verification_private.pass_report_clear_events)<>2 THEN RAISE EXCEPTION 'Wrong history expiry boundary'; END IF; END $$;
ROLLBACK TO expiry;
SAVEPOINT history_grant;
SET LOCAL ROLE board_anonymous_owner;
GRANT SELECT ON verification_private.pass_report_clear_events TO board_public;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'History grant drift was accepted'; END IF; END $$;
ROLLBACK TO history_grant;
SAVEPOINT identity_change;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 BEGIN
  PERFORM content.pass_record_report_clears(ARRAY[current_setting('board.fixture_clear_report')::bigint],ARRAY[sha256('changed')],ARRAY[NULL::uuid],clock_timestamp());
  RAISE EXCEPTION 'Clear identity overwrite succeeded';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
ROLLBACK TO identity_change;
SET LOCAL ROLE board_report_admission_owner;
DELETE FROM post_secrets.report_membership WHERE board='qpassrpt';
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT count(*) FROM verification_private.pass_report_clear_events)<>2 OR EXISTS(SELECT 1 FROM verification_private.pass_reports) THEN RAISE EXCEPTION 'Membership retirement changed clear history'; END IF; END $$;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.pass_record_report_clears(ARRAY[99999999::bigint],ARRAY[sha256('untrusted')],ARRAY[NULL::uuid],clock_timestamp());
  RAISE EXCEPTION 'Public role recorded clear history';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
