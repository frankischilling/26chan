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
SELECT set_config('board.fixture_auto',(SELECT automatic_identity::text FROM post_secrets.anonymous_sessions WHERE token_hash=sha256('rpt-anon')),true);
SELECT set_config('board.fixture_request',floor(extract(epoch FROM clock_timestamp()))::bigint::text,true);
SET LOCAL ROLE board_anonymous_owner;
SELECT verification_private.pass_consume_report_receipt(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-anon'),sha256('rpt-proof'),sha256('peer'),sha256('form'));
SET LOCAL ROLE board_public;
SELECT content.bind_report_restrictions(sha256('rpt-actor'),sha256('rpt-network-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,current_setting('board.fixture_request')::bigint);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM NULL THEN RAISE EXCEPTION 'Incomplete history became a negative proof'; END IF; END $$;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,0) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Disabled filter did not return false'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_report_history_policy SET complete_after=statement_timestamp()-interval '1 second';
UPDATE verification_private.policy SET report_history_epoch=sha256('owned-epoch'),report_history_epoch_tainted=false,report_history_complete_after=statement_timestamp()-interval '1 second';
SELECT set_config('board.report_history_epoch',encode(sha256('owned-epoch'),'hex'),true);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Complete empty history did not qualify'; END IF; END $$;
SAVEPOINT clear_case;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_report_clear_events VALUES(99999991,sha256('other'),NULL,sha256('rpt-account'),statement_timestamp());
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM true THEN RAISE EXCEPTION 'member-other-address'; END IF; END $$;
ROLLBACK TO clear_case;
SAVEPOINT clear_case;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_report_clear_events VALUES(99999991,sha256('rpt-actor'),NULL,sha256('other-account'),statement_timestamp());
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM false THEN RAISE EXCEPTION 'member-excludes-address'; END IF; END $$;
ROLLBACK TO clear_case;
SAVEPOINT clear_case;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_report_clear_events VALUES(99999991,sha256('other'),current_setting('board.fixture_auto')::uuid,sha256('other-account'),statement_timestamp());
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM true THEN RAISE EXCEPTION 'password-secondary'; END IF; END $$;
ROLLBACK TO clear_case;
SAVEPOINT clear_case;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_report_clear_events VALUES(99999991,sha256('rpt-actor'),current_setting('board.fixture_auto')::uuid,sha256('rpt-account'),statement_timestamp());
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,2) IS DISTINCT FROM false THEN RAISE EXCEPTION 'overlap-counts-once'; END IF; END $$;
ROLLBACK TO clear_case;
SAVEPOINT fresh_password;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_report_clear_events VALUES(99999991,sha256('other'),current_setting('board.fixture_auto')::uuid,sha256('other-account'),statement_timestamp());
SET LOCAL ROLE board_migrator;
UPDATE post_secrets.anonymous_sessions SET created_at=current_setting('board.fixture_request')::bigint WHERE token_hash=sha256('rpt-anon');
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM false THEN RAISE EXCEPTION 'New password retained history branch'; END IF; END $$;
ROLLBACK TO fresh_password;
SAVEPOINT ordinary;
SET LOCAL ROLE board_anonymous_owner;
DELETE FROM verification_private.pass_report_handoffs;
INSERT INTO verification_private.pass_report_clear_events VALUES(99999991,sha256('rpt-actor'),NULL,sha256('other-account'),statement_timestamp());
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM true THEN RAISE EXCEPTION 'Ordinary reporter lost address history'; END IF; END $$;
ROLLBACK TO ordinary;
SAVEPOINT cutoff;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_report_clear_events VALUES(99999991,sha256('other'),NULL,sha256('rpt-account'),statement_timestamp()-interval '2 days');
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Exact two-day boundary counted'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_report_clear_events SET created_at=clock_timestamp()-interval '2 days'+interval '1 minute';
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM true THEN RAISE EXCEPTION 'Inside two-day boundary omitted'; END IF; END $$;
ROLLBACK TO cutoff;
SAVEPOINT rotation;
SELECT set_config('board.report_history_epoch',encode(sha256('changed-epoch'),'hex'),true);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM NULL THEN RAISE EXCEPTION 'Changed epoch granted negative history'; END IF; END $$;
ROLLBACK TO rotation;
SAVEPOINT bans;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,pass_identity_hash,template_id,upload_observation,retired_at,retirement)
 SELECT 'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('other'),current_setting('board.fixture_auto')::uuid,false,false,false,statement_timestamp()-interval '1 day',statement_timestamp()+interval '1 day','Owned history','',7,536,'staff',sha256('rpt-account'),190,'absent',statement_timestamp(),'expiration' FROM generate_series(1,2);
UPDATE restrictions.capacity SET used_bytes=used_bytes+1072 WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Ban history case 2-False-190-1 day'; END IF; END $$;
ROLLBACK TO bans;
SAVEPOINT bans;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,pass_identity_hash,template_id,upload_observation,retired_at,retirement)
 SELECT 'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('other'),current_setting('board.fixture_auto')::uuid,false,false,false,statement_timestamp()-interval '1 day',statement_timestamp()+interval '1 day','Owned history','',7,536,'staff',sha256('rpt-account'),190,'absent',statement_timestamp(),'expiration' FROM generate_series(1,3);
UPDATE restrictions.capacity SET used_bytes=used_bytes+1608 WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM true THEN RAISE EXCEPTION 'Ban history case 3-False-190-1 day'; END IF; END $$;
ROLLBACK TO bans;
SAVEPOINT bans;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,pass_identity_hash,template_id,upload_observation,retired_at,retirement)
 SELECT 'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('other'),current_setting('board.fixture_auto')::uuid,false,false,true,statement_timestamp()-interval '1 day',statement_timestamp()+interval '1 day','Owned history','',7,536,'staff',sha256('rpt-account'),190,'absent',NULL,NULL FROM generate_series(1,3);
UPDATE restrictions.capacity SET used_bytes=used_bytes+1608 WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Ban history case 3-True-190-1 day'; END IF; END $$;
ROLLBACK TO bans;
SAVEPOINT bans;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,pass_identity_hash,template_id,upload_observation,retired_at,retirement)
 SELECT 'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('other'),current_setting('board.fixture_auto')::uuid,false,false,false,statement_timestamp()-interval '1 day',statement_timestamp()+interval '1 day','Owned history','',7,536,'staff',sha256('rpt-account'),191,'absent',statement_timestamp(),'expiration' FROM generate_series(1,3);
UPDATE restrictions.capacity SET used_bytes=used_bytes+1608 WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Ban history case 3-False-191-1 day'; END IF; END $$;
ROLLBACK TO bans;
SAVEPOINT bans;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,pass_identity_hash,template_id,upload_observation,retired_at,retirement)
 SELECT 'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('other'),current_setting('board.fixture_auto')::uuid,false,false,false,statement_timestamp()-interval '31 days',statement_timestamp()+interval '1 day','Owned history','',7,536,'staff',sha256('rpt-account'),190,'absent',statement_timestamp(),'expiration' FROM generate_series(1,3);
UPDATE restrictions.capacity SET used_bytes=used_bytes+1608 WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Ban history case 3-False-190-31 days'; END IF; END $$;
ROLLBACK TO bans;
SAVEPOINT bans;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,pass_identity_hash,template_id,upload_observation,retired_at,retirement)
 SELECT 'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('other'),current_setting('board.fixture_auto')::uuid,false,false,false,statement_timestamp()-interval '30 days',statement_timestamp()+interval '1 day','Owned history','',7,536,'staff',sha256('rpt-account'),190,'absent',statement_timestamp(),'expiration' FROM generate_series(1,3);
UPDATE restrictions.capacity SET used_bytes=used_bytes+1608 WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Ban history case 3-False-190-30 days'; END IF; END $$;
ROLLBACK TO bans;
SAVEPOINT bans;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,pass_identity_hash,template_id,upload_observation,retired_at,retirement)
 SELECT 'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('other'),current_setting('board.fixture_auto')::uuid,false,false,false,statement_timestamp()-interval '-1 minute',statement_timestamp()+interval '1 day','Owned history','',7,536,'staff',sha256('rpt-account'),190,'absent',statement_timestamp(),'expiration' FROM generate_series(1,3);
UPDATE restrictions.capacity SET used_bytes=used_bytes+1608 WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN IF content.pass_report_history_filtered('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1) IS DISTINCT FROM true THEN RAISE EXCEPTION 'Ban history case 3-False-190--1 minute'; END IF; END $$;
ROLLBACK TO bans;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.pass_report_history_filtered('qpassrpt',1,sha256('rpt-actor'),sha256('rpt-anon'),current_setting('board.fixture_request')::bigint,1);
  RAISE EXCEPTION 'Public history query succeeded';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
