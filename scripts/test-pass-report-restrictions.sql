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
SELECT set_config('board.fixture_report_automatic',(SELECT automatic_identity::text FROM post_secrets.anonymous_sessions WHERE token_hash=sha256('rpt-anon')),true);
SET LOCAL ROLE board_anonymous_owner;
WITH account AS (INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
 VALUES(sha256('rpt-account'),repeat('owned-placeholder',3),0,'2020-01-01',statement_timestamp()+interval '1 year') RETURNING id)
INSERT INTO verification_private.pass_sessions SELECT sha256('rpt-member'),id,1,statement_timestamp(),statement_timestamp()+interval '1 day' FROM account;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_write_receipt(sha256('rpt-member'),sha256('rpt-proof'),sha256('peer'),sha256('login'),sha256('posting'),sha256('rpt-anon'),sha256('form'),'report','qpassrpt',current_setting('board.fixture_report_post')::bigint))<>'ready' THEN
 RAISE EXCEPTION 'Owned report receipt unavailable'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
SELECT verification_private.pass_consume_report_receipt(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-anon'),sha256('rpt-proof'),sha256('peer'),sha256('form'));
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF verification_private.pass_report_identity(sha256('rpt-actor'),sha256('wrong-session'),'qpassrpt',current_setting('board.fixture_report_post')::bigint) IS NOT NULL
 OR verification_private.pass_report_identity(sha256('wrong-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint) IS NOT NULL THEN RAISE EXCEPTION 'Report identity crossed its binding'; END IF; END $$;
-- member-other-address
SAVEPOINT report_ban_case;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest,pass_identity_hash,automatic_identity)
 VALUES('qpassrpt',NULL,sha256('unrelated'),false,false,true,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Owned report ban','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('rpt-account'),NULL);
UPDATE restrictions.capacity SET used_bytes=used_bytes+536 WHERE singleton;
SET LOCAL ROLE board_public;
SELECT content.bind_report_restrictions(sha256('rpt-actor'),sha256('rpt-network-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,floor(extract(epoch FROM clock_timestamp()))::bigint);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 BEGIN
  PERFORM content.assert_reporting_restrictions('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'));
  IF true THEN RAISE EXCEPTION 'Expected member-other-address refusal'; END IF;
 EXCEPTION WHEN SQLSTATE 'P135B' THEN IF NOT true THEN RAISE EXCEPTION 'Unexpected member-other-address refusal'; END IF; END;
END $$;
ROLLBACK TO report_ban_case;
-- member-other-board
SAVEPOINT report_ban_case;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest,pass_identity_hash,automatic_identity)
 VALUES('g',NULL,sha256('unrelated'),false,false,true,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Owned report ban','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('rpt-account'),NULL);
UPDATE restrictions.capacity SET used_bytes=used_bytes+536 WHERE singleton;
SET LOCAL ROLE board_public;
SELECT content.bind_report_restrictions(sha256('rpt-actor'),sha256('rpt-network-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,floor(extract(epoch FROM clock_timestamp()))::bigint);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 BEGIN
  PERFORM content.assert_reporting_restrictions('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'));
  IF false THEN RAISE EXCEPTION 'Expected member-other-board refusal'; END IF;
 EXCEPTION WHEN SQLSTATE 'P135B' THEN IF NOT false THEN RAISE EXCEPTION 'Unexpected member-other-board refusal'; END IF; END;
END $$;
ROLLBACK TO report_ban_case;
-- member-global
SAVEPOINT report_ban_case;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest,pass_identity_hash,automatic_identity)
 VALUES('g',NULL,sha256('unrelated'),true,false,true,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Owned report ban','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('rpt-account'),NULL);
UPDATE restrictions.capacity SET used_bytes=used_bytes+536 WHERE singleton;
SET LOCAL ROLE board_public;
SELECT content.bind_report_restrictions(sha256('rpt-actor'),sha256('rpt-network-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,floor(extract(epoch FROM clock_timestamp()))::bigint);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 BEGIN
  PERFORM content.assert_reporting_restrictions('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'));
  IF true THEN RAISE EXCEPTION 'Expected member-global refusal'; END IF;
 EXCEPTION WHEN SQLSTATE 'P135B' THEN IF NOT true THEN RAISE EXCEPTION 'Unexpected member-global refusal'; END IF; END;
END $$;
ROLLBACK TO report_ban_case;
-- address-still-banned
SAVEPOINT report_ban_case;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest,pass_identity_hash,automatic_identity)
 VALUES('qpassrpt',NULL,sha256('rpt-network-actor'),false,false,true,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Owned report ban','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('other-account'),NULL);
UPDATE restrictions.capacity SET used_bytes=used_bytes+536 WHERE singleton;
SET LOCAL ROLE board_public;
SELECT content.bind_report_restrictions(sha256('rpt-actor'),sha256('rpt-network-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,floor(extract(epoch FROM clock_timestamp()))::bigint);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 BEGIN
  PERFORM content.assert_reporting_restrictions('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'));
  IF true THEN RAISE EXCEPTION 'Expected address-still-banned refusal'; END IF;
 EXCEPTION WHEN SQLSTATE 'P135B' THEN IF NOT true THEN RAISE EXCEPTION 'Unexpected address-still-banned refusal'; END IF; END;
END $$;
ROLLBACK TO report_ban_case;
-- password-still-banned
SAVEPOINT report_ban_case;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest,pass_identity_hash,automatic_identity)
 VALUES('qpassrpt',NULL,sha256('unrelated'),false,false,true,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Owned report ban','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('other-account'),current_setting('board.fixture_report_automatic')::uuid);
UPDATE restrictions.capacity SET used_bytes=used_bytes+536 WHERE singleton;
SET LOCAL ROLE board_public;
SELECT content.bind_report_restrictions(sha256('rpt-actor'),sha256('rpt-network-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,floor(extract(epoch FROM clock_timestamp()))::bigint);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 BEGIN
  PERFORM content.assert_reporting_restrictions('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'));
  IF true THEN RAISE EXCEPTION 'Expected password-still-banned refusal'; END IF;
 EXCEPTION WHEN SQLSTATE 'P135B' THEN IF NOT true THEN RAISE EXCEPTION 'Unexpected password-still-banned refusal'; END IF; END;
END $$;
ROLLBACK TO report_ban_case;
-- inactive-member
SAVEPOINT report_ban_case;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest,pass_identity_hash,automatic_identity)
 VALUES('qpassrpt',NULL,sha256('unrelated'),false,false,true,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Owned report ban','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('rpt-account'),NULL);
UPDATE restrictions.bans SET active=false,retired_at=statement_timestamp(),retirement='expiration' WHERE board='qpassrpt';
UPDATE restrictions.capacity SET used_bytes=used_bytes+536 WHERE singleton;
SET LOCAL ROLE board_public;
SELECT content.bind_report_restrictions(sha256('rpt-actor'),sha256('rpt-network-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,floor(extract(epoch FROM clock_timestamp()))::bigint);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 BEGIN
  PERFORM content.assert_reporting_restrictions('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'));
  IF false THEN RAISE EXCEPTION 'Expected inactive-member refusal'; END IF;
 EXCEPTION WHEN SQLSTATE 'P135B' THEN IF NOT false THEN RAISE EXCEPTION 'Unexpected inactive-member refusal'; END IF; END;
END $$;
ROLLBACK TO report_ban_case;
-- different-member
SAVEPOINT report_ban_case;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest,pass_identity_hash,automatic_identity)
 VALUES('qpassrpt',NULL,sha256('unrelated'),false,false,true,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Owned report ban','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('other-account'),NULL);
UPDATE restrictions.capacity SET used_bytes=used_bytes+536 WHERE singleton;
SET LOCAL ROLE board_public;
SELECT content.bind_report_restrictions(sha256('rpt-actor'),sha256('rpt-network-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,floor(extract(epoch FROM clock_timestamp()))::bigint);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 BEGIN
  PERFORM content.assert_reporting_restrictions('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'));
  IF false THEN RAISE EXCEPTION 'Expected different-member refusal'; END IF;
 EXCEPTION WHEN SQLSTATE 'P135B' THEN IF NOT false THEN RAISE EXCEPTION 'Unexpected different-member refusal'; END IF; END;
END $$;
ROLLBACK TO report_ban_case;
-- active-warning
SAVEPOINT report_ban_case;
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,active,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest,pass_identity_hash,automatic_identity)
 VALUES('qpassrpt',NULL,sha256('unrelated'),false,true,true,statement_timestamp()-interval '1 hour',statement_timestamp()-interval '1 hour','Owned report ban','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('rpt-account'),NULL);
UPDATE restrictions.capacity SET used_bytes=used_bytes+536 WHERE singleton;
SET LOCAL ROLE board_public;
SELECT content.bind_report_restrictions(sha256('rpt-actor'),sha256('rpt-network-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,floor(extract(epoch FROM clock_timestamp()))::bigint);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 BEGIN
  PERFORM content.assert_reporting_restrictions('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'));
  IF true THEN RAISE EXCEPTION 'Expected active-warning refusal'; END IF;
 EXCEPTION WHEN SQLSTATE 'P135B' THEN IF NOT true THEN RAISE EXCEPTION 'Unexpected active-warning refusal'; END IF; END;
END $$;
ROLLBACK TO report_ban_case;
ROLLBACK;
