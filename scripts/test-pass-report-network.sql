-- Owned report identities; public receipt admission remains disconnected.
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_enabled)
 VALUES('qpassrpt','Owned report membership','',2000,100,100,100,10,true);
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
INSERT INTO admission.network_rules(id,range_start,range_end,boards) VALUES(999824,3221225984,3221226239,'qpassrpt');
SET LOCAL ROLE board_report_admission_owner;
SELECT content.lock_report_network_policy();
-- ordinary reporter
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 PERFORM set_config('board.report_network_context',jsonb_build_object('board','qpassrpt','post',current_setting('board.fixture_report_post')::bigint,
  'actor',encode(sha256('rpt-actor'),'hex'),'request_at',floor(extract(epoch FROM clock_timestamp()))::bigint,
  'peer_known',true,'asn_known',true,'ipv4',3221225985::bigint,'asn',64500,'browser','','request','')::text,true);
 BEGIN
  PERFORM post_secrets.assert_report_network('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint);
  IF true THEN RAISE EXCEPTION 'Expected ordinary reporter refusal'; END IF;
  IF current_setting('board.report_network_context',true)<>'' THEN RAISE EXCEPTION 'Bypass retained context'; END IF;
 EXCEPTION WHEN SQLSTATE 'P179B' THEN IF NOT true THEN RAISE EXCEPTION 'Unexpected ordinary reporter refusal'; END IF; END;
END $$;
SET LOCAL ROLE board_anonymous_owner;
SELECT verification_private.pass_consume_report_receipt(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-anon'),sha256('rpt-proof'),sha256('peer'),sha256('form'));
-- consumed membership
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 PERFORM set_config('board.report_network_context',jsonb_build_object('board','qpassrpt','post',current_setting('board.fixture_report_post')::bigint,
  'actor',encode(sha256('rpt-actor'),'hex'),'request_at',floor(extract(epoch FROM clock_timestamp()))::bigint,
  'peer_known',true,'asn_known',true,'ipv4',3221225985::bigint,'asn',64500,'browser','','request','')::text,true);
 BEGIN
  PERFORM post_secrets.assert_report_network('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint);
  IF false THEN RAISE EXCEPTION 'Expected consumed membership refusal'; END IF;
  IF current_setting('board.report_network_context',true)<>'' THEN RAISE EXCEPTION 'Bypass retained context'; END IF;
 EXCEPTION WHEN SQLSTATE 'P179B' THEN IF NOT false THEN RAISE EXCEPTION 'Unexpected consumed membership refusal'; END IF; END;
END $$;
-- different session
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 PERFORM set_config('board.report_network_context',jsonb_build_object('board','qpassrpt','post',current_setting('board.fixture_report_post')::bigint,
  'actor',encode(sha256('rpt-actor'),'hex'),'request_at',floor(extract(epoch FROM clock_timestamp()))::bigint,
  'peer_known',true,'asn_known',true,'ipv4',3221225985::bigint,'asn',64500,'browser','','request','')::text,true);
 BEGIN
  PERFORM post_secrets.assert_report_network('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('wrong-session'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint);
  IF true THEN RAISE EXCEPTION 'Expected different session refusal'; END IF;
  IF current_setting('board.report_network_context',true)<>'' THEN RAISE EXCEPTION 'Bypass retained context'; END IF;
 EXCEPTION WHEN SQLSTATE 'P179B' THEN IF NOT true THEN RAISE EXCEPTION 'Unexpected different session refusal'; END IF; END;
END $$;
-- different actor
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 PERFORM set_config('board.report_network_context',jsonb_build_object('board','qpassrpt','post',current_setting('board.fixture_report_post')::bigint,
  'actor',encode(sha256('wrong-actor'),'hex'),'request_at',floor(extract(epoch FROM clock_timestamp()))::bigint,
  'peer_known',true,'asn_known',true,'ipv4',3221225985::bigint,'asn',64500,'browser','','request','')::text,true);
 BEGIN
  PERFORM post_secrets.assert_report_network('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('wrong-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint);
  IF true THEN RAISE EXCEPTION 'Expected different actor refusal'; END IF;
  IF current_setting('board.report_network_context',true)<>'' THEN RAISE EXCEPTION 'Bypass retained context'; END IF;
 EXCEPTION WHEN SQLSTATE 'P179B' THEN IF NOT true THEN RAISE EXCEPTION 'Unexpected different actor refusal'; END IF; END;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DELETE FROM verification_private.pass_report_handoffs;
SET LOCAL ROLE board_migrator;
UPDATE content.boards SET captcha_enabled=false WHERE slug='qpassrpt';
-- CAPTCHA-off board
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 PERFORM set_config('board.report_network_context',jsonb_build_object('board','qpassrpt','post',current_setting('board.fixture_report_post')::bigint,
  'actor',encode(sha256('rpt-actor'),'hex'),'request_at',floor(extract(epoch FROM clock_timestamp()))::bigint,
  'peer_known',true,'asn_known',true,'ipv4',3221225985::bigint,'asn',64500,'browser','','request','')::text,true);
 BEGIN
  PERFORM post_secrets.assert_report_network('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),false,floor(extract(epoch FROM clock_timestamp()))::bigint);
  IF false THEN RAISE EXCEPTION 'Expected CAPTCHA-off board refusal'; END IF;
  IF current_setting('board.report_network_context',true)<>'' THEN RAISE EXCEPTION 'Bypass retained context'; END IF;
 EXCEPTION WHEN SQLSTATE 'P179B' THEN IF NOT false THEN RAISE EXCEPTION 'Unexpected CAPTCHA-off board refusal'; END IF; END;
END $$;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.pass_report_network_bypass(sha256('rpt-actor'),sha256('rpt-anon'),'qpassrpt',current_setting('board.fixture_report_post')::bigint);
  RAISE EXCEPTION 'Public role invoked private network authority';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
